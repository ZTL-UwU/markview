//! Document layout and immutable snapshots, independent of a window or GPU.
use crate::text::{TextCluster, TextNode};
use crate::{
	document::{
		Block, BlockKind, CellAlign, Document, Inline, InlineKind, RichText,
		TextStyle,
	},
	linebreak::{self, Break, Unit},
	math::{MathBox, MathEngine},
};
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	ops::Range,
	sync::Arc,
};

pub use crate::scene::*;
pub use crate::shaping::TextShaper;
use crate::shaping::{Cluster, Span};
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutOptions {
	pub width: f32,
	pub font_size: f32,
	pub justify: bool,
	pub hyphenate: bool,
	pub greedy: bool,
}
impl Default for LayoutOptions {
	fn default() -> Self {
		Self {
			width: 760.0,
			font_size: 18.0,
			justify: true,
			hyphenate: true,
			greedy: false,
		}
	}
}

struct Prepared {
	reading: String,
	mapping: Vec<(Range<usize>, Range<usize>, bool)>,
	text: String,
	spans: Vec<Span>,
	math: BTreeMap<usize, Arc<MathBox>>,
}

pub struct LayoutEngine {
	shaper: TextShaper,
	math: MathEngine,
	cache: HashMap<CacheKey, Arc<BlockLayout>>,
}

#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
	content: u64,
	width: u32,
	size: u32,
	justify: bool,
	hyphenate: bool,
	greedy: bool,
}
impl Default for LayoutEngine {
	fn default() -> Self {
		Self::new()
	}
}
impl LayoutEngine {
	pub fn new() -> Self {
		Self {
			shaper: TextShaper::new(),
			math: MathEngine::default(),
			cache: HashMap::new(),
		}
	}
	pub fn clear_document_cache(&mut self) {
		self.cache.clear();
	}

	pub fn layout(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
	) -> LayoutSnapshot {
		let mut result = LayoutSnapshot {
			width: options.width,
			..Default::default()
		};
		let previous = std::mem::take(&mut self.cache);
		let mut cached_draws = 0;
		for block in &document.blocks {
			let key = CacheKey {
				content: block.content_key,
				width: options.width.to_bits(),
				size: options.font_size.to_bits(),
				justify: options.justify,
				hyphenate: options.hyphenate,
				greedy: options.greedy,
			};
			let layout = if let Some(cached) = previous.get(&key) {
				result.reused += 1;
				cached.clone()
			} else {
				let mut out = BlockLayout::default();
				self.block(block, 0.0, 0.0, options.width, options, &mut out);
				Arc::new(out)
			};
			result.blocks.push(PlacedBlock {
				id: block.id,
				source: block.source.clone(),
				y: result.height,
				layout: layout.clone(),
			});
			result.height += layout.height + options.font_size * 0.8;
			result.degraded += layout.degraded;
			result.math_errors += layout.math_errors;
			cached_draws += layout.draws.len();
			if cached_draws < 100_000 && self.cache.len() < 256 {
				self.cache.insert(key, layout);
			}
		}
		result
	}

