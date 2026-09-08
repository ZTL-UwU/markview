//! Document layout and immutable snapshots, independent of a window or GPU.
use crate::{
	document::{
		Block, BlockKind, CellAlign, Document, Inline, InlineKind, RichText,
		TextStyle,
	},
	linebreak::{self, Break, Unit},
	math::{MathBox, MathEngine},
};
use parley::{
	FontContext, FontData, FontStyle, FontWeight, LayoutContext, StyleProperty,
};
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	ops::Range,
	sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Paint {
	#[default]
	Text,
	Muted,
	Accent,
	Panel,
	Border,
	Background,
	Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
	#[default]
	Light,
	Dark,
}
impl Theme {
	pub fn color(self, p: Paint) -> [f32; 4] {
		let rgb = match (self, p) {
			(Self::Light, Paint::Text) => 0x262b30,
			(Self::Light, Paint::Muted) => 0x69747e,
			(Self::Light, Paint::Accent) => 0x315d86,
			(Self::Light, Paint::Panel) => 0xeff1f3,
			(Self::Light, Paint::Border) => 0xd8dee3,
			(Self::Light, Paint::Background) => 0xfafaf8,
			(Self::Light, Paint::Error) => 0xa13f3f,
			(Self::Dark, Paint::Text) => 0xdde2e7,
			(Self::Dark, Paint::Muted) => 0x98a5b1,
			(Self::Dark, Paint::Accent) => 0x8fb9dd,
			(Self::Dark, Paint::Panel) => 0x2a3139,
			(Self::Dark, Paint::Border) => 0x414c58,
			(Self::Dark, Paint::Background) => 0x20252b,
			(Self::Dark, Paint::Error) => 0xf39a9a,
		};
		[
			((rgb >> 16) & 255) as f32 / 255.0,
			((rgb >> 8) & 255) as f32 / 255.0,
			(rgb & 255) as f32 / 255.0,
			1.0,
		]
	}
}

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

#[derive(Clone, Debug)]
pub struct Glyph {
	pub font: FontData,
	pub coords: Arc<[i16]>,
	pub id: u16,
	pub size: f32,
	pub x: f32,
	pub y: f32,
	pub paint: Paint,
	/// Byte range in the paragraph's visible text, for future hit testing.
	pub text_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
	pub x: f32,
	pub y: f32,
	pub w: f32,
	pub h: f32,
}
impl Rect {
	pub fn contains(self, x: f32, y: f32) -> bool {
		x >= self.x
			&& x <= self.x + self.w
			&& y >= self.y
			&& y <= self.y + self.h
	}
}

#[derive(Clone, Debug)]
pub enum Draw {
	Glyph(Glyph),
	Rect(Rect, Paint),
	Math { math: Arc<MathBox>, x: f32, y: f32 },
}
impl Draw {
	pub fn translate(&mut self, x: f32, y: f32) {
		match self {
			Self::Glyph(g) => {
				g.x += x;
				g.y += y;
			}
			Self::Rect(r, _) => {
				r.x += x;
				r.y += y;
			}
			Self::Math { x: gx, y: gy, .. } => {
				*gx += x;
				*gy += y;
			}
		}
	}
}

#[derive(Clone, Debug)]
pub struct Overflow {
	pub rect: Rect,
	pub content_width: f32,
	pub commands: Range<usize>,
}

#[derive(Debug, Default)]
pub struct BlockLayout {
	pub draws: Vec<Draw>,
	pub height: f32,
	pub width: f32,
	pub overflow: Vec<Overflow>,
	pub degraded: usize,
	pub math_errors: usize,
}

#[derive(Clone, Debug)]
pub struct PlacedBlock {
	pub id: u64,
	pub source: Range<usize>,
	pub y: f32,
	pub layout: Arc<BlockLayout>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutSnapshot {
	pub blocks: Vec<PlacedBlock>,
	pub height: f32,
	pub width: f32,
	pub reused: usize,
	pub degraded: usize,
	pub math_errors: usize,
}

#[derive(Clone)]
struct Span {
	range: Range<usize>,
	style: TextStyle,
}
#[derive(Clone)]
struct Cluster {
	range: Range<usize>,
	width: f32,
	ascent: f32,
	descent: f32,
	glyphs: Vec<Glyph>,
	continuation: bool,
}
struct Prepared {
	text: String,
	spans: Vec<Span>,
	math: BTreeMap<usize, Arc<MathBox>>,
}

pub struct LayoutEngine {
	fonts: FontContext,
	context: LayoutContext<usize>,
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
			fonts: FontContext::new(),
			context: LayoutContext::new(),
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

