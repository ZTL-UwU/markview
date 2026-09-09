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
special cases.

The application owns drag selection, Shift-click extension, double-click word
selection, triple-click block selection, select-all, copying, edge autoscroll and
gesture cancellation. Links activate on matching release only when the gesture has
not become a drag, so a double-click inside a link selects text instead of opening
it. Reflow preserves logical selection;
accepting semantically different contents clears it, while a metadata-only reload
or a failed reload preserves it. The settings panel consumes pointer events in its
own region and keyboard events when a panel control has focus; clicking the
document returns keyboard focus to the reader.

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
