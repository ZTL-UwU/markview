# Reading architecture

Markview is a single read-only application in a three-package Cargo workspace.
The root `markview` package remains the default executable; all packages share
one lockfile, release profile, dependency versions and lint configuration.

```text
markview ──→ markview-render ──→ markview-core
    └────────────────────────→ markview-core
```

- **core** owns Markdown/HTML semantics, shaping, paragraph optimization,
  mathematics, immutable drawing geometry and logical reading text. It has no
  window, GPU, clipboard, filesystem observation or configuration dependencies.
- **render** resolves semantic stylesheet colors and owns GPU resources, raster caches, window surface
  recovery, clipping and offscreen output. A small `TextShaper` supplies fallback
  glyphs; rendering does not instantiate a document layout engine.
- **application** owns launch modes, platform effects, settings, reader sessions,
  input routing, file loading, observation and background processing. `app/chrome`
  draws controls; `app/interaction` translates commands and selection gestures.

## Ownership and updates

`LaunchOptions` contains only CLI inputs. `ReaderSettings` is the effective
runtime preference set. `ReaderSession` owns document/layout versions and reading
position. `InteractionState` owns focus, selection, hover and pointer gestures.
Window and GPU handles, watcher, worker and clipboard are application resources.

A worker retains the last parsed `Arc<Document>`. Requests carry a monotonically
increasing request version and a content version. Opening a file or observing a
file change advances the content version; changing layout preferences advances
only the request version. Even when a reflow replaces a pending reload in the
single-slot inbox, its content version still requires that reload.

The worker publishes `ReaderSnapshot { document, layout, content_version }`.
The application rejects stale request versions before atomically accepting the
text and geometry. Failed reads keep the last accepted snapshot. Cached documents
are shared, not copied into the UI. File observation and bounded file loading are
separate services and do not own the layout engine.

| Change | Work |
| --- | --- |
| File change/open | Read, parse, layout |
| Font size, effective column width, alignment, hyphenation | Layout retained document |
| Theme | Repaint |
| Pointer, selection, scroll | Interaction and repaint |
| DPI | Clear raster cache; reflow if effective logical width changed |
| Image decoded or file changed | Layout retained document; repaint |

## Reading text and selection

Each cached block contains logical `TextNode`s generated from its semantic text,
plus final `TextCluster` geometry. A position identifies the content revision,
top-level block occurrence, node ordinal and UTF-8 boundary with affinity. Node
ordinals follow semantic reading order, including nested content and table cells;
line wrapping does not change them. Content hashes are cache keys, not unique
selection identities. Repeated blocks can share geometry without sharing positions.

Glyphs carry no text identity. The reading index preserves grapheme boundaries,
shaping cluster direction and final justification widths. Formula display
placeholders and fallback glyphs map to the same atomic LaTeX range. Inserted
hyphens have no logical range; code tab expansion maps back to the original tab.
Code language badges are decorations. Tables copy in row order with tabs between
cells; ordinary paragraphs use blank lines; list markers remain readable.

`hit_test_text`, `selection_rects[_in]` and `extract_text` are GPU-independent.
`hit_test_text` binary-searches blocks by `y` and stops once a block cannot be
nearer, so pointer cost does not grow with the document above the cursor.
`Viewport` defines logical window/document conversion, and `command_view` defines
local overflow translation and clipping for painting, selection and links.
Selection painting visits only vertically visible blocks and is ordered above
block backgrounds but below glyphs, so a highlight never tints text. The benchmark
reports retained text-index allocation capacity separately from process RSS and
GPU bytes.

`select_word_at` resolves double-click ranges with ICU dictionary segmentation
(`WordSegmenter::new_auto`), so a Chinese or Japanese run yields dictionary words
instead of one segment per character. The hit's affinity recovers the grapheme
under the pointer, punctuation and emoji select their own cluster, and whitespace
selects the adjacent word. `select_block_at` backs triple-click. Both return
ordinary logical selections, so reflow, copying and selection counts need no
special cases. `TextCounts` reuses the same segmentation, so the footer word count
and double-click agree; a segment counts as a word when it carries a letter or a
digit, which also covers a base letter followed by a combining mark.

The application owns drag selection, Shift-click extension, double-click word
selection, triple-click block selection, select-all, copying, edge autoscroll and
gesture cancellation. A press that already selected a word or block keeps its
`Grain`, so dragging extends the range by whole words or blocks from the fixed
edge of the multi-click selection. Links activate on matching release only when
the gesture has not become a drag, so a double-click inside a link selects text
instead of opening it. Reflow preserves logical selection;
accepting semantically different contents clears it, while a metadata-only reload
or a failed reload preserves it. Scrollbars are draggable: the document's
vertical bar and each wide block's horizontal bar share one mapping between thumb
position and scroll offset, a press on the thumb keeps the pointer's grab offset
so the thumb never jumps, and a press on the empty track first moves the thumb
under the pointer, which then continues as the same drag. A scrollbar drag holds
the pointer grab, so it keeps following the pointer past the window edges until
the button is released. Bars are drawn thin but grabbed across their full
interactive thickness, and the document bar thickens to that thickness while
hovered or dragged; the layout reserves a gutter below every overflowing block so
a horizontal bar never crowds the last line of text. The `[scrollbar]` section of
a stylesheet sets both bars' rest and hover thicknesses and that gutter, so a
theme can thicken the wide-block bar too or reserve no space at all. The settings
panel consumes pointer events in its own
region and keyboard events when a panel control has focus; clicking the document
returns keyboard focus to the reader.

## Images