	pub fn label(
		&mut self,
		text: &str,
		size: f32,
		x: f32,
		y: f32,
		paint: Paint,
	) -> Vec<Draw> {
		self.shaper.label(text, size, x, y, paint)
	}
	pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
		self.shaper.fit(text, size, max)
	}
	pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
		self.shaper.text_width(text, size)
	}
	fn prepare(
		&mut self,
		rich: &[Inline],
		size: f32,
		out: &mut BlockLayout,
	) -> Prepared {
		let mut p = Prepared {
			reading: String::new(),
			mapping: Vec::new(),
			text: String::new(),
			spans: Vec::new(),
			math: BTreeMap::new(),
		};
		for inline in rich {
			let start = p.text.len();
			let reading_start = p.reading.len();
			match &inline.kind {
				InlineKind::Text(t) => p.reading.push_str(t),
				InlineKind::Math { latex, .. } => p.reading.push_str(latex),
			}
			let mut style = inline.style.clone();
			match &inline.kind {
				InlineKind::Text(t) => p.text.push_str(t),
				InlineKind::Math { latex, display } => {
					match self.math.layout(latex, *display, size) {
						Ok(m) => {
							p.math.insert(start, m);
							p.text.push('\u{fffc}');
						}
						Err(_) => {
							p.text.push_str(latex);
							style.code = true;
							out.math_errors += 1;
						}
					}
				}
			}
			p.mapping.push((
				start..p.text.len(),
				reading_start..p.reading.len(),
				matches!(inline.kind, InlineKind::Math { .. }),
			));
			p.spans.push(Span {
				range: start..p.text.len(),
				style,
			});
		}
		p
	}

	fn units(
		&mut self,
		p: &Prepared,
		size: f32,
		sans: bool,
		hyphenate: bool,
	) -> Vec<Unit> {
		let mut clusters = self.shaper.shape(&p.text, &p.spans, size, sans);
		clusters.sort_by_key(|c| c.range.start);
		let segmenter =
			icu_segmenter::LineSegmenter::new_auto(Default::default());
		let breaks: HashSet<usize> = segmenter.segment_str(&p.text).collect();
		let mut hyphens = HashSet::new();
		if hyphenate {
			let mut word_start = None;
			for (i, c) in p
				.text
				.char_indices()
				.chain(std::iter::once((p.text.len(), ' ')))
			{
				if c.is_ascii_alphabetic() {
					word_start.get_or_insert(i);
				} else if let Some(start) = word_start.take() {
					let word = &p.text[start..i];
					if word.len() >= 6
						&& !p.spans.iter().any(|s| {
							s.range.contains(&start)
								&& (s.style.code || s.style.link.is_some())
						}) {
						let mut offset = start;
						for syllable in
							hypher::hyphenate(word, hypher::Lang::English)
						{
							offset += syllable.len();
							if offset - start >= 2 && i - offset >= 3 {
								hyphens.insert(offset);
							}
						}
					}
				}
			}
		}
		let hyphen_width: f32 = self
			.shaper
			.shape("-", &[], size, sans)
			.iter()
			.map(|c| c.width)
			.sum();
		let mut units = Vec::new();
		for (i, c) in clusters.iter().enumerate() {
			let t = &p.text[c.range.clone()];
			let whitespace = t
				.chars()
				.all(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r');
			let hard = t.contains('\n');
			let soft_hyphen = t == "\u{ad}";
			let math = p.math.get(&c.range.start);
			let cjk = t.chars().next().is_some_and(is_cjk);
			let next = clusters.get(i + 1);
			let legal = breaks.contains(&c.range.end)
				&& !next.is_some_and(|c| c.continuation);
			let after = if hard {
				Some(Break::FORCED)
			} else if soft_hyphen || hyphens.contains(&c.range.end) {
				Some(Break {
					penalty: 50.0,
					hyphen_width,
					forced: false,
				})
			} else if legal {
				Some(Break::NORMAL)
			} else {
				None
			};
			let width = if hard || soft_hyphen {
				0.0
			} else {
				math.map_or(c.width, |m| m.width)
			};
			units.push(Unit {
				source: c.range.clone(),
				width,
				stretch: if whitespace && !hard {
					width * 0.65
				} else if cjk {
					size * 0.08
				} else {
					0.0
				},
				shrink: if whitespace && !hard {
					width * 0.3
				} else {
					0.0
				},
				discard: whitespace,
				after,
			});
		}
		units
	}

	fn line_clusters(
		&mut self,
		p: &Prepared,
		range: Range<usize>,
		hyphen: bool,
		size: f32,
		sans: bool,
	) -> Vec<Cluster> {
		let mut text = p.text[range.clone()].to_string();
		if hyphen {
			text.push('-');
		}
		let mut spans: Vec<Span> = p
			.spans
			.iter()
			.filter_map(|s| {
				let start = s.range.start.max(range.start);
				let end = s.range.end.min(range.end);
				(start < end).then(|| Span {
					range: start - range.start..end - range.start,
					style: s.style.clone(),
				})
			})
			.collect();
		if hyphen && let Some(s) = spans.last_mut() {
			s.range.end = text.len();
		}
		let mut clusters = self.shaper.shape(&text, &spans, size, sans);
		for c in &mut clusters {
			c.range = (c.range.start + range.start).min(range.end)
				..(c.range.end + range.start).min(range.end);
			if let Some(m) = p.math.get(&c.range.start) {
				c.width = m.width;
				c.ascent = m.ascent;
				c.descent = m.descent;
				c.glyphs.clear();
			}
			if p.text.get(c.range.clone()) == Some("\u{ad}") {
				c.width = 0.0;
				c.glyphs.clear();
			}
		}
		clusters
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	fn paragraph(
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
			return size * 1.65;
		}
		let units = self.units(&p, size, sans, opts.hyphenate && !sans);
		let solution = if opts.greedy {
			linebreak::greedy(&units, width)
		} else {
			linebreak::break_lines(&units, width, justify)
		};
		out.degraded += usize::from(solution.degraded && !opts.greedy);
		let mut y_cursor = y;
		let mut lines: std::collections::VecDeque<_> = solution.lines.into();
		while let Some(mut line) = lines.pop_front() {
			if line.units.is_empty() {
				y_cursor += size * 1.65;
				continue;
			}
			let range = units[line.units.start].source.start
				..units[line.units.end - 1].source.end;
			let mut clusters =
				self.line_clusters(&p, range, line.hyphen, size, sans);
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
				clusters =
					self.line_clusters(&p, range, line.hyphen, size, sans);
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
			let height = (size * 1.65).max(ascent + descent + size * 0.18);
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
				if style.is_some_and(|s| s.code) {
					out.draws.push(Draw::Rect(
						Rect {
							x: cursor,
							y: baseline - c.ascent - 1.0,
							w: c.width,
							h: c.ascent + c.descent + 2.0,
						},
						Paint::Panel,
					));
				}
				if let Some(math) = p.math.get(&c.range.start) {
					out.draws.push(Draw::Math {
						math: math.clone(),
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
				if style.is_some_and(|s| s.strike) {
					out.draws.push(Draw::Rect(
						Rect {
							x: cursor,
							y: baseline - size * 0.3,
							w: c.width,
							h: 1.0,
						},
						Paint::Text,
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
				out.overflow.push(Overflow {
					rect: Rect {
						x,
						y: y_cursor,
						w: width,
						h: height,
					},
					content_width: actual,
					commands: start_draw..out.draws.len(),
				});
			}
			out.width = out.width.max(x + actual.min(width));
			y_cursor += height;
		}
		y_cursor - y
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Text style and block geometry are independent layout inputs"
	)]
	fn rich(
		&mut self,
		rich: &RichText,
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
		let first_node = out.text.len();
		let mut start = 0;
		let mut cursor = y;
		for (i, inline) in rich.iter().enumerate() {
			if let InlineKind::Math { display: true, .. } = inline.kind {
				if i > start {
					cursor += self.paragraph(
						&rich[start..i],
						x,
						cursor,
						width,
						size,
						sans,
						align,
						justify,
						opts,
						out,
					);
				}
				cursor += size * 0.5;
				cursor += self.paragraph(
					&rich[i..i + 1],
					x,
					cursor,
					width,
					size * 1.1,
					false,
					CellAlign::Center,
					false,
					opts,
					out,
				);
				cursor += size * 0.5;
				start = i + 1;
			}
		}
		if start < rich.len() {
			cursor += self.paragraph(
				&rich[start..],
				x,
				cursor,
				width,
				size,
				sans,
				align,
				justify,
				opts,
				out,
			);
		}
		if let Some(node) = out.text.get_mut(first_node) {
			node.separator = "\n\n";
		}
		cursor - y
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Recursive block geometry and spacing"
	)]
	fn children(
		&mut self,
		blocks: &[Block],
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		gap: f32,
		out: &mut BlockLayout,
	) -> f32 {
		let mut cursor = y;
		for (i, block) in blocks.iter().enumerate() {
			if i > 0 {
				cursor += gap;
			}
			cursor += self.block(block, x, cursor, width, opts, out);
		}
		cursor - y
	}

	fn block(
		&mut self,
		block: &Block,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size;
		let width = width.max(40.0);
		let height = match &block.kind {
			BlockKind::Paragraph(text) => self.rich(
				text,
				x,
				y,
				width,
				size,
				false,
				CellAlign::Left,
				opts.justify,
				opts,
				out,
			),
			BlockKind::Heading { level, text } => {
				let factor = [1.9, 1.5, 1.25, 1.1, 1.0, 0.95]
					[(*level as usize).saturating_sub(1).min(5)];
				let mut text = text.clone();
				for s in &mut text {
					s.style.bold = true;
				}
				let top = if *level == 1 { 3.0 } else { size * 0.6 };
				top + self.rich(
					&text,
					x,
					y + top,
					width,
					size * factor,
					true,
					CellAlign::Left,
					false,
					opts,
					out,
				)
			}
			BlockKind::Rule => {
				out.draws.push(Draw::Rect(
					Rect {
						x,
						y: y + size * 0.5,
						w: width,
						h: 1.0,
					},
					Paint::Border,
				));
				size
			}
			BlockKind::Code { language, text } => {
				let node = out.text.len();
				out.text.push(TextNode::new(text.clone(), "\n\n"));
				let mut line_offset = 0;
				let start = out.draws.len();
				out.draws.push(Draw::Rect(Rect::default(), Paint::Panel));
				let mut cursor = y + 12.0;
				if !language.is_empty() {
					out.draws.extend(self.shaper.label(
						language,
						size * 0.67,
						x + 14.0,
						cursor + size * 0.7,
						Paint::Muted,
					));
					cursor += size * 1.3;
				}
				let content_start = out.draws.len();
				let mut natural = 0.0_f32;
				for line in text.trim_end_matches('\n').split('\n') {
					let original = line;
					let (line, offsets) = expand_tabs_mapped(line, 4);
					let spans = [Span {
						range: 0..line.len(),
						style: TextStyle {
							code: true,
							..Default::default()
						},
					}];
					let clusters =
						self.shaper.shape(&line, &spans, size, false);
					let mut left = x + 14.0;
					for c in clusters {
						let start = offsets[c.range.start];
						let end = offsets[c.range.end];
						let end = if start == end {
							start
								+ original[start..]
									.chars()
									.next()
									.map_or(0, char::len_utf8)
						} else {
							end
						};
						out.text[node].push(TextCluster {
							range: line_offset + start..line_offset + end,
							rect: Rect {
								x: left,
								y: cursor,
								w: c.width.max(1.0),
								h: size * 1.45,
							},
							rtl: c.rtl,
							command: out.draws.len(),
						});
						for mut g in c.glyphs {
							g.x += left;
							g.y += cursor + size;
							out.draws.push(Draw::Glyph(g));
						}
						left += c.width;
					}
					natural = natural.max(left - x + 14.0);
					cursor += size * 1.45;
					line_offset += original.len() + 1;
				}
				let h = cursor - y + 12.0;
				out.draws[start] =
					Draw::Rect(Rect { x, y, w: width, h }, Paint::Panel);
				if natural > width {
					out.overflow.push(Overflow {
						rect: Rect {
							x: x + 10.0,
							y,
							w: width - 20.0,
							h,
						},
						content_width: natural - 20.0,
						commands: content_start..out.draws.len(),
					});
				}
				h
			}
			BlockKind::Quote { label, blocks } => {
				let index = out.draws.len();
				out.draws.push(Draw::Rect(Rect::default(), Paint::Accent));
				let mut top = y + 4.0;
				if let Some(label) = label {
					out.draws.extend(self.shaper.label(
						label,
						size * 0.8,
						x + 20.0,
						top + size,
						Paint::Accent,
					));
					top += size * 1.65;
				}
				let h = top - y
					+ self.children(
						blocks,
						x + 20.0,
						top,
						width - 24.0,
						opts,
						size * 0.6,
						out,
					) + 4.0;
				out.draws[index] =
					Draw::Rect(Rect { x, y, w: 3.0, h }, Paint::Accent);
				h
			}
			BlockKind::List {
				start,
				tight,
				items,
			} => {
				let mut top = y;
				for (i, item) in items.iter().enumerate() {
					if i > 0 {
						top += if *tight { 2.0 } else { size * 0.6 };
					}
					let marker = match item.checked {
						Some(true) => "[x] ".into(),
						Some(false) => "[ ] ".into(),
						None => start.map_or_else(
							|| "• ".into(),
							|n| format!("{}. ", n + i),
						),
					};
					let mut node = TextNode::new(marker.clone(), "\n");
					node.push(TextCluster {
						range: 0..marker.len(),
						rect: Rect {
							x,
							y: top,
							w: 25.0,
							h: size * 1.65,
						},
						rtl: false,
						command: out.draws.len(),
					});
					out.text.push(node);
					let first_child = out.text.len();
					let indent = if start.is_some_and(|n| n + i >= 100) {
						48.0
					} else {
						30.0
					};
					if let Some(checked) = item.checked {
						let r = Rect {
							x: x + 2.0,
							y: top + size * 0.5,
							w: size * 0.65,
							h: size * 0.65,
						};
						out.draws.push(Draw::Rect(r, Paint::Accent));
						if !checked {
							out.draws.push(Draw::Rect(
								Rect {
									x: r.x + 1.5,
									y: r.y + 1.5,
									w: r.w - 3.0,
									h: r.h - 3.0,
								},
								Paint::Background,
							));
						}
					} else {
						let marker = start.map_or_else(
							|| "•".to_string(),
							|n| format!("{}.", n + i),
						);
						out.draws.extend(self.shaper.label(
							&marker,
							size * 0.9,
							x + 2.0,
							top + size * 1.15,
							Paint::Muted,
						));
					}
					top += self
						.children(
							&item.blocks,
							x + indent,
							top,
							width - indent,
							opts,
							size * 0.6,
							out,
						)
						.max(size * 1.65);
					if let Some(node) = out.text.get_mut(first_child) {
						node.separator = "";
					}
				}
				top - y
			}
			BlockKind::Table { align, rows } => {
				self.table(align, rows, x, y, width, opts, out)
			}
			BlockKind::Footnote { label, blocks } => {
				out.draws.extend(self.shaper.label(
					&format!("[{label}]"),
					size * 0.75,
					x,
					y + size,
					Paint::Muted,
				));
				let smaller = LayoutOptions {
					font_size: size * 0.88,
					..opts.clone()
				};
				self.children(
					blocks,
					x + 36.0,
					y,
					width - 36.0,
					&smaller,
					size * 0.5,
					out,
				)
			}
		};
		out.height = out.height.max(y + height);
		out.width = out.width.max(x + width);
		height
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Table geometry and column alignment"
	)]
	fn table(
		&mut self,
		align: &[CellAlign],
		rows: &[Vec<RichText>],
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let n = align.len();
		if n == 0 {
			return 0.0;
		}
		let size = opts.font_size * 0.9;
		let mut minima = vec![48.0_f32; n];
		let mut preferred = vec![48.0_f32; n];
		for row in rows {
			for (col, cell) in row.iter().enumerate().take(n) {
				let p = self.prepare(cell, size, out);
				let units = self.units(&p, size, false, false);
				preferred[col] = preferred[col]
					.max(units.iter().map(|u| u.width).sum::<f32>() + 24.0);
				let mut segment = 0.0_f32;
				for u in &units {
					segment += u.width;
					if u.after.is_some() {
						minima[col] = minima[col].max(segment + 24.0);
						segment = 0.0;
					}
				}
				minima[col] = minima[col].max(segment + 24.0);
			}
		}
		let min: f32 = minima.iter().sum();
		let preferred_total: f32 = preferred.iter().sum();
		let total = width.max(min);
		let widths: Vec<f32> = (0..n)
			.map(|i| {
				minima[i]
					+ if preferred_total > min {
						(total - min) * (preferred[i] - minima[i]).max(0.0)
							/ (preferred_total - min)
					} else {
						(total - min) / n as f32
					}
			})
			.collect();
		let start = out.draws.len();
		let mut top = y;
		let overflow_start = out.overflow.len();
		for (row_index, row) in rows.iter().enumerate() {
			let background = out.draws.len();
			out.draws.push(Draw::Rect(Rect::default(), Paint::Panel));
			let mut left = x;
			let mut row_height = size * 2.0;
			for col in 0..n {
				if let Some(cell) = row.get(col) {
					let mut cell = cell.clone();
					if row_index == 0 {
						for i in &mut cell {
							i.style.bold = true;
						}
					}
					let first_node = out.text.len();
					let h = self.rich(
						&cell,
						left + 12.0,
						top + 8.0,
						widths[col] - 24.0,
						size,
						false,
						align[col],
						false,
						opts,
						out,
					);
					if let Some(node) = out.text.get_mut(first_node) {
						node.separator = if col > 0 {
							"\t"
						} else if row_index > 0 {
							"\n"
						} else {
							"\n\n"
						};
					}
					row_height = row_height.max(h + 16.0);
				}
				left += widths[col];
			}
			out.draws[background] = Draw::Rect(
				Rect {
					x,
					y: top,
					w: total,
					h: row_height,
				},
				if row_index % 2 == 0 {
					Paint::Panel
				} else {
					Paint::Background
				},
			);
			out.draws.push(Draw::Rect(
				Rect {
					x,
					y: top + row_height - 1.0,
					w: total,
					h: 1.0,
				},
				Paint::Border,
			));
			top += row_height;
		}
		if total > width + 0.5 {
			out.overflow.truncate(overflow_start);
			out.overflow.push(Overflow {
				rect: Rect {
					x,
					y,
					w: width,
					h: top - y,
				},
				content_width: total,
				commands: start..out.draws.len(),
			});
		}
		top - y
	}
}

fn is_cjk(c: char) -> bool {
	matches!(c as u32, 0x2e80..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x3134f)
}

pub fn anchored_scroll(
	old: &LayoutSnapshot,
	new: &LayoutSnapshot,
	scroll: f32,
	viewport: f32,
	follow: bool,
) -> f32 {
	let max = (new.height - viewport).max(0.0);
	if follow && scroll >= (old.height - viewport - 3.0).max(0.0) {
		return max;
	}
	let index = old
		.blocks
		.partition_point(|b| b.y <= scroll)
		.saturating_sub(1);
	if let Some(anchor) = old.blocks.get(index) {
		let occurrence = old.blocks[..index]
			.iter()
			.filter(|b| b.id == anchor.id)
			.count();
		if let Some(b) = new
			.blocks
			.iter()
			.filter(|b| b.id == anchor.id)
			.nth(occurrence)
		{
			return (b.y + (scroll - anchor.y).min(b.layout.height))
				.clamp(0.0, max);
		}
		for delta in 1..=old.blocks.len() {
			for neighbor in [
				index.checked_sub(delta),
				index.checked_add(delta).filter(|&n| n < old.blocks.len()),
			]
			.into_iter()
			.flatten()
			{
				let a = &old.blocks[neighbor];
				if let Some(b) = new.blocks.iter().find(|b| b.id == a.id) {
					return (b.y + scroll - a.y).clamp(0.0, max);
				}
			}
		}
	}
	scroll.clamp(0.0, max)
}

impl Prepared {
	fn reading_range(&self, range: Range<usize>) -> Range<usize> {
		let Some((visual, logical, atomic)) = self
			.mapping
			.iter()
			.find(|(v, _, _)| v.contains(&range.start))
		else {
			return self.reading.len()..self.reading.len();
		};
		if *atomic {
			return logical.clone();
		}
		let start = logical.start + range.start - visual.start;
		let end = self
			.mapping
			.iter()
			.find(|(v, _, _)| v.start < range.end && v.end >= range.end)
			.map(|(v, l, atomic)| {
				if *atomic {
					l.end
				} else {
					l.start + range.end - v.start
				}
			})
			.unwrap_or(self.reading.len());
		start..end
	}
}
fn expand_tabs_mapped(text: &str, size: usize) -> (String, Vec<usize>) {
	let mut out = String::new();
	let mut offsets = vec![0];
	let mut column = 0;
	for (i, ch) in text.char_indices() {
		if ch == '\t' {
			let count = size - column % size;
			for n in 0..count {
				out.push(' ');
				offsets.push(if n + 1 == count { i + 1 } else { i });
			}
			column += count;
		} else {
			out.push(ch);
			for n in 1..=ch.len_utf8() {
				offsets.push(i + n);
			}
			column += 1;
		}
	}
	(out, offsets)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::document;
	#[test]
	fn links_are_hit_testable_and_survive_reuse() {
		let d = document::parse(
			"See [the manual](https://example.com/manual) and [mail](mailto:a@b.example).\n",
		);
		let mut engine = LayoutEngine::new();
		let opts = LayoutOptions {
			width: 400.0,
			..Default::default()
		};
		let snapshot = engine.layout(&d, &opts);
		let block = &snapshot.blocks[0];
		assert_eq!(block.layout.links.len(), 2);
		assert_eq!(block.layout.links[0].url, "https://example.com/manual");
		assert_eq!(block.layout.links[1].url, "mailto:a@b.example");
		let hit = block.layout.links[0].rect;
		let none = HashMap::new();
		assert_eq!(
			snapshot.link_at(hit.x + 1.0, block.y + hit.y + 1.0, &none),
			Some("https://example.com/manual")
		);
		assert_eq!(
			snapshot.link_at(hit.x - 6.0, block.y + hit.y + 1.0, &none),
			None
		);
		assert_eq!(snapshot.link_at(hit.x + 1.0, block.y - 1.0, &none), None);
		let again = engine.layout(&d, &opts);
		assert_eq!(again.reused, 1);
		assert_eq!(again.blocks[0].layout.links.len(), 2);
	}
	#[test]
	fn wrapped_links_produce_one_rect_per_line() {
		let d = document::parse(
			"[an intentionally long linked phrase that wraps](https://example.com)\n",
		);
		let mut engine = LayoutEngine::new();
		let opts = LayoutOptions {
			width: 120.0,
			..Default::default()
		};
		let snapshot = engine.layout(&d, &opts);
		let links = &snapshot.blocks[0].layout.links;
		assert!(links.len() > 1, "expected a wrapped link, got {links:?}");
		assert!(links.iter().all(|l| l.url == "https://example.com"));
		assert!(links.windows(2).all(|w| w[0].rect.y < w[1].rect.y));
	}
	#[test]
	fn long_labels_are_trimmed_to_fit() {
		let mut engine = LayoutEngine::new();
		let short = "https://example.com";
		assert_eq!(engine.fit(short, 11.0, 500.0), short);
		let long = "https://example.com/a/very/long/path/that/keeps/going?with=query&more=1";
		let fitted = engine.fit(long, 11.0, 160.0);
		assert!(engine.text_width(&fitted, 11.0) <= 160.0);
		let (head, tail) = fitted.split_once('…').expect("ellipsis");
		assert!(long.starts_with(head) && long.ends_with(tail));
		assert!(!head.is_empty() && !tail.is_empty());
		assert!(fitted.chars().count() < long.chars().count());
	}
	#[test]
	fn mixed_layout_is_finite_and_reused() {
		let mut engine = LayoutEngine::new();
		let d = document::parse(
			"中文标点（不应落在错误的位置），以及 **English typography** 与 $\\frac{x_1}{y}$ 混排。\n\nSecond paragraph.\n",
		);
		let opts = LayoutOptions {
			width: 280.0,
			..Default::default()
		};
		let a = engine.layout(&d, &opts);
		assert!(a.height.is_finite() && a.height > 50.0);
		assert_eq!(a.math_errors, 0);
		assert!(
			a.blocks[0]
				.layout
				.draws
				.iter()
				.any(|d| matches!(d, Draw::Math { .. }))
		);
		let b = engine.layout(&d, &opts);
		assert_eq!(b.reused, 2);
		let c = engine.layout(
			&d,
			&LayoutOptions {
				width: 400.0,
				..opts
			},
		);
		assert_eq!(c.reused, 0);
	}
	#[test]
	fn cjk_boundaries_and_hyphenation() {
		let mut e = LayoutEngine::new();
		let mut out = BlockLayout::default();
		let rich = vec![Inline {
			kind: InlineKind::Text("（中文），排版。 extraordinary".into()),
			style: TextStyle::default(),
			source: 0..0,
		}];
		let p = e.prepare(&rich, 18.0, &mut out);
		let units = e.units(&p, 18.0, false, true);
		for u in &units {
			if u.after.is_some() && u.source.end < p.text.len() {
				assert!(
					!"），。".contains(
						p.text[u.source.end..].chars().next().unwrap()
					)
				);
				assert_ne!(&p.text[u.source.clone()], "（");
			}
		}
		assert!(
			units
				.iter()
				.any(|u| u.after.is_some_and(|b| b.hyphen_width > 0.0))
		);
	}
	#[test]
	fn content_cache_survives_offsets_but_not_changed_references() {
		let mut e = LayoutEngine::new();
		let opts = LayoutOptions::default();
		let a = document::parse("A [link][id].\n\n[id]: https://one.example\n");
		e.layout(&a, &opts);
		let b = document::parse(
			"Inserted paragraph.\n\nA [link][id].\n\n[id]: https://one.example\n",
		);
		assert_eq!(e.layout(&b, &opts).reused, 1);
		let c = document::parse(
			"Inserted paragraph.\n\nA [link][id].\n\n[id]: https://two.example\n",
		);
		assert_eq!(e.layout(&c, &opts).reused, 1); // Only the inserted paragraph.
	}
	#[test]
	fn anchor_follows_content_and_only_follows_bottom_when_requested() {
		fn snapshot(ids: &[u64]) -> LayoutSnapshot {
			LayoutSnapshot {
				height: ids.len() as f32 * 120.0,
				blocks: ids
					.iter()
					.enumerate()
					.map(|(i, &id)| PlacedBlock {
						id,
						source: 0..0,
						y: i as f32 * 120.0,
						layout: Arc::new(BlockLayout {
							height: 120.0,
							..Default::default()
						}),
					})
					.collect(),
				..Default::default()
			}
		}
		let old = snapshot(&[1, 2, 3, 4, 5, 6]);
		let new = snapshot(&[0, 1, 2, 3, 4, 5, 6]);
		assert_eq!(anchored_scroll(&old, &new, 310.0, 200.0, true), 430.0);
		let appended = snapshot(&[1, 2, 3, 4, 5, 6, 7]);
		assert_eq!(anchored_scroll(&old, &appended, 520.0, 200.0, true), 640.0);
		assert_eq!(
			anchored_scroll(&old, &appended, 520.0, 200.0, false),
			520.0
		);
	}
	#[test]
	fn wide_blocks_are_scrollable_and_formulas_grow_line_height() {
		let mut e = LayoutEngine::new();
		let opts = LayoutOptions {
			width: 260.0,
			..Default::default()
		};
		let d = document::parse(
			"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n\n| Long column | Another column |\n|--|--|\n| aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb |\n",
		);
		let layout = e.layout(&d, &opts);
		assert!(layout.blocks.iter().all(|b| !b.layout.overflow.is_empty()));
		let text = e.layout(&document::parse("Plain text."), &opts);
		let math = e.layout(
			&document::parse(
				"Before $\\dfrac{\\dfrac{a}{b}}{\\dfrac{c}{d}}$ after.",
			),
			&opts,
		);
		assert!(math.height > text.height);
		assert_eq!(math.math_errors, 0);
	}
	#[test]
	fn benchmark_corpus_needs_no_emergency_greedy_fallback() {
		let mut e = LayoutEngine::new();
		for source in [
			include_str!("../../../tests/fixtures/ordinary-10k.md"),
			include_str!("../../../tests/fixtures/math-10k.md"),
		] {
			assert_eq!(source.len(), 10240);
			let d = document::parse(source);
			for width in [350.0, 760.0] {
				let s = e.layout(
					&d,
					&LayoutOptions {
						width,
						..Default::default()
					},
				);
				assert_eq!(s.math_errors, 0);
				assert_eq!(s.degraded, 0);
				assert!(s.blocks.iter().all(|b| b.layout.overflow.is_empty()));
			}
		}
	}
}
