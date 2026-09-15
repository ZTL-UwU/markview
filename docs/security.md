# Security and threat model

Markview's product promise is that opening any file is safe. This page states what that promise covers, which parts of the system an untrusted document can reach, what has already been shown to break, and which risks are knowingly accepted. It owns every security-relevant design decision; [Architecture](architecture.md) owns the pipeline, resource boundaries, and snapshot ownership. A page that needs a security fact links here rather than restating it.

Status: draft, revision 2. This revision records intent. Nothing under [Policy decisions](#policy-decisions) or [Mitigations](#mitigations) is implemented yet; treat those sections as a specification to work from, not as a description of current behavior.

## Summary

Three findings drive the priorities below.

1. **A confirmed, remotely triggerable abort.** A 12 KB Markdown file aborts the process with a stack overflow, on exactly the 8 MiB stack the layout worker requests. No user interaction is required beyond opening the file. See [Appendix A](#appendix-a-confirmed-findings).
2. **The image-path decision does not form a boundary.** Allowing relative image paths still allows `../../../etc/passwd`, because a traversal is a relative path. Bounding this requires a canonicalized prefix check, not a syntactic rule. See [T5](#t5-arbitrary-local-file-read).
3. **Remote image loading is unbounded in count.** Every image whose intrinsic size is unknown is fetched regardless of visibility, so a document with N distinct remote URLs issues N HTTP requests. See [T7](#t7-server-side-request-forgery-and-network-beaconing).

## Scope and assumptions

In scope: a user on an unsandboxed desktop opening a Markdown file from an untrusted source, including downloads, mail attachments, extracted archives, generated text, shared folders, a directory watched with `--watch`, and a second Markdown file reached by following a link.

Out of scope: physical access, kernel and GPU driver defects considered as defects in themselves (they appear only as [T10](#t10-gpu-and-driver-boundary)), social engineering in which the user installs or approves something, and supply-chain compromise of a dependency (tracked as [T4](#t4-memory-corruption-in-a-dependency) and handled separately with `cargo-deny`, `cargo-audit`, and `cargo-vet`).

One structural fact shapes everything below. Markview executes nothing from a document: the raw HTML subset is deliberately limited to semantics Markdown already expresses, and attributes such as `class` and `style` are never interpreted. There are therefore exactly two channels from document content to an effect outside the process:

- `open::that_detached`, reached from document-controlled links
- image I/O, reached from a document-controlled `img src`

The rest of the system is a pure data-to-geometry pipeline. The highest-value conclusion of this model is that those two channels should each be modelled and closed explicitly, and everything else should be held to "does not crash, hang, or exhaust memory."

## Assets

| | Asset | Consequence of compromise |
| --- | --- | --- |
| A1 | Process availability | Loss of what the reader is reading, unsaved session state, an interrupted `--watch` session |
| A2 | Host confidentiality | Local file contents read or rendered |
| A3 | Host integrity | Arbitrary code execution |
| A4 | Network identity | Opening a document alone discloses IP address and online activity |
| A5 | Trust in the reader's own interface | Document content impersonating reader chrome or a security prompt |
| A6 | Clipboard and selected text | Injection through the paste path |

## Trust boundaries

```text
   .md bytes ──(B1)──► comrak ──► Document (pure data) ──(B2)──► Layout ──► Scene ──► GPU (B4)
      │                    │                                            ▲
      │                    ├─► fence info ──► syntect ────────────────┘
      │                    ├─► $...$ ───────► ratex ──────────────────┘
      │                    └─► img src ──┐
      │                                  ▼
      └────────────────────────► (B3) image source resolution ──► local file / HTTP / data:
                                          │
   link click ────────────────────► (B5) open::that_detached ──► OS handler

B1 untrusted bytes → pure data        B2 pure data → geometry (no side effects)
B3 document → filesystem and network  B4 process → driver (not fuzzable)
B5 document → OS execution
```

B3 and B5 carry all of the risk. B2 needs only the safety properties listed under [Security invariants](#security-invariants), which are engineering problems rather than policy problems.

## Attacker capabilities

| | Capability | Realistic setting |
| --- | --- | --- |
| K0 | Controls the `.md` bytes | The baseline; always assume it |
| K1 | Also controls other files reachable by relative path | Extracted archive, cloned repository, shared folder |
| K2 | Controls a remote server the document points at | Common |
| K3 | Controls a stylesheet the user loads | Depends on distribution |

K1 is easy to overlook and is what makes [T6](#t6-arbitrary-file-opened-by-the-operating-system) a code-execution path rather than only a nuisance.

## Threat catalog

| ID | Threat | Capability | Impact | Priority |
| --- | --- | --- | --- | --- |
| T1 | Process abort from unbounded recursion | K0 | A1 | **P0** |
| T2 | Hang or CPU exhaustion | K0 | A1 | **P0** |
| T3 | Memory exhaustion | K0, K2 | A1 | P1 |
| T4 | Memory corruption in a dependency | K0, K1 | A3 | **P0** |
| T5 | Arbitrary local file read | K0 | A2 | P1 |
| T6 | Arbitrary file opened by the OS | K0 + one click | A3 | **P0** (policy) |
| T7 | SSRF and network beaconing | K2 | A4 | P1 |
| T8 | Symlink and time-of-check/time-of-use races | K1 | A2, A1 | P2 |
| T9 | Concurrency state-machine races | K0 triggers | A1 | P2 |
| T10 | GPU and driver boundary | K0 | A1, A3 | P1 (separate track) |
| T11 | Interface impersonation | K0 | A5 | P3 |

### T1: process abort from unbounded recursion

Confirmed and reproducible. `Reader::inlines` in `crates/markview-core/src/document/parse.rs` recurses once per inline AST node with no depth budget, while the sibling `Reader::blocks` in the same file caps recursion at depth 64. Comrak happily produces a deeply nested inline AST: for nested emphasis the depth is half the asterisk count, measured at 10002 levels for a 40 KB input. The layout worker requests an 8 MiB stack, so roughly 10 KB of input is enough to abort the process.

Two properties make this worse than an ordinary bug.

- The stack overflow is a `SIGSEGV` and an abort, so the `catch_unwind` guards that exist elsewhere to keep a malformed file from taking the reader down do not apply.
- Opening is not the only path. `--watch` re-parses on change, and following a link parses another document in the same process.

The same defect class exists wherever a budget was invented locally rather than taken from a shared limit: `visit` in `crates/markview-core/src/style/parse.rs` recurses over nested tables, and the greedy fallback in `crates/markview-core/src/linebreak.rs` has no evaluation budget at all while the optimal pass does.

### T2: hang or CPU exhaustion

Four document-controlled hot spots have no work budget.

| Hot spot | Bound today |
| --- | --- |
| `crates/markview-core/src/highlight.rs`, syntect | None. The language token comes from fence info and the theme from the stylesheet, so both the pattern set and the input are attacker-controlled, and regex backtracking is the risk. A syntect-based service at Sourcegraph has reported unexpected in-production terminations, which is consistent with the component being hard to bound. |
| `crates/markview-core/src/math.rs`, ratex | A 16 KiB input length limit and a 256-entry cache that is cleared wholesale, but no bound on work per formula. A document with many large formulas pays the full cost of each one. |
| `crates/markview-core/src/linebreak.rs`, `greedy` | None. The optimal pass has `BUDGET`, but it is per paragraph and does not cover the fallback. |
| `crates/markview-core/src/layout/table.rs`, wide tables | None. Column and cell counts are document-controlled. |

Comrak's own `MAX_LIST_DEPTH` bounds block-list nesting at 100, and the Knuth-Plass pass is bounded per paragraph; neither covers the cases above.

### T3: memory exhaustion

Decoded pixels are capped at 16 million pixels per image, roughly 64 MB of RGBA8, the decoded cache is capped at 256 MB, and four worker threads decode concurrently. The peak is therefore on the order of half a gigabyte before the source text, the comrak arena, and layout are counted. Memory keys hold the raw source string, so a `data:` URI costs roughly its own size in file bytes; the amplification there is modest, and the decoded cache and concurrent decoders are the real consumers. This needs measurement rather than speculation before any limit is chosen.

### T4: memory corruption in a dependency

The workspace forbids `unsafe_code`, so every unsafe operation reachable from a document lives in a dependency: `image` for six raster decoders, `resvg` and `usvg` and `tiny-skia` for SVG and curve rasterization, `swash` and `parley` for font parsing and shaping, `syntect` for highlighting, and `ratex-*` for math.

Worth knowing when allocating effort: the `image` crate is already continuously fuzzed upstream in OSS-Fuzz, so byte-level raster fuzzing would largely repeat that work. The parts that are *not* covered upstream are Markview's own decode paths: the hand-written ICO entry scan in `src/images/decode.rs`, first-frame selection for APNG and animated WebP, the SVG path that rasterizes at a caller-supplied target size, and the premultiplied-to-straight alpha conversion. No OSS-Fuzz project for `resvg` or `usvg` was found, so SVG rendering is the least covered layer in the stack.

### T5: arbitrary local file read

`source` in `src/images/source.rs` accepts an absolute path, a `file:` URL, or any relative path, and resolves the last against the document directory. A traversal therefore escapes the document directory, and `canonicalize(...).unwrap_or(path)` turns the escape into a canonical absolute path. The only remaining gate is whether the bytes decode as an image.

Severity is bounded by the absence of an exfiltration channel: Markview never concatenates what it reads into a URL, so an attacker learns nothing remotely. The exposure is that a local sensitive file that happens to be an image is rendered, and that the error strings distinguish *missing*, *not a regular file*, and *not an image*, which yields a local existence-and-type oracle.

See [T5 decision](#t5-image-paths) for the resolution rules.

### T6: arbitrary file opened by the operating system

`local_link_path` in `src/app/pointer.rs` returns a canonical path for any local link, and the caller hands anything that is not a `.md` file to `open::that_detached`. On Linux, `xdg-open` on a `.desktop` file runs it, and an `AppImage` or a `+x` script is dispatched according to the user's MIME associations, which Markview does not control. On Windows, `ShellExecute` runs `.bat`, `.cmd`, `.ps1`, `.js`, `.vbs`, `.hta`, `.scr`, and `.msi`, imports `.reg` and `.inf`, mounts `.iso`, and resolves `.lnk` and `.url` to arbitrary targets. On macOS, `open` runs `.command`, `.tool`, `.terminal`, `.workflow`, `.scpt`, and `.app`.

This is the most severe design surface in the repository: with K1 it is code execution from a Markdown link plus one click, and without K1 it is at least an arbitrary dispatch of a local file to whatever the user has associated with it.

Two mitigating facts are already true. The extension test is applied to the canonicalized path, so a symlink named `note.txt` pointing at `payload.desktop` does not bypass it. And the other two `open::that_detached` call sites, in `src/app/interaction.rs`, are UI-initiated, opening the stylesheet directory and the settings file, and are not document-controlled.

See [T6 decision](#t6-local-links).

### T7: server-side request forgery and network beaconing

The decision is to keep remote image loading on by default. The magnitude is nevertheless larger than it appears, because of the scheduling condition in `src/images.rs`: a job is started whenever `e.info.size.is_none()`, which is true for every image in the document that has never been loaded, independent of visibility. Visible images are ordered first, but the total is not bounded. Opening a document with N distinct remote URLs eventually issues N requests, four at a time, each up to 32 MiB.

Consequences:

- A beacon that tells an attacker exactly when a document was opened, and a unique URL per copy identifies which copy.
- SSRF with side effects. A plain `GET` is enough against internal services that do not distinguish it from a state-changing method, so "the attacker gets no response body" is not the same as "no impact".
- Long background activity, and an unbounded wait on the headless path, where `Images::wait` blocks until every entry has a size or an error.

Redirects are already restricted to `http` and `https`, and external references from inside SVG are already disabled by pointing the image-href string resolver at `None`, so an SVG cannot pull in a local file or a URL. Body reads are bounded at 32 MiB. What is missing is a total-count bound and a private-address policy.

### T8: symlink and time-of-check/time-of-use races

A symlink inside the document directory can point anywhere, so any containment check must run after canonicalization. A check followed by an open is still racy: an attacker with K1 can swap the link between the two, so either accept the residual risk or open first and validate the opened descriptor. Separately, the image staleness stamp is `(len, mtime)`, and a writer can preserve both.

### T9: concurrency state-machine races

The application runs the layout worker, four image loaders, a file watcher, and the UI. `Worker` and `Images` each carry their own generation and ticket counters, and `Images::poll` briefly holds the decoded-pixels and demand locks together, which is a lock-order obligation that nothing currently documents or enforces. Coverage-guided fuzzing cannot reach these; they need a model such as `loom` or `shuttle`.

### T10: GPU and driver boundary

Malformed geometry reaches wgpu as validation errors, device loss, or driver defects. This layer has no useful feedback signal for a coverage-guided fuzzer, so the only economic approach is a headless smoke test on a software adapter that opens documents and renders frames without asserting on pixels. Raster images are resized to stay within the 8192-pixel texture dimension, while SVG rasterization is only bounded by a 16-million-pixel check, which is an asymmetry to note but not a defect by itself.

### T11: interface impersonation

The HTML subset interprets no `class` or `style`, so document content cannot adopt reader styling. Headings, link text, and image alt text remain attacker-controlled, which is a low risk recorded here so that it is not re-litigated. It is a P3 non-goal.

## Security invariants

| | Invariant | Status |
| --- | --- | --- |
| I1 | No input at or below the accepted size aborts, panics, hangs, or exhausts memory | Violated by T1 |
| I2 | Every recursion has an explicit depth bound whose violation is a recoverable error | Violated by `Reader::inlines` and `visit` |
| I3 | Decoded pixel totals, cache residency, and concurrent decodes are bounded per document | Partially: per-image and cache bounds exist, totals do not |
| I4 | Document content produces no effect outside the process unless the user confirms an action whose target the document cannot forge | Not yet: see T5 and T6 |
| I5 | Every filesystem path is confined to the document directory subtree | Not yet: see T5 |
| I6 | Rendering the same input is deterministic across runs and processes | Unverified; `HashMap` iteration order is used for image selection in `crates/markview-core/src/layout/paragraph.rs` |
| I7 | Every output geometry value is finite and bounded in magnitude | Unverified; the stylesheet validates only finiteness and sign, so a finite but absurd size such as `1e30` passes |
| I8 | The only URL Markview opens comes from a single scheme allowlist, with no second path | Not yet: `src/app/pointer.rs` is a second path |
| I9 | The number of remote requests and total bytes triggered by one document is bounded | Not yet: see T7 |
| I10 | Any path handed to the OS has had its executability judged for that platform and confirmed by the user | Not yet: see T6 |

## Existing defenses

These are worth keeping and are the reason the risk profile is as good as it is: `unsafe_code` forbidden workspace-wide; ratex pinned to an exact version; per-image pixel, byte, and cache bounds; the 16 KiB formula limit; the `--offline` switch; the redirect scheme policy; the `data:image/` prefix check; the `openable_link` scheme allowlist; the SVG image-href resolver disabled; the texture-dimension clamp for raster images; the depth-64 block guard; and the Knuth-Plass budget.

The gap is structural rather than a series of oversights. Each bound above was invented locally, so the ones that were forgotten are exactly where the failures are: `inlines` missed the depth guard its sibling has, `visit` has none, the greedy fallback missed the budget its sibling has, and highlighting has none at all. A single `Limits` value threaded through parsing, styling, math, highlighting, and layout, from which every recursion and allowance is drawn, is the structural answer. Patching instances one at a time will leave a fifth one.

## Policy decisions

These are the decisions taken for this revision. Each is followed by the gap that remains before it holds.

### T5: image paths

Decision: relative image paths are allowed; absolute paths and `file:` URLs are not.

Gap: as written this does not bound anything, because `../../../etc/passwd` is a relative path. To make the decision mean what it intends, containment must be enforced after resolution:

```text
1. Reject an empty src.
2. Parse as a URL. Accept http and https only. Remove the file: branch.
   Keep data:image/ as an inline source with its existing size cap.
3. Otherwise treat as a path:
   a. Percent-decode.
   b. Reject if any component is a prefix or root: this covers absolute paths,
      UNC paths, \\?\ and \\.\ forms, and, on Windows, C:foo (drive-relative)
      and \foo (rooted, yet is_absolute() is false).
   c. Canonicalize doc_dir.join(decoded). On failure, reject; do not fall back
      to the unresolved path.
   d. Reject unless the canonical result starts with the canonical document
      directory. This is the boundary, and it also closes symlink escape.
4. After opening, confirm a regular file through the opened descriptor.
```

Step 3d is what turns the decision into an invariant. Steps 3c and 3d together also close escape through a symlink placed inside the document directory, because the check runs on the resolved target. The residual is the check-then-open race in T8.

### T6: local links

Decision: local links are allowed to open `.md` files and common text and code files, and anything else prompts a warning rather than being refused outright.

Gap: "common code" is not a portable category. `.py` is text on Linux and is executed by the interpreter association on Windows; `.js` is text on Linux and is executed by Windows Script Host. An allowlist must therefore be per platform, and a warning on a category that is known to be executable is a speed bump rather than a control. The recommended shape:

| Class | Behavior |
| --- | --- |
| A | `.md` parsed in a new tab; known text and code extensions rendered as plain text **inside Markview**, bypassing the OS entirely |
| B | A small allowlist of inert non-text types (`.pdf`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.ico`, and common audio and video) dispatched to the OS |
| C | Everything else: a blocking confirmation, defaulting to cancel |
| D | Known-executable types refused: `.desktop`, `.AppImage`, `.run`, `.lnk`, `.url`, `.bat`, `.cmd`, `.ps1`, `.js`, `.vbs`, `.hta`, `.reg`, `.inf`, `.msc`, `.scr`, `.com`, `.pif`, `.cpl`, `.msi`, `.command`, `.app`, `.scpt`, `.workflow`, `.action`, `.dmg`, plus any file with an executable bit on Unix |

Class A is the substantive suggestion: rendering text and code in-process satisfies the intent of the decision with no external dispatch at all, and it is a better reading experience than handing a file to an editor. Class D deliberately departs from "warn rather than refuse", because user confirmation is the least reliable control in the chain; if the departure is rejected, class D collapses into class C and the residual risk should be recorded here.

The confirmation must be blocking, default to cancel, offer no "remember this choice", and display the canonical absolute path rather than the link's label text, since the label is attacker-controlled.

### T7: network access

Decision: remote images remain enabled by default.

Gap: the default stands, but three bounds should be added, none of which change the default.

1. A per-document cap on the number of images fetched, with the remainder left at the placeholder size until they come into view. `ImageSpec::size` already has a placeholder path and the layout already re-runs when image metadata changes, so deferral is architecturally supported.
2. Prefetch limited to the visible region plus a bounded lookahead.
3. Refuse loopback, link-local, and private addresses (`127.0.0.0/8`, `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, `::1`, `fc00::/7`, `fe80::/10`) on the initial URL and on every redirect. A DNS-rebinding-resistant implementation re-checks after resolution, before connecting.

Item 3 blocks the legitimate case of a local document referencing a local service. Since Markview cannot tell whether a document is trustworthy, the recommendation is to apply it unconditionally and explain the refusal in the placeholder text.

## Accepted and residual risks

| Risk | Why accepted |
| --- | --- |
| A4 under T7: remote images are fetched by default, so opening a document discloses the reader's address | Product decision; mitigated by the bounds above, and the `--offline` switch remains available |
| T8: check-then-open race and a forgeable staleness stamp | Requires a local adversary with write access to the document directory; the containment check in T5 removes the unconditioned form |
| T9: unverified lock-ordering obligation in `Images::poll` | To be covered by a concurrency model rather than by review |
| T10: driver defects | Out of scope as defects in themselves; covered only by a headless smoke test |
| T11: interface impersonation through headings, link text, and alt text | The HTML subset interprets no `class` or `style`, so the practical surface is small |
| No exfiltration channel for T5 | Markview never places file content into a URL, so a local read cannot be reported back to a remote party |

## Mitigations

Structural, in order of value:

1. One `Limits` value threaded through parsing, styling, math, highlighting, and layout, from which every depth, byte, pixel, iteration, and time allowance is drawn.
2. Recursion replaced by an explicit stack, or given a budget, wherever it currently relies on input size.
3. A pinned font for layout instead of loading system fonts, which is both a reproducibility requirement for testing and a determinism requirement for I6.
4. The single URL allowlist for I8, and the containment check for I5.

Engineering, in order of cost:

| Step | Cost | Covers |
| --- | --- | --- |
| Run the test suite under `cargo careful` | Minutes | Undefined behavior in dependencies that the standard library can detect |
| ASAN-instrumented fuzzing of decode, SVG, and shaping | Hours | T4 |
| `loom` or `shuttle` models of `Worker` and `Images` | Days | T9 |
| `kani` proofs for the integer and slice logic in `html`, image-source resolution, and the link allowlist | Days | Boundary errors in I5 and I8. Not applicable to the float-heavy line breaker, where Kani's support is poor |
| `miri` over the dependency-free logic modules | Days | Requires extracting those modules, since the font stack is FFI |

## Verification plan

Each invariant maps to a harness with an oracle, rather than to "no crash". Crash-only fuzzing is weak here: a 15-day run of 30 billion inputs against a CommonMark parser produced 594 new-interesting inputs and no bugs, which is the expected outcome for a pure parser under byte-level mutation. The value is in the deeper stages and in the oracles.

| Target | Harness | Oracle |
| --- | --- | --- |
| I1, I2 | Parse and layout, plus a dedicated line-breaker harness over unit vectors | No abort, no panic, a recorded recursion bound, a time budget |
| I3, I9 | Layout with a synthetic image snapshot; decode in isolation | A counting global allocator asserting peak allocation against input length; a request counter |
| I4, I5, I8 | Source resolution over `(src, document path)` | Containment and scheme predicates from the T5 and T6 tables |
| I6 | Layout twice in one process and across processes | Field-by-field equality after serialization |
| I7 | Layout over generated numeric options | Finiteness and magnitude bounds on every geometry value |
| T4 | Decode, SVG rasterization, and shaping | ASAN cleanliness |
| T2 | Highlight, math, and line breaking, each with a timeout | Wall-clock budget per input |
| Differential | Cached versus uncached layout; progressive prefix versus final snapshot | Field-by-field equality. Both properties are already asserted in unit tests and should be enforced during fuzzing |

Input generation should be structured rather than byte-level. Comrak exposes an `arbitrary` feature that derives `Arbitrary` for its option types, which makes randomized configuration free, and the comrak repository already carries a fuzz suite whose targets include a complexity-focused one and a `sourcepos`-focused one that exercises the same positions `Reader::range` depends on. See [Appendix B](#appendix-b-existing-fuzzing-assets).

## Non-goals

- Making a hostile document safe to *act on*. Only the reader is hardened, not the user's judgement.
- Defending against a local attacker who can already write to the document directory, beyond the containment check and the recorded residuals.
- Treating driver defects, or wgpu validation failures caused by driver behavior rather than by Markview's inputs, as findings.
- Restricting which content a document may *display*. Styling attributes are not interpreted, and impersonation is a recorded P3 risk.

## Appendix A: confirmed findings

**T1 — stack overflow in `Reader::inlines`.** Reproduced outside the application, on a thread with the same 8 MiB stack size that `Worker` requests in `src/worker.rs`.

```sh
python3 -c "n=6000; print('*'*n + 'a' + '*'*n, end='')" > nested.md
```

The file is 12001 bytes. Observations against the current tree:

| Input | Result |
| --- | --- |
| 8001 bytes (`n = 4000`) | Parses, one block |
| 12001 bytes (`n = 6000`) | `fatal runtime error: stack overflow, aborting`, exit 134 |
| Comrak alone, 8 MiB stack, 50000 levels | Parses; the AST is built iteratively |
| Comrak alone, nested emphasis, 40 KB | AST depth 10002 |

So the recursion is Markview's, not the parser's, and the trigger is small enough to arrive inside an ordinary document. The fix is a depth budget in `Reader::inlines` matching the one `Reader::blocks` already has, or an explicit stack; either way the input belongs in a regression test.

## Appendix B: existing fuzzing assets

Nothing off the shelf generates Markdown input in Rust, but the following are worth reusing rather than rebuilding.

| Asset | Use |
| --- | --- |
| [comrak's own fuzz suite](https://github.com/kivikakk/comrak/blob/master/fuzz/Cargo.toml) | Nine targets covering parse, CommonMark, GFM, source positions, footnotes, all-options, CLI defaults, and a complexity-focused target. Markview depends on the same version, so it adapts with a path change. |
| comrak's `arbitrary` feature | Derives `Arbitrary` for the option types, so randomized configuration needs no generator. |
| [pulldown-cmark's `pandoc` target](https://github.com/pulldown-cmark/pulldown-cmark/blob/master/fuzz/fuzz_targets/pandoc.rs) | A template for differential fuzzing. The applicable differential here is comrak's HTML output against Markview's block extraction. |
| [tree-crasher](https://github.com/langston-barrett/tree-crasher) with [tree-sitter-markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown) | Grammar-aware mutation without instrumentation. tree-crasher ships C, CSS, JavaScript, Regex, Rust, SQL, TypeScript, HTML, OpenSCAD, and Ruby, but not Markdown; the grammar exists, so adding a `tree-crasher-markdown` crate is small. Its HTML front end applies to the raw HTML subset directly. |
| [codec-corpus](https://docs.rs/codec-corpus) | Image test corpora, including PngSuite, as a development dependency. |
| [OSS-Fuzz: cmark](https://github.com/google/oss-fuzz/tree/master/projects/cmark) and [md4c](https://github.com/google/oss-fuzz/tree/master/projects/md4c) | Reference engineering for Markdown fuzzing, including corpus and dictionary layout. |
| [OSS-Fuzz: image-rs](https://github.com/google/oss-fuzz/tree/master/projects/image-rs) | Confirms the `image` crate is fuzzed upstream; do not duplicate it. |
| [librsvg's OSS-Fuzz work](https://gitlab.gnome.org/GNOME/librsvg/-/work_items/1096) | SVG seed corpus and a render-focused target. No equivalent project was found for `resvg` or `usvg`. |
| CommonMark and GFM spec suites, KaTeX test cases | Seeds for parsing and math. Correctness corpora, not crash corpora; pair them with generated extremes. |
| [bolero](https://github.com/camshaft/bolero) | One harness that runs as a coverage-guided fuzzer, a property test, or a Kani proof. |
| [aretext's report](https://devnonsense.com/posts/aretext-markdown-fuzz-test/) | The expectation-setting data point cited under [Verification plan](#verification-plan). |
