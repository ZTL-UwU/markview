# Development guide

This page is for contributors changing Markview. It is procedural: the design rationale is in [architecture](architecture.md), and the MVSS format is in [the stylesheet guide](stylesheets.md).

## Build and verify

Use the locked workspace commands from the repository root:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
```

For visual or timing changes, also use the real pipelines:

```sh
target/release/markview --render examples/welcome.md --output artifacts/welcome.png
target/release/markview --smoke-test examples/welcome.md --output artifacts/window.png
target/release/markview --bench tests/fixtures/ordinary-10k.md --output artifacts/ordinary.json
python3 scripts/smoke_watch.py target/release/markview
```

The render and benchmark modes use the GPU offscreen and do not load personal settings. The watch smoke test writes only temporary documents and closes the window it starts. Ignored GPU tests are useful for settings, selection, and image-frame regressions:

```sh
cargo test --workspace --locked settings_and_selection_frame -- --ignored
cargo test --workspace --locked gpu_frame_draws_decoded_images -- --ignored
```

## Choose the layer

1. Put Markdown meaning, reading text, geometry, hit testing, and selection mapping in `crates/markview-core`.
2. Put GPU resources, clipping, rasterization, and frame assembly in `crates/markview-render`.
3. Put files, settings, watching, image I/O, platform effects, commands, and window interaction in the root crate.

Keep core free of window, GPU, clipboard, filesystem, and configuration dependencies. Prefer immutable snapshots and explicit version tags at asynchronous boundaries. Reuse the retained `Document` when only layout settings change.

## Add a document node

Use this sequence when adding a Markdown or HTML construct:

1. Identify the semantic input and its intended reading-text and copy behavior. Do not begin with a renderer primitive.
2. Extend `BlockKind`, `InlineKind`, or the relevant style data in `crates/markview-core/src/document.rs` (and `html.rs` for supported raw HTML).
3. Parse the construct into that semantic representation, preserving a source range and stable reading order. Keep unsupported syntax as literal text rather than silently dropping it.
4. Add layout behavior in `layout.rs`. Decide whether the node is text, a block, or an atomic inline box; produce text nodes and geometry together so hit testing and copying use the same mapping.
5. Add only the semantic paint instructions needed by the renderer. Do not make the renderer reinterpret Markdown.
6. Update stylesheet roles only if the node has a visual role that cannot use an existing one. Validate the role through the same MVSS parser as bundled and user styles.
7. Add focused unit tests for parsing, source ranges, reading text, selection/copying, layout, and cache identity. Add a renderer test only for GPU-specific behavior.
8. Add a fixture or example when the feature is difficult to understand visually, then run the full workspace checks.

Common mistakes are treating source offsets as persistent identity, putting display-only text into copied reading text, using glyphs as the selection model, and creating a second layout path for a special inline object. These produce subtle bugs in reflow, repeated blocks, CJK selection, and asynchronous reloads.

## Change asynchronous behavior safely

When a worker or loader changes, test stale-result rejection, replacement of pending requests, failed reads, and versioned image completion. Preserve the last valid snapshot on recoverable errors. When a change affects only colors or interaction overlays, avoid invalidating geometry; when it affects fonts, width, spacing, or intrinsic asset size, invalidate the relevant layout.

## Keep documentation maintainable

Document guarantees and reasons in architecture pages, procedures and examples in guides, and measured facts in the performance page. Remove a stale statement instead of adding a contradictory exception. Link to the owning page rather than copying the same rule into several documents.

