# Performance model

This page explains what Markview measures and how to interpret the current baseline. It is not a change log and does not define a performance guarantee.

## What the timings mean

- **Initialization** covers device, pipeline, font, and renderer setup before a document is opened.
- **First frame** covers reading, parsing, full geometry layout, visible glyph preparation, and GPU completion. It does not include compositor presentation.
- **Full reflow** measures rebuilding document geometry after content or layout settings change. The block cache is cleared for this measurement, while font and shaping resources remain warm.
- **Block refresh** measures reusing unchanged blocks after a localized invalidation, such as an image completing or a file update affecting only part of the document.
- **RSS** is process resident memory after scrolling through the document. It is not GPU memory.
- **Tracked GPU resources** are the capacities of resources Markview can account for; they are not a complete driver-memory report.

The benchmark separates a cold first open from repeated warm reflows. A P95 from repeated runs must not be presented as a cold-start P95.

## Current repository baseline

The latest checked-in measurements used Linux x86_64, an Intel Arc GPU through Vulkan, release mode, Noto Serif/CJK fonts, 18 px text, a 760 px column, and 10 KiB fixtures. They are useful for detecting regressions in the same environment:

| Measurement | Ordinary fixture | Math fixture |
| --- | ---: | ---: |
| First read to completed GPU frame | about 22–25 ms | about 24–25 ms |
| Full reflow P95 | about 9 ms | about 10 ms |
| Block refresh P95 | about 1 ms | about 1 ms |
| RSS after full scroll | about 82 MB | about 84 MB |

A small image-layout comparison measured approximately 13.4 ms with no image, 13.6 ms with one inline SVG, and 14.3 ms with ten repeated inline SVGs on the same machine. The important architectural result is that images participate as atomic inline boxes, so they do not trigger a separate float-layout pass.

## What can change the result

Font discovery and glyph coverage, language shaping, DPI, GPU backend, driver state, image dimensions, long unbreakable runs, and table or formula complexity all affect memory and time. The ordinary-document memory target is an optimization target, not a hard limit for arbitrary input.

Windows and macOS compile checks do not establish native runtime or performance behavior. When a performance-sensitive change is made, compare like-for-like fixtures and report the machine, backend, fonts, build profile, and whether the cache was warm.
