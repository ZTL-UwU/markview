# Architecture

This document explains what the major parts of Markview own and why the boundaries exist. It is intended for someone reading the implementation, not for someone trying to add a feature; procedural guidance lives in [the development guide](development.md).

## The three-layer pipeline

Markview is a read-only desktop application split across three Cargo packages:

```text
Markdown / assets
        │
        ▼
markview-core: semantic document → immutable layout snapshot
        │
        ├── markview-render: snapshot → GPU frame
        │
        └── markview application: files, settings, input, and lifecycle
```

`markview-core` is window- and GPU-independent. It parses Markdown and the supported raw HTML subset, represents semantic blocks and inline content, shapes text, lays out paragraphs, measures math and images, and exposes reading text, selection geometry, links, and draw instructions.

`markview-render` consumes those instructions. It owns the wgpu device and surface, glyph and image resources, clipping, colors that can be changed without reflow, and headless output. It does not contain a second document layout engine.

The root package owns effects that must touch the operating system: launching, file and settings I/O, file watching, image loading, clipboard access, platform link opening, window events, and background work. The UI translates gestures into commands; it does not define document semantics.

The separation matters because the same core layout is used by the interactive window, the renderer tests, and the offscreen render and benchmark modes.

## Semantic identity and immutable snapshots

Parsing produces a `Document` made of blocks and rich inline content. A block keeps its source range for diagnostics and a semantic identity for cache reuse. Source positions are not used as identity: inserting text above a block must not make every later block appear to be a different kind of content.

Layout produces an immutable `LayoutSnapshot`. A snapshot contains final geometry, logical reading text, text clusters, link hit regions, overflow information, and drawing instructions. The renderer and interaction code can therefore read the same result without mutating the layout engine or rebuilding text for copying.

The reading index is deliberately separate from glyphs. Grapheme boundaries, shaping clusters, formula ranges, image fallback text, and code whitespace all need a stable logical mapping even when visual layout inserts hyphens, expands tabs, or replaces an unavailable asset with a placeholder. Selection and copying operate on that logical mapping, so reflow changes rectangles but not the meaning of a selection.

## Versions and asynchronous work

The application distinguishes a content version from a request version. A file open or reload changes content; a type-size, column, alignment, or hyphenation change changes only the requested layout. The worker retains the last parsed document and publishes snapshots tagged with both versions.

Only the newest request may be accepted. A late result cannot replace a newer layout, while a reload cannot be lost merely because a reflow request occupied the worker's single pending slot. Failed reads keep the last usable snapshot, because a transient editor save should not blank the reader.

Images follow the same model. Loading and decoding happen outside layout. A decoded image changes the version of the affected source, causing only dependent blocks to reflow; the document's semantic reading identity, selection, and reading position remain stable.

## Why layout is separate from painting

Paragraphs are shaped before painting because line breaking needs real glyph advances, language-aware break opportunities, hyphenation, inline formulas, and atomic image boxes. Markview uses a bounded Knuth–Plass-style optimizer for ordinary paragraphs and falls back to legal greedy breaks when a paragraph exceeds the candidate budget or has no valid optimized solution. The fallback protects responsiveness without making invalid breaks.

Math is laid out as an atomic display list and images as atomic inline boxes. This keeps their baseline and height in the line model. Images do not create a float band: text never wraps around their sides. An image-only paragraph is centered and may receive a caption; mixed content remains an inline paragraph.

Painting is consequently a projection of an already-decided layout. Scrolling and selection only change which geometry is visible and which overlays are painted. Theme colors can often be late-bound; font, width, spacing, and other geometry changes require reflow.

## Interaction and platform effects

The application owns focus, hover, selection gestures, scrolling, scrollbar grabs, and modal settings input. Core owns hit testing and selection geometry so those operations remain testable without a window or GPU.

Links are activated only on a matching, non-drag release. Web and mail links use the system handler; local files use the system default application, while local `.md` links open as reader tabs. Markdown is never opened for writing. Clipboard output is reading text: code preserves meaningful whitespace, tables use tabs, formulas contribute LaTeX, and Markdown markers are omitted.

Settings are layered as defaults, user TOML, then explicit command-line overrides. Interactive changes may persist user preferences; render, benchmark, and smoke modes intentionally avoid personal configuration so their output is reproducible. Stylesheets are parsed and merged transactionally: an invalid update leaves the last effective stylesheet in place.

## Resource and performance boundaries

The practical performance boundary is not a promise about every Markdown file. Ordinary paragraphs have a line-break budget, image decoding has byte and pixel caps, and CPU image pixels and GPU textures have independent budgets. Caches are bounded or scoped to the current document where possible.

Measured baselines and their environment live in the [performance model](performance.md). Compare like-for-like runs: fonts, drivers, DPI, pathological paragraphs and large assets all affect the result.

## Internal ownership

The package boundaries also apply inside each crate. Entry points compose concrete
components; helpers receive borrowed inputs instead of an application-wide context.

| Component | Owns | Boundary |
| --- | --- | --- |
| Application `Tabs` | Active session, inactive tabs, request serial | Tab transitions return to the window adapter for watching, redraws and requests. |
| Application `Preferences` | Effective settings, persistence store, stylesheet catalog, save deadline | Stylesheet validation finishes before the effective sheet and UI appearance change. The application applies successful changes to the renderer. |
| Application chrome | Borrowed display state | Controls, footer, tabs and styles produce geometry without window, worker or configuration I/O access. Selection-count caching remains in the application adapter. |
| Image scheduler | Versioned entries, jobs and published snapshot | Source reads, bounded decoding and allocation-aware pixel eviction are separate modules. |
| `LayoutEngine` | Document block cache, shaping/math resources, highlight owner | Snapshot assembly and invalidation stay at this entry point; immutable stylesheet identity is computed once per document pass. |
| Block layout context | Borrowed shaper, math engine, image snapshot and completed highlights | Inline preparation, paragraphs, code, images, tables and containers cannot start jobs or invalidate document caches. |
| Renderer `Gpu` | Device, queue, surface and device-loss state | Owns acquisition, resize/recovery, completion and offscreen readback. |
| Renderer raster cache | Atlas, raster keys, scaler and math fonts | Glyph/path preparation borrows the queue; paths write to the shared geometry buffer. |
| Renderer image textures | Texture cache, image runs and current-frame demand | Owns budget checks, version pruning and atomic demand publication. |
| Renderer geometry | Vertices and reusable GPU vertex storage | Produces clipped quads; the frame adapter preserves paint order and submission. |

Parsing and representation have separate source modules: Markdown translation
owns the comrak reader, settings persistence owns the serialized configuration,
and stylesheet parsing owns validation. Reading selection and visual hit testing
share the immutable text nodes without rebuilding their logical content.

Module extraction does not change line-break budgets, cache keys, worker counts,
image budgets, request acceptance, or ordering of draw commands. Hot paths use
concrete types and borrowed resources, with no dispatch registry or additional
synchronization. A source-file target of about 500 production lines is a review
heuristic, not a reason to split an otherwise cohesive algorithm. Tests live next
to their owning modules in separate sources when they obscure production code.
`scene.rs` remains a small exception at about 515 lines: it keeps the shared
immutable drawing, viewport and scrollbar geometry vocabulary together.