MVSS exposes `img` for the frame and padding, `img.placeholder` for loading/error
text, and `img.caption` for a single-image paragraph's optional caption. Caption
source and alignment participate in the geometry cache key; colors remain late
bound. Captions and placeholders have character-level reading geometry and can
be selected and copied like paragraph text. Hidden captions retain an empty
node slot so subsequent nested paragraphs and cells keep their node ordinals.
Placeholder ellipses map to the omitted full error text. When visible text
changes, selection endpoints are rebased across unchanged prefixes/suffixes;
selections containing replaced text are cleared, and counts are refreshed.

`markview-core::image` holds image semantics (`ImageSpec`), immutable decoded
pixels (`Pixels`), per-source metadata (`ImageInfo`) and an `ImageSnapshot` that
pairs metadata with a pixel cache shared across layout snapshots. Layout only
reads intrinsic sizes from the snapshot, so it never blocks on I/O.

An image alone in its block is a centered figure; mixed with text it is an atomic
inline box like a formula. It occupies one line-break unit, the line grows to its
height and the text before and after it stays on the same line while there is
room. Images never reserve a float band, so text never wraps beside them. The
block cache key includes each image version and measured size, so a load or a
file change relayouts only the affected blocks and never changes reading text.

The application owns `images`, a bounded loader. Sources resolve relative to the
document directory, and `file:`, `http(s):` and `data:` are recognized;
`--offline` rejects network sources. Four threads fetch and decode with a 32 MiB
byte cap, a 16 million pixel cap and a 15 s network timeout, and redirects stay
on http(s). Bitmaps come from `image` (PNG, JPEG, GIF, WebP, BMP, ICO), including
the first frame of animated GIF, WebP and APNG; anything else is parsed as SVG by
resvg, which loads no external resources or scripts and shares one system font
database. Loader threads turn decoder panics into errors, so a malformed file
cannot take the reader down. When pixels arrive the worker lays out again with
the same content version, which preserves selection and reading position.

`markview-render` uploads straight-alpha sRGB RGBA8 textures on demand, keyed by
source and image version and sampled with linear filtering. The renderer
publishes a complete frame's image demand atomically. Repeated uses and aliases
request their maximum physical size, so a smaller later occurrence cannot lower
SVG quality. GPU-resident images do not require evicted CPU pixels to be fetched
again. Headless rendering waits for this size negotiation before exporting.
CPU pixel residency and GPU textures are each budgeted at 256 MiB (decoder
scratch space is separate); aliases count as one CPU allocation and are evicted
together. A texture larger than the device limit is never uploaded. Renaming a
reference to another alias reuses the decoded pixels. Each load has its own
ticket, so completion of a removed reference cannot replace a re-added one.

## Preferences and platform effects

The settings panel, toolbar and shortcuts dispatch the same commands. Settings
are TOML with `version = 1`, persisted after a 250 ms quiet period and flushed on
normal exit. A same-directory temporary file replaces the old configuration.
Malformed or unsupported configurations remain untouched on load; before a user
change is saved, their bytes are preserved in `settings-invalid-*.toml`.
The settings file has its own parent-directory watcher with bounded debounce and
polling fallback. External edits update controls and reflow only when layout
options change. Invalid or missing files retain the last good settings. Pending UI
fields win conflicts; other external fields and TOML comments survive UI saves.
The centered translucent modal captures input; outside click or Escape closes it.

Window startup applies defaults, then user settings, then explicit CLI overrides.
An absent `style` list follows the desktop light/dark preference. A user choice
saves an ordered stylesheet list; an empty list fixes the light base. Reset
returns to following the system. Only fields actually changed through controls are written back;
launch overrides alone never become preferences. Reset explicitly replaces all
reading settings.
Render, benchmark and smoke modes use defaults plus CLI flags and do not load or
save personal configuration. Persistence errors do not revert session settings.

The configuration locations are `XDG_CONFIG_HOME/markview/settings.toml` (falling
back to `~/.config`), macOS `~/Library/Application Support/markview/settings.toml`,
and Windows `%APPDATA%/markview/settings.toml`. Window startup creates this file
when absent, migrating a valid sibling `settings.json` without deleting it.

Clipboard integration uses arboard with Wayland data-control support, retaining
its handle for the application's lifetime. Clipboard failures appear in the status
bar. Markdown input is never opened for writing. There is no document editing
buffer, insertion cursor, IME editing path, undo history or save command.

## Validation

Run `cargo test --workspace --all-targets --locked`, workspace clippy, formatting
and release build. `cargo test --workspace --locked settings_and_selection_frame
-- --ignored` uses the actual GPU pipeline and writes `artifacts/refactor-ui.png`;
it is a local regression check, not part of CI. Existing CLI rendering, benchmark
and native reload smoke tools remain available. Windows/macOS cross checks verify
compilation, not native runtime behavior.

## Stylesheet pipeline

`markview-core::style` strictly parses MVSS v1, merges sparse rules, resolves
semantic text inheritance, and computes a geometry-only cache identity. Bundled
light/dark TOML files use the same parser as user files. Glyphs carry compact
semantic color ancestry, allowing color changes (including newly added rules)
to repaint cached layouts without reshaping. Font candidates have independent
face requirements and are checked for real style, weight and cluster coverage
before Parley shapes the paragraph.

The application owns stylesheet discovery, installation, selection and directory
watching. The user `styles/` directory sits beside `settings.toml`. Loading a
selected list is transactional: failures retain the last effective stylesheet,
while the requested list remains available for diagnosis and repair. Rendering
and layout receive the same effective stylesheet. See [the MVSS reference](stylesheets.md).