	fn shape(
		&mut self,
		text: &str,
		spans: &[Span],
		size: f32,
		sans: bool,
	) -> Vec<Cluster> {
		if text.is_empty() {
			return Vec::new();
		}
		let mut builder =
			self.context
				.ranged_builder(&mut self.fonts, text, 1.0, false);
		builder.push_default(StyleProperty::FontSize(size));
		builder.push_default(StyleProperty::FontFamily(
			if sans {
				"sans-serif"
			} else {
				"Noto Serif, Noto Serif CJK SC, serif"
			}
			.into(),
		));
		builder.push_default(StyleProperty::Brush(0));
		for (i, span) in spans.iter().enumerate() {
			let r = span.range.clone();
			builder.push(StyleProperty::Brush(i), r.clone());
			if span.style.bold {
				builder.push(
					StyleProperty::FontWeight(FontWeight::BOLD),
					r.clone(),
				);
			}
			if span.style.italic {
				builder.push(
					StyleProperty::FontStyle(FontStyle::Italic),
					r.clone(),
				);
			}
			if span.style.code {
				builder.push(
					StyleProperty::FontFamily("monospace".into()),
					r.clone(),
				);
				builder.push(StyleProperty::FontSize(size * 0.9), r.clone());
			}
			if span.style.superscript {
				builder.push(StyleProperty::FontSize(size * 0.7), r);
			}
		}
		let mut layout = builder.build(text);
		layout.break_all_lines(None);
		let mut clusters = Vec::new();
		for line in layout.lines() {
			for run in line.runs() {
				let coords: Arc<[i16]> = run.normalized_coords().into();
				for c in run.visual_clusters() {
					let mut x = 0.0;
					let mut glyphs = Vec::new();
					for g in c.glyphs() {
						let index = layout.styles()[g.style_index()].brush;
						let style = spans.get(index).map(|s| &s.style);
						let rise = if style.is_some_and(|s| s.superscript) {
							size * 0.35
						} else {
							0.0
						};
						glyphs.push(Glyph {
							font: run.font().clone(),
							coords: coords.clone(),
							id: g.id as u16,
							size: run.font_size(),
							x: x + g.x,
							y: g.y - rise,
							paint: if style.is_some_and(|s| s.link.is_some()) {
								Paint::Accent
							} else {
								Paint::Text
							},
							text_range: c.text_range(),
						});
						x += g.advance;
					}
					clusters.push(Cluster {
						range: c.text_range(),
						width: c.advance(),
						ascent: run.metrics().ascent,
						descent: run.metrics().descent,
						glyphs,
						continuation: c.is_ligature_continuation(),
					});
				}
			}
		}
		clusters
	}

	pub fn label(
		&mut self,
		text: &str,
		size: f32,
		x: f32,
		baseline: f32,
		paint: Paint,
	) -> Vec<Draw> {
		let clusters = self.shape(text, &[], size, true);
		let mut draws = Vec::new();
		let mut cursor = x;
		for c in clusters {
			for mut g in c.glyphs {
				g.x += cursor;
				g.y += baseline;
				g.paint = paint;
				draws.push(Draw::Glyph(g));
			}
			cursor += c.width;
		}
		draws
	}

	fn prepare(
		&mut self,
		rich: &[Inline],
		size: f32,
		out: &mut BlockLayout,
	) -> Prepared {
		let mut p = Prepared {
			text: String::new(),
			spans: Vec::new(),
			math: BTreeMap::new(),
		};
		for inline in rich {
			let start = p.text.len();
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
		let mut clusters = self.shape(&p.text, &p.spans, size, sans);
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
		let mut clusters = self.shape(&text, &spans, size, sans);
		for c in &mut clusters {
			c.range = c.range.start + range.start..c.range.end + range.start;
			for g in &mut c.glyphs {
				g.text_range = c.range.clone();
			}
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
			for (c, flex) in clusters.into_iter().zip(flexibility) {
				let style = p
					.spans
					.iter()
					.find(|s| s.range.contains(&c.range.start))
					.map(|s| &s.style);
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
				let start = out.draws.len();
				out.draws.push(Draw::Rect(Rect::default(), Paint::Panel));
				let mut cursor = y + 12.0;
				if !language.is_empty() {
					out.draws.extend(self.label(
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
					let line = expand_tabs(line, 4);
					let spans = [Span {
						range: 0..line.len(),
						style: TextStyle {
							code: true,
							..Default::default()
						},
					}];
					let clusters = self.shape(&line, &spans, size, false);
					let mut left = x + 14.0;
					for c in clusters {
						for mut g in c.glyphs {
							g.x += left;
							g.y += cursor + size;
							out.draws.push(Draw::Glyph(g));
						}
						left += c.width;
					}
					natural = natural.max(left - x + 14.0);
					cursor += size * 1.45;
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
					out.draws.extend(self.label(
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
						out.draws.extend(self.label(
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
				}
				top - y
			}
			BlockKind::Table { align, rows } => {
				self.table(align, rows, x, y, width, opts, out)
			}
			BlockKind::Footnote { label, blocks } => {
				out.draws.extend(self.label(
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

fn expand_tabs(line: &str, size: usize) -> String {
	let mut out = String::new();
	let mut col = 0;
	for c in line.chars() {
		if c == '\t' {
			let n = size - col % size;
			out.extend(std::iter::repeat_n(' ', n));
			col += n;
		} else {
			out.push(c);
			col += 1;
		}
	}
	out
}

/// Preserve a block-relative reading location; repeated blocks use occurrence order.
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::document;
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
			include_str!("../tests/fixtures/ordinary-10k.md"),
			include_str!("../tests/fixtures/math-10k.md"),
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
