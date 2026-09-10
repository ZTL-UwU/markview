# Documentation map

Markview's documentation is deliberately split by reader intent. Each page has one job.

## Start with the product

The root [README](../README.md) is the user-facing entry point: installation, reading controls, supported content, limitations, and a short customization example.

## Understand the implementation

- [Architecture](architecture.md) explains ownership, snapshots, versions, layout, interaction, and resource boundaries. It focuses on what the system guarantees and why.
- [Performance model](performance.md) explains the measured terms, current baselines, and the limits of those numbers.

## Change the implementation

- [Development guide](development.md) is the how-to page for building, testing, changing behavior, and adding a new document node.
- [Stylesheet guide](stylesheets.md) is the how-to/reference page for authoring and installing MVSS themes.

When a fact belongs to more than one page, keep the detailed explanation in the page that owns the concept and link to it elsewhere. In particular, keep commands and procedures out of architecture documentation.

