# Stylesheet guide

Markview stylesheets are UTF-8 TOML files with the `.mvss.toml` suffix. They define portable visual themes. Personal reading preferences—font size, column width, alignment, and hyphenation—belong in `settings.toml`, not in a stylesheet.

## Install and select a style

```sh
markview ss install paper.mvss.toml
markview document.md --style paper
markview document.md --style paper --style dark
```

Installation validates the complete file and copies it to the user stylesheet directory. It does not install fonts or enable the style. Use `--force` to replace an installed style whose `version` is equal to or lower than the incoming version.

The directory is next to `settings.toml`:

| Platform | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/styles/` or `~/.config/markview/styles/` |
| macOS | `~/Library/Application Support/markview/styles/` |
| Windows | `%APPDATA%/markview/styles/` |

The filename without `.mvss.toml` is the style ID. Only the first directory level is scanned. The built-in `light` and `dark` IDs are reserved.

In the Settings panel, **Styles…** lets you enable, disable, and reorder styles. The leftmost selected style has the highest priority. `--style` replaces the session's selected list and is not saved. It cannot be combined with `--light` or `--dark`.

## Minimal valid file

```toml
format_version = 1
version = 1

[meta]
name = "Paper"
description = "Warm reading theme"

[body]
color = "#292524"
background = "#FAF8F2"
font = [{ family = "serif" }]
line_height = 1.65

[link]
color = "#315D86"
decoration = ["underline"]
```

`format_version` describes the file format; `version` is the installed theme's revision. Both are required and `version` must be a non-negative integer. `meta` is optional and does not participate in styling.

## Roles and properties

Roles are semantic names, not CSS selectors. Supported roles are:

| Area | Roles |
| --- | --- |
| Blocks | `body`, `p`, `h1`–`h6`, `blockquote`, `list`, `list_item`, `footnote` |
| Inline content | `em`, `strong`, `strong_em`, `link`, `code`, `del`, `sup` |
| Code and media | `code_block`, `code_block.label`, `img`, `img.caption`, `img.placeholder` |
| Tables and marks | `table`, `table.header`, `table.cell`, `list.marker`, `task_marker`, `hr`, `math` |
| Reader and UI | `selection`, `scrollbar`, `ui`, `ui.toolbar`, `ui.statusbar`, `ui.panel`, `ui.button` |

Text roles accept `color`, `font`, `weight`, `size`, and `decoration`. Block roles additionally accept `line_height`, `space_before`, and `space_after`; block containers accept `padding`, `border_color`, `border_width`, and `radius`. Inline roles do not accept container geometry.

Special properties include `align` on images, `source` on captions, scrollbar colors and thicknesses, and `shadow`/`scrim` on `ui`. The UI theme controls appearance, not widget layout or dimensions.

Colors are sRGB `#RRGGBB` or `#RRGGBBAA`; `body.background` must be opaque. Sizes and spacing are positive or non-negative finite values. `size` is relative to the reader's base size, `line_height` is a multiple of the role's size, and spacing/padding use base-size units. Border width and radius use logical pixels. Unknown roles, fields, types, and enum values are errors.

## Cascade and inheritance

Stylesheets are merged from left to right by role. A field omitted by a higher-priority style remains from the lower-priority style; arrays replace the entire lower-priority array. The final role is then applied with document-text inheritance.

Text properties inherit from the containing block. Backgrounds, borders, padding, and spacing do not inherit. There are no variables, selectors, `inherit`, `unset`, imports, scripts, or remote resources.

`strong_em` combines the `em` font with the `strong` weight unless it explicitly supplies a value. Inline conflicts resolve by semantic precedence: block, emphasis, link, deletion, superscript, then code.

## Fonts and fallback

Fonts are named by ordered candidates. A candidate must reference an installed family or one of the generic families `serif`, `sans-serif`, and `monospace`:

```toml
[[fontdef]]
id = "reading"
lookfor = ["Noto Serif", "Georgia"]

[body]
font = [{ family = "reading" }]
```

Use `variant = "normal"`, `"italic"`, or `"oblique"`, and an optional weight from 1 to 1000. Markview skips a candidate when the face, requested style, or complete grapheme cluster is unavailable; it does not synthesize slant or weight. CJK variants may be defined with `type = "SC"`, `"TC"`, or `"JP"`. A user may override a definition with `[[fontdef-override]]`, but stylesheet files cannot bundle font files or download them.

## Images and captions

```toml
[img]
align = "center"
padding = 0.3
border_width = 1.0
border_color = "#D8DEE3"

[img.caption]
source = "title_or_alt"
align = "center"
size = 0.8
color = "#69747E"
```

`align` affects image-only paragraphs. Images mixed with text remain inline and never create text wrapping on their sides. A single image paragraph may show a caption, using `title_or_alt`, `title`, `alt`, or `none`; multiple-image and mixed paragraphs do not show captions. `img.placeholder` styles loading and error text.

## Live updates and safe authoring

Markview watches installed styles and settings. A valid save applies automatically; an invalid stylesheet leaves the previous effective style active. Color-only changes can repaint cached layout, while font and geometry changes reflow it.

Keep a style focused on visual decisions, use semantic roles rather than trying to imitate CSS, and test it with both Latin and CJK text, formulas, code, tables, links, selections, and missing images. Do not rely on a font that is unavailable on the target machine; provide an ordered fallback list.
