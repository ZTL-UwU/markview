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

The 2026-09-12 refactor comparison used Linux x86_64, an Intel Core Ultra 5
125H with Intel Arc (MTL) through Vulkan, Rust 1.96.0-nightly (2026-03-26),
release mode with thin LTO and one codegen unit, bundled light styles, system
fonts, 18 px text, a 760 px column and a 1200 × 800 offscreen target at scale 1.
Both binaries used the default affinity across all 18 logical CPUs.

| Fixture | First open (ms) | Full pipeline P95 (ms) | Cached pipeline P50 (ms) | RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| ordinary-10k | 26.16 | 13.74 | 0.63 | 45.79 |
| math-10k | 28.93 | 13.76 | 0.72 | 47.74 |
| code-10k | 16.77 | 11.01 | 0.50 | 55.32 |
| long-code-10k | 17.03 | 11.34 | 0.66 | 51.77 |
| images | 46.73 | 4.09 | 0.50 | 47.95 |

The comparison preserved the release binary from commit
`2529179b65f63a32badf02a6e33dd46160ba8783` and alternated it with the candidate
for five groups of 100 full and 100 cached iterations per process. The long-code
fixture needed 15 additional groups because GPU wait times were noisy; all 20
groups were retained. Every acceptance metric stayed within the 5% regression
limit. Tracked GPU capacities were unchanged.

Computing the immutable stylesheet layout identity once per document pass,
instead of once per block lookup, reduced ordinary cached-pipeline P50 from
2.05 ms to 0.63 ms and ordinary full-pipeline P95 from 15.18 ms to 13.74 ms.
Cache identity and invalidation semantics did not change. These are whole-pipeline
measurements: `full_layout_reopens` and `cached_refreshes` summarize `total_ms`,
including read, parse and completed GPU work. Individual `layout_ms` samples
remain available for geometry-only diagnosis.

The local raw reports, environment and binary hashes, early diagnostic runs,
and merged comparison are under `artifacts/refactor/`; these generated artifacts
are ignored by Git. See the [development guide](development.md#refactor-with-a-preserved-baseline)
for the repeatable comparison and merge commands.

## What can change the result

Font discovery and glyph coverage, language shaping, DPI, GPU backend, driver state, image dimensions, long unbreakable runs, and table or formula complexity all affect memory and time. The ordinary-document memory target is an optimization target, not a hard limit for arbitrary input.

Windows and macOS compile checks do not establish native runtime or performance behavior. When a performance-sensitive change is made, compare like-for-like fixtures and report the machine, backend, fonts, build profile, and whether the cache was warm.
