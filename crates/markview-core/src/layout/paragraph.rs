use super::inline::is_cjk;
use super::{BlockContext, LayoutOptions};
use crate::{
	document::{CellAlign, Inline, InlineKind, TextStyle},
	linebreak::{self},
	scene::{BlockLayout, Draw, LinkRect, Overflow, Rect},
	style::{ColorField, Decoration, Role},
	text::{TextCluster, TextNode},
};
impl BlockContext<'_> {
	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	pub(super) fn paragraph(
		&mut self,
		rich: &[Inline],
		x: f32,
		y: f32,
		width: f32,
		size: f32,
		sans: bool,
		align: CellAlign,
		justify: bool,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let p = self.prepare(rich, size, out);
		let node = out.text.len();
		out.text.push(TextNode::new(p.reading.clone(), ""));
		if p.text.is_empty() {
			return size * self.shaper.appearance.line_height;
		}
		let units = self.units(&p, size, sans, opts.hyphenate && !sans, width);
		let solution = if opts.greedy {
			linebreak::greedy(&units, width)
		} else {
			linebreak::break_lines(&units, width, justify)
		};
		out.degraded += usize::from(solution.degraded && !opts.greedy);
		let mut y_cursor = y;
		// An image alone in its block is a centered figure; mixed with text it
		// is an ordinary atomic inline box in the line flow.
		let only_images = !rich.is_empty()
			&& rich.iter().all(|i| {
				matches!(&i.kind, InlineKind::Image(_))
					|| matches!(&i.kind, InlineKind::Text(t) if t.trim().is_empty())
			});
		let align = if only_images && align == CellAlign::Left {
			opts.stylesheet
				.rule(Role::Image)
				.align
				.map(Into::into)
				.unwrap_or(CellAlign::Center)
		} else {
			align
		};
		let mut lines: std::collections::VecDeque<_> = solution.lines.into();
		while let Some(mut line) = lines.pop_front() {
			if line.units.is_empty() {
				y_cursor += size * self.shaper.appearance.line_height;
				continue;
			}
			let range = units[line.units.start].source.start
				..units[line.units.end - 1].source.end;
			let mut clusters =
				self.line_clusters(&p, range, line.hyphen, size, sans, width);
			let mut natural: f32 = clusters.iter().map(|c| c.width).sum();
			// Boundary reshaping (ligatures, kerning, inserted hyphens) can alter
			// the measured advance. Move to an earlier legal break and reoptimize
			// the remaining paragraph, rather than horizontally scrolling ordinary text.
			loop {
				let shrink: f32 = if justify && !line.last {
					clusters
						.iter()
						.filter(|c| p.text.get(c.range.clone()) == Some(" "))
						.map(|c| c.width * 0.3)
						.sum()
				} else {
					0.0
				};
				if natural - shrink <= width + 0.1 {
					break;
				}
				let Some(end) = (line.units.start + 1..line.units.end)
					.rev()
					.find(|&end| units[end - 1].after.is_some())
				else {
					break;
				};
				let br = units[end - 1].after.unwrap();
				line.units.end = end;
				while line.units.end > line.units.start
					&& units[line.units.end - 1].discard
				{
					line.units.end -= 1;
				}
				if line.units.is_empty() {
					break;
				}
				line.hyphen = br.hyphen_width > 0.0;
				line.last = br.forced;
				let range = units[line.units.start].source.start
					..units[line.units.end - 1].source.end;
				clusters = self.line_clusters(
					&p,
					range,
					line.hyphen,
					size,
					sans,
					width,
				);
				natural = clusters.iter().map(|c| c.width).sum();
				let tail = if opts.greedy {
					linebreak::greedy(&units[end..], width)
				} else {
					linebreak::break_lines(&units[end..], width, justify)
				};
				lines = tail
					.lines
					.into_iter()
					.map(|mut l| {
						l.units.start += end;
						l.units.end += end;
						l
					})
					.collect();
			}
			let ascent =
				clusters.iter().map(|c| c.ascent).fold(size * 0.8, f32::max);
			let descent = clusters
				.iter()
				.map(|c| c.descent)
				.fold(size * 0.2, f32::max);
			let mut height = (size * self.shaper.appearance.line_height)
				.max(ascent + descent + size * 0.18);
			let baseline =
				y_cursor + (height - ascent - descent) * 0.5 + ascent;
			let mut flexibility = Vec::new();
			for (i, c) in clusters.iter().enumerate() {
				let text = p.text.get(c.range.clone()).unwrap_or("-");
				let value = if i + 1 == clusters.len() {
					0.0
				} else if text == " " {
					if natural <= width {
						c.width * 0.65
					} else {
						c.width * 0.3
					}
				} else if natural <= width
					&& text.chars().next().is_some_and(is_cjk)
				{
					size * 0.08
				} else {
					0.0
				};
				flexibility.push(value);
			}
			let total: f32 = flexibility.iter().sum();
			let ratio = if justify && !line.last && total > 0.0 {
				((width - natural) / total).clamp(-1.0, 3.0)
			} else {
				0.0
			};
			let actual = natural + ratio * total;
			let offset = match align {
				CellAlign::Left => 0.0,
				CellAlign::Center => ((width - actual) * 0.5).max(0.0),
				CellAlign::Right => (width - actual).max(0.0),
			};
			let start_draw = out.draws.len();
			let mut cursor = x + offset;
			let mut link: Option<(String, f32)> = None;
			for (c, flex) in clusters.into_iter().zip(flexibility) {
				let range = p.reading_range(c.range.clone());
				if let Some(image) = p.images.get(&c.range.start) {
					let rect = Rect {
						x: cursor,
						y: baseline - c.ascent,
						w: c.width,
						h: c.ascent,
					};
					let command = out.draws.len();
					if !range.is_empty()
						&& self.image_placeholder(image).is_none()
					{
						out.text[node].push(TextCluster {
							range: range.clone(),
							rect,
							rtl: false,
							command,
						});
					}
					if let Some(url) = p
						.spans
						.iter()
						.find(|s| s.range.contains(&c.range.start))
						.and_then(|s| s.style.link.clone())
					{
						out.links.push(LinkRect { command, rect, url });
					}
					for mut cluster in
						self.draw_image(image, rect, size, width, out)
					{
						cluster.range.start += range.start;
						cluster.range.end += range.start;
						out.text[node].push(cluster);
					}
					cursor += c.width;
					continue;
				}
				if !range.is_empty() {
					out.text[node].push(TextCluster {
						range,
						rect: Rect {
							x: cursor,
							y: y_cursor,
							w: (c.width + flex * ratio).max(1.0),
							h: height,
						},
						rtl: c.rtl,
						command: out.draws.len(),
					});
				}

				let style = p
					.spans
					.iter()
					.find(|s| s.range.contains(&c.range.start))
					.map(|s| &s.style);
				let url = style.and_then(|s| s.link.as_deref());
				// A link wraps as one run per line, so hit testing stays tight.
				if link.as_ref().map(|(u, _)| u.as_str()) != url {
					if let Some((url, x0)) = link.take() {
						out.links.push(LinkRect {
							command: start_draw,
							rect: Rect {
								x: x0,
								y: baseline - ascent,
								w: cursor - x0,
								h: ascent + descent,
							},
							url,
						});
					}
					if let Some(url) = url {
						link = Some((url.to_string(), cursor));
					}
				}
				let appearance = style
					.map(|s| {
						self.shaper
							.stylesheet
							.inline(&self.shaper.appearance, s)
					})
					.unwrap_or_else(|| self.shaper.appearance.clone());
				if let Some(background) = appearance.background {
					out.draws.push(Draw::Rect(
						Rect {
							x: cursor,
							y: baseline - c.ascent - 1.0,
							w: c.width,
							h: c.ascent + c.descent + 2.0,
						},
						background,
					));
				}
				if let Some(math) = p.math.get(&c.range.start) {
					out.draws.push(Draw::Math {
						math: math.clone(),
						paint: appearance
							.paint
							.cascade(Role::Math, ColorField::Color),
						x: cursor,
						y: baseline - math.ascent,
					});
				} else {
					for mut g in c.glyphs {
						g.x += cursor;
						g.y += baseline;
						out.draws.push(Draw::Glyph(g));
					}
				}
				for decoration in &appearance.decoration {
					out.draws.push(Draw::Rect(
						Rect {
							x: cursor,
							y: if *decoration == Decoration::Strike {
								baseline - size * 0.3
							} else {
								baseline + size * 0.12
							},
							w: c.width,
							h: 1.0,
						},
						appearance.paint,
					));
				}
				cursor += c.width + flex * ratio;
			}
			if let Some((url, x0)) = link {
				out.links.push(LinkRect {
					command: start_draw,
					rect: Rect {
						x: x0,
						y: baseline - ascent,
						w: cursor - x0,
						h: ascent + descent,
					},
					url,
				});
			}
			if actual > width + 0.5 {
				let gutter = opts.stylesheet.scrollbar_gutter();
				out.overflow.push(Overflow {
					rect: Rect {
						x,
						y: y_cursor,
						w: width,
						h: height,
					},
					content_width: actual,
					commands: start_draw..out.draws.len(),
					gutter,
				});
				height += gutter;
			}
			out.width = out.width.max(x + actual.min(width));
			y_cursor += height;
		}
		if only_images && p.images.len() == 1 {
			let image = p.images.values().next().unwrap();
			let rule = opts.stylesheet.rule(Role::ImageCaption).clone();
			if let Some(caption) = rule.source.unwrap_or_default().text(image) {
				let old = self.shaper.appearance.clone();
				self.shaper.appearance =
					opts.stylesheet.text(&old, Role::ImageCaption);
				let caption_size = opts.font_size * self.shaper.appearance.size;
				y_cursor += rule.space_before.unwrap_or(0.) * opts.font_size;
				let mut decoration = BlockLayout::default();
				y_cursor += self.paragraph(
					&[Inline {
						kind: InlineKind::Text(caption.to_owned()),
						style: TextStyle::default(),
						source: 0..0,
					}],
					x,
					y_cursor,
					width,
					caption_size,
					false,
					rule.align.map(Into::into).unwrap_or(CellAlign::Center),
					false,
					opts,
					&mut decoration,
				);
				let offset = out.draws.len();
				for mut node in decoration.text {
					node.separator = "\n";
					for cluster in &mut node.clusters {
						cluster.command += offset;
					}
					out.text.push(node);
				}
				out.draws.extend(decoration.draws);
				out.overflow.extend(decoration.overflow.into_iter().map(
					|mut o| {
						o.commands.start += offset;
						o.commands.end += offset;
						o
					},
				));
				out.degraded += decoration.degraded;
				y_cursor += rule.space_after.unwrap_or(0.) * opts.font_size;
				self.shaper.appearance = old;
			} else {
				// Reserve the caption's ordinal so toggling it cannot renumber
				// subsequent paragraphs or table cells in this cached block.
				out.text.push(TextNode::new(String::new(), "\n"));
			}
		}
		y_cursor - y
	}
}
