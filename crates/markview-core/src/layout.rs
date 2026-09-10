//! Document layout and immutable snapshots, independent of a window or GPU.
use crate::style::{ColorField, Decoration, Role};
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
	sync::atomic::{AtomicUsize, Ordering},
	sync::{Arc, mpsc},
	thread,
};

pub use crate::scene::*;
pub use crate::shaping::TextShaper;
use crate::shaping::{Cluster, Span};

fn fitted_range(full: &str, shown: &str, range: Range<usize>) -> Range<usize> {
	let (prefix, full_end, shown_end) = crate::text::changed_span(full, shown);
	let start = if range.start <= prefix {
		range.start
	} else if range.start >= shown_end {
		range.start - shown_end + full_end
	} else {
		prefix
	};
	let end = if range.end <= prefix {
		range.end
	} else if range.end >= shown_end {
		range.end - shown_end + full_end
	} else {
		full_end
	};
	start..end
}

fn image_key(block: &Block, images: &crate::image::ImageSnapshot) -> u64 {
	let mut specs = Vec::new();
	block.images(&mut specs);
	crate::document::fingerprint(
		&specs
			.iter()
			.map(|s| {
				(
					&s.src,
					images
						.entries
						.get(&s.src)
						.map(|i| (i.version, i.size, &i.error)),
				)
			})
			.collect::<Vec<_>>(),
	)
}

#[derive(Clone, Debug)]
pub struct LayoutOptions {
	pub width: f32,
	pub font_size: f32,
	pub justify: bool,
	pub hyphenate: bool,
	pub greedy: bool,
	pub codeblock_theme_override: Option<String>,
	pub stylesheet: Arc<crate::style::Stylesheet>,
}
impl Default for LayoutOptions {
	fn default() -> Self {
		Self {
			width: 760.0,
			font_size: 18.0,
			justify: true,
			hyphenate: true,
			greedy: false,
			codeblock_theme_override: None,
			stylesheet: crate::style::Stylesheet::bundled(false),
		}
	}
}

impl PartialEq for LayoutOptions {
	fn eq(&self, other: &Self) -> bool {
		self.width == other.width
			&& self.font_size == other.font_size
			&& self.justify == other.justify
			&& self.hyphenate == other.hyphenate
			&& self.greedy == other.greedy
			&& self.codeblock_theme_override == other.codeblock_theme_override
			&& self.stylesheet.layout_key() == other.stylesheet.layout_key()
	}
}

struct Prepared {
	images: BTreeMap<usize, crate::image::ImageSpec>,
	reading: String,
	mapping: Vec<(Range<usize>, Range<usize>, bool)>,
	text: String,
	spans: Vec<Span>,
	math: BTreeMap<usize, Arc<MathBox>>,
}

pub struct LayoutEngine {
	images: crate::image::ImageSnapshot,
	shaper: TextShaper,
	math: MathEngine,
	cache: HashMap<CacheKey, Arc<BlockLayout>>,
	highlight_cache: HashMap<
		u64,
		Arc<Vec<Vec<(Range<usize>, Option<crate::style::Color>)>>>,
	>,
	highlight_tx: mpsc::Sender<(
		u64,
		Arc<Vec<Vec<(Range<usize>, Option<crate::style::Color>)>>>,
	)>,
	highlight_rx: mpsc::Receiver<(
		u64,
		Arc<Vec<Vec<(Range<usize>, Option<crate::style::Color>)>>>,
	)>,
	highlight_inflight: HashSet<u64>,
	highlight_generation: u64,
}

#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
	images: u64,
	content: u64,
	width: u32,
	size: u32,
	justify: bool,
	hyphenate: bool,
	greedy: bool,
	codeblock_theme_override: Option<String>,
	codeblock_theme: Option<String>,
	highlight_generation: u64,
	style: u64,
}
impl Default for LayoutEngine {
	fn default() -> Self {
		Self::new()
	}
}
impl LayoutEngine {
	pub fn new() -> Self {
		let (highlight_tx, highlight_rx) = mpsc::channel();
		Self {
			images: Default::default(),
			shaper: TextShaper::new(),
			math: MathEngine::default(),
			cache: HashMap::new(),
			highlight_cache: HashMap::new(),
			highlight_tx,
			highlight_rx,
			highlight_inflight: HashSet::new(),
			highlight_generation: 0,
		}
	}
	pub fn clear_document_cache(&mut self) {
		self.cache.clear();
	}
	pub fn validate_stylesheet(
		&mut self,
		stylesheet: &crate::style::Stylesheet,
	) -> anyhow::Result<()> {
		self.shaper.validate_stylesheet(stylesheet)
	}

	pub fn layout(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
	) -> LayoutSnapshot {
		self.layout_with_images(document, options, &Default::default())
	}

	pub fn layout_with_images(
		&mut self,
		document: &Document,
		options: &LayoutOptions,
		images: &crate::image::ImageSnapshot,
	) -> LayoutSnapshot {
		self.images = images.clone();
		self.shaper.set_stylesheet(options.stylesheet.clone());
		self.poll_highlights();
		let mut result = LayoutSnapshot {
			images: images.clone(),
			width: options.width,
			..Default::default()
		};
		let body = options.stylesheet.rule(Role::Body);
		let padding = body
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * options.font_size))
			.unwrap_or([0.; 4]);
		let content_width = (options.width - padding[1] - padding[3]).max(1.);
		self.prepare_highlights(&document.blocks, options);
		let codeblock_theme = options
			.codeblock_theme_override
			.clone()
			.or_else(|| options.stylesheet.rule(Role::CodeBlock).theme.clone());
		result.height =
			padding[0] + body.space_before.unwrap_or(0.) * options.font_size;
		let previous = std::mem::take(&mut self.cache);
		let mut cached_draws = 0;
		for block in &document.blocks {
			let key = CacheKey {
				images: image_key(block, images),
				content: block.content_key,
				width: options.width.to_bits(),
				size: options.font_size.to_bits(),
				justify: options.justify,
				hyphenate: options.hyphenate,
				greedy: options.greedy,
				codeblock_theme_override: options
					.codeblock_theme_override
					.clone(),
				codeblock_theme: codeblock_theme.clone(),
				highlight_generation: self.highlight_generation,
				style: options.stylesheet.layout_key(),
			};
			let layout = if let Some(cached) = previous.get(&key) {
				result.reused += 1;
				cached.clone()
			} else {
				let mut out = BlockLayout::default();
				self.block(
					block,
					padding[3],
					0.0,
					content_width,
					options,
					&mut out,
				);
				Arc::new(out)
			};
			result.blocks.push(PlacedBlock {
				id: block.id,
				source: block.source.clone(),
				y: result.height,
				layout: layout.clone(),
			});
			result.height += layout.height;
			result.degraded += layout.degraded;
			result.math_errors += layout.math_errors;
			cached_draws += layout.draws.len();
			if cached_draws < 100_000 && self.cache.len() < 256 {
				self.cache.insert(key, layout);
			}
		}
		result.height +=
			padding[2] + body.space_after.unwrap_or(0.) * options.font_size;
		result.document_box = Some(Draw::Box {
			rect: Rect {
				x: 0.,
				y: 0.,
				w: options.width,
				h: result.height,
			},
			role: Role::Body,
			radius: body.radius.unwrap_or(0.),
			border: body.border_width.unwrap_or(0.),
			left_only: false,
		});
		result
	}

	fn prepare_highlights(
		&mut self,
		blocks: &[Block],
		options: &LayoutOptions,
	) {
		let theme = options
			.codeblock_theme_override
			.as_deref()
			.or(options.stylesheet.rule(Role::CodeBlock).theme.as_deref())
			.map(str::to_owned);
		let mut jobs = Vec::new();
		fn collect(
			blocks: &[Block],
			theme: Option<&str>,
			jobs: &mut Vec<(u64, String, String, Option<String>)>,
		) {
			for block in blocks {
				match &block.kind {
					BlockKind::Code { language, text } => {
						let key = crate::document::fingerprint(&(
							language, text, theme,
						));
						jobs.push((
							key,
							language.clone(),
							text.clone(),
							theme.map(str::to_owned),
						));
					}
					BlockKind::Quote { blocks, .. } => {
						collect(blocks, theme, jobs)
					}
					BlockKind::List { items, .. } => {
						for item in items {
							collect(&item.blocks, theme, jobs);
						}
					}
					BlockKind::Footnote { blocks, .. } => {
						collect(blocks, theme, jobs)
					}
					_ => {}
				}
			}
		}
		collect(blocks, theme.as_deref(), &mut jobs);
		jobs.retain(|(key, ..)| {
			!self.highlight_cache.contains_key(key)
				&& !self.highlight_inflight.contains(key)
		});
		if jobs.is_empty() {
			return;
		}
		for (key, ..) in &jobs {
			self.highlight_inflight.insert(*key);
		}
		let worker_count = thread::available_parallelism()
			.map_or(1, std::num::NonZeroUsize::get)
			.min(jobs.len())
			.min(if jobs.len() < 4 { 1 } else { 4 });
		let tx = self.highlight_tx.clone();
		thread::spawn(move || {
			let next = AtomicUsize::new(0);
			thread::scope(|scope| {
				for _ in 0..worker_count {
					let next = &next;
					let jobs = &jobs;
					let tx = tx.clone();
					scope.spawn(move || {
						loop {
							let index = next.fetch_add(1, Ordering::Relaxed);
							let Some((key, language, text, theme)) =
								jobs.get(index)
							else {
								break;
							};
							let lines = text
								.trim_end_matches('\n')
								.split('\n')
								.map(|line| {
									expand_tabs_mapped(line, 4).0.to_owned()
								});
							let highlighted = crate::highlight::highlight_block(
								language,
								theme.as_deref(),
								lines,
							);
							let _ = tx.send((*key, Arc::new(highlighted)));
						}
					});
				}
			});
		});
	}

	pub fn poll_highlights(&mut self) -> bool {
		let mut changed = false;
		while let Ok((key, highlighted)) = self.highlight_rx.try_recv() {
			self.highlight_inflight.remove(&key);
			if self.highlight_cache.len() >= 256 {
				self.highlight_cache.clear();
			}
			self.highlight_cache.insert(key, highlighted);
			changed = true;
		}
		if changed {
			self.highlight_generation =
				self.highlight_generation.wrapping_add(1);
			self.cache.clear();
		}
		changed
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
			images: BTreeMap::new(),
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
				InlineKind::Image(image) => p.reading.push_str(
					&self
						.image_placeholder(image)
						.unwrap_or_else(|| image.alt.clone()),
				),
				InlineKind::Text(t) => p.reading.push_str(t),
				InlineKind::Math { latex, .. } => p.reading.push_str(latex),
			}
			let mut style = inline.style.clone();
			match &inline.kind {
				InlineKind::Image(image) => {
					p.images.insert(start, image.clone());
					p.text.push('\u{fffc}');
				}
				InlineKind::Text(t) => p.text.push_str(t),
				InlineKind::Math { latex, display } => {
					match self.math.layout(
						latex,
						*display,
						size * self
							.shaper
							.stylesheet
							.rule(Role::Math)
							.size
							.unwrap_or(1.),
					) {
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
				matches!(
					inline.kind,
					InlineKind::Math { .. } | InlineKind::Image(_)
				),
			));
			p.spans.push(Span {
				range: start..p.text.len(),
				style,
			});
		}
		p
	}

	/// The image box scaled into the paragraph measure, like `max-width: 100%`.
	fn image_size(
		&self,
		image: &crate::image::ImageSpec,
		available: f32,
		size: f32,
	) -> (f32, f32) {
		let inset = self.image_insets(size, available);
		let (w, h) = image.size(
			self.images.entries.get(&image.src),
			(available - inset[1] - inset[3]).max(1.),
		);
		(w + inset[1] + inset[3], h + inset[0] + inset[2])
	}

	fn image_placeholder(
		&self,
		image: &crate::image::ImageSpec,
	) -> Option<String> {
		let message = match self.images.entries.get(&image.src) {
			Some(i) if i.size.is_some() && i.error.is_none() => return None,
			Some(i) if i.error.is_some() => i.error.as_deref().unwrap(),
			_ => "Loading image…",
		};
		Some(if image.alt.is_empty() {
			message.to_owned()
		} else {
			format!("{} · {message}", image.alt)
		})
	}

	fn image_insets(&self, size: f32, available: f32) -> [f32; 4] {
		let rule = self.shaper.stylesheet.rule(Role::Image);
		let base = size / self.shaper.appearance.size;
		let border = rule.border_width.unwrap_or(0.);
		let mut inset = rule
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * base + border))
			.unwrap_or([border; 4]);
		let scale =
			((available - 1.).max(0.) / (inset[1] + inset[3]).max(1.)).min(1.);
		inset[1] *= scale;
		inset[3] *= scale;
		inset
	}

	fn units(
		&mut self,
		p: &Prepared,
		size: f32,
		sans: bool,
		hyphenate: bool,
		available: f32,
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
			} else if let Some(image) = p.images.get(&c.range.start) {
				self.image_size(image, available, size).0
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
		available: f32,
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
			if let Some(image) = p.images.get(&c.range.start) {
				let (w, h) = self.image_size(image, available, size);
				c.width = w;
				c.ascent = h;
				c.descent = 0.;
				c.glyphs.clear();
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

	fn draw_image(
		&mut self,
		image: &crate::image::ImageSpec,
		rect: Rect,
		size: f32,
		available: f32,
		out: &mut BlockLayout,
	) -> Vec<TextCluster> {
		let mut text_clusters = Vec::new();
		let info = self.images.entries.get(&image.src);
		let inset = self.image_insets(size, available);
		let content = Rect {
			x: rect.x + inset[3],
			y: rect.y + inset[0],
			w: (rect.w - inset[1] - inset[3]).max(1.),
			h: (rect.h - inset[0] - inset[2]).max(1.),
		};
		out.draws.push(Draw::Image {
			src: image.src.clone(),
			version: info.map_or(0, |i| i.version),
			rect: content,
			title: image.title.clone(),
		});
		out.draws.push(Draw::Box {
			rect,
			role: Role::Image,
			radius: 0.,
			border: self
				.shaper
				.stylesheet
				.rule(Role::Image)
				.border_width
				.unwrap_or(0.)
				.min(inset[1])
				.min(inset[3]),
			left_only: false,
		});
		if let Some(text) = self.image_placeholder(image) {
			let rect = content;
			out.draws.push(Draw::Rect(
				rect,
				Paint::Styled(Role::ImagePlaceholder, ColorField::Background),
			));
			let old = self.shaper.appearance.clone();
			let base = size / old.size;
			self.shaper.appearance =
				self.shaper.stylesheet.text(&old, Role::ImagePlaceholder);
			let label_size = base * self.shaper.appearance.size;
			let label = self.shaper.fit(&text, base, (rect.w - 12.).max(0.));
			if rect.h >= label_size + 12. && rect.w > 12. {
				let baseline = rect.y + 6. + label_size;
				let mut cursor = rect.x + 6.;
				let command = out.draws.len();
				for c in self.shaper.shape(&label, &[], label_size, true) {
					text_clusters.push(TextCluster {
						range: fitted_range(&text, &label, c.range),
						rect: Rect {
							x: cursor,
							y: baseline - c.ascent,
							w: c.width.max(1.),
							h: c.ascent + c.descent,
						},
						rtl: c.rtl,
						command,
					});
					cursor += c.width;
				}
				out.draws.extend(self.shaper.label(
					&label,
					base,
					rect.x + 6.,
					rect.y + 6. + label_size,
					Paint::Styled(Role::ImagePlaceholder, ColorField::Color),
				));
			}
			self.shaper.appearance = old;
		}
		out.width = out.width.max(rect.x + rect.w);
		text_clusters
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
					size,
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
		_gap: f32,
		out: &mut BlockLayout,
	) -> f32 {
		let mut cursor = y;
		for block in blocks {
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
		let role = match &block.kind {
			BlockKind::Paragraph(_) => Role::P,
			BlockKind::Heading { level, .. } => Role::heading(*level),
			BlockKind::Code { .. } => Role::CodeBlock,
			BlockKind::Quote { .. } => Role::Blockquote,
			BlockKind::List { .. } => Role::List,
			BlockKind::Table { .. } => Role::Table,
			BlockKind::Footnote { .. } => Role::Footnote,
			BlockKind::Rule => Role::Hr,
		};
		let previous = self.shaper.appearance.clone();
		let rule = opts.stylesheet.rule(role).clone();
		self.shaper.appearance = opts.stylesheet.text(&previous, role);
		self.shaper.appearance.background = None;
		let before = rule.space_before.unwrap_or(0.) * opts.font_size;
		let after = rule.space_after.unwrap_or(0.) * opts.font_size;
		let pad = rule
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * opts.font_size))
			.unwrap_or([0.; 4]);
		let inner_y = y + before + pad[0];
		let placeholder = out.draws.len();
		out.draws.push(Draw::Box {
			rect: Rect::default(),
			role,
			radius: rule.radius.unwrap_or(0.),
			border: rule.border_width.unwrap_or(0.),
			left_only: role == Role::Blockquote,
		});
		let height = self.block_inner(
			block,
			x + pad[3],
			inner_y,
			(width - pad[1] - pad[3]).max(1.),
			opts,
			out,
		);
		let box_height = pad[0] + height + pad[2];
		out.draws[placeholder] = Draw::Box {
			rect: Rect {
				x,
				y: y + before,
				w: width,
				h: box_height,
			},
			role,
			radius: rule.radius.unwrap_or(0.),
			border: if role == Role::Hr || role == Role::Table {
				0.
			} else {
				rule.border_width.unwrap_or(0.)
			},
			left_only: role == Role::Blockquote,
		};
		self.shaper.appearance = previous;
		let total = before + box_height + after;
		out.height = out.height.max(y + total);
		out.width = out.width.max(x + width);
		total
	}
	fn block_inner(
		&mut self,
		block: &Block,
		x: f32,
		y: f32,
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let size = opts.font_size * self.shaper.appearance.size;
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
			BlockKind::Heading { text, .. } => self.rich(
				text,
				x,
				y,
				width,
				size,
				true,
				CellAlign::Left,
				false,
				opts,
				out,
			),
			BlockKind::Rule => {
				out.draws.push(Draw::Rect(
					Rect {
						x,
						y,
						w: width,
						h: opts
							.stylesheet
							.rule(Role::Hr)
							.border_width
							.unwrap_or(1.),
					},
					Paint::Styled(Role::Hr, ColorField::Color),
				));
				opts.stylesheet.rule(Role::Hr).border_width.unwrap_or(1.)
			}
			BlockKind::Code { language, text } => {
				let node = out.text.len();
				out.text.push(TextNode::new(text.clone(), "\n\n"));
				let mut line_offset = 0;

				let mut cursor = y;
				if !language.is_empty() {
					let rule = opts.stylesheet.rule(Role::CodeLabel);
					cursor += rule.space_before.unwrap_or(0.) * opts.font_size;
					let label = opts
						.stylesheet
						.text(&self.shaper.appearance, Role::CodeLabel);
					let label_size = opts.font_size * label.size;
					let label_height = label_size * label.line_height;
					out.draws.extend(self.shaper.label(
						language,
						opts.font_size,
						x,
						cursor + label_size,
						Paint::Styled(Role::CodeLabel, ColorField::Color),
					));
					cursor += label_height
						+ rule.space_after.unwrap_or(0.) * opts.font_size;
				}
				let content_start = out.draws.len();
				let mut natural = 0.0_f32;
				let theme = opts.codeblock_theme_override.as_deref().or(opts
					.stylesheet
					.rule(Role::CodeBlock)
					.theme
					.as_deref());
				let highlight_key =
					crate::document::fingerprint(&(language, text, theme));
				let highlighted = self
					.highlight_cache
					.get(&highlight_key)
					.cloned()
					.unwrap_or_else(|| {
						Arc::new(vec![
							Vec::new();
							text.trim_end_matches('\n')
								.split('\n')
								.count()
						])
					});
				for (line_index, line) in
					text.trim_end_matches('\n').split('\n').enumerate()
				{
					let original = line;
					let (line, offsets) = expand_tabs_mapped(line, 4);
					// Syntax colors are applied after shaping. Keeping the shaper input
					// plain means highlighting cannot affect font selection, shaping,
					// line breaking, or any geometry used by selection.
					let spans = [Span {
						range: 0..line.len(),
						style: TextStyle::default(),
					}];
					let clusters =
						self.shaper.shape(&line, &spans, size, false);
					let line_ascent = clusters
						.iter()
						.map(|c| c.ascent)
						.fold(size * 0.8, f32::max);
					let line_descent = clusters
						.iter()
						.map(|c| c.descent)
						.fold(size * 0.2, f32::max);
					let line_height = (size
						* self.shaper.appearance.line_height)
						.max(line_ascent + line_descent);
					let baseline = cursor
						+ (line_height - line_ascent - line_descent) * 0.5
						+ line_ascent;
					let mut left = x;
					for c in clusters {
						let color = highlighted[line_index]
							.iter()
							.find(|(range, _)| {
								range.start <= c.range.start
									&& c.range.start < range.end
							})
							.and_then(|(_, color)| *color);
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
								h: line_height,
							},
							rtl: c.rtl,
							command: out.draws.len(),
						});
						for mut g in c.glyphs {
							if let Some(color) = color {
								g.paint = Paint::Color(color);
							}
							g.x += left;
							g.y += baseline;
							out.draws.push(Draw::Glyph(g));
						}
						left += c.width;
					}
					natural = natural.max(left - x);
					cursor += line_height;
					line_offset += original.len() + 1;
				}
				let mut h = cursor - y;
				if natural > width {
					let gutter = opts.stylesheet.scrollbar_gutter();
					out.overflow.push(Overflow {
						rect: Rect { x, y, w: width, h },
						content_width: natural,
						commands: content_start..out.draws.len(),
						gutter,
					});
					h += gutter;
				}
				h
			}
			BlockKind::Quote { label, blocks } => {
				let mut top = y;
				if let Some(label) = label {
					out.draws.extend(self.shaper.label(
						label,
						size * 0.8,
						x,
						top + size,
						Paint::Styled(Role::Blockquote, ColorField::Color),
					));
					top += size * self.shaper.appearance.line_height;
				}
				top - y
					+ self.children(
						blocks,
						x,
						top,
						width,
						opts,
						size * 0.6,
						out,
					)
			}
			BlockKind::List {
				start,
				tight: _,
				items,
			} => {
				let mut top = y;
				let list_appearance = self.shaper.appearance.clone();
				let item_rule = opts.stylesheet.rule(Role::ListItem).clone();
				let padding = item_rule
					.padding
					.as_ref()
					.map(|p| p.sides().map(|v| v * opts.font_size))
					.unwrap_or([0.; 4]);
				for (i, item) in items.iter().enumerate() {
					self.shaper.appearance =
						opts.stylesheet.text(&list_appearance, Role::ListItem);
					top +=
						item_rule.space_before.unwrap_or(0.) * opts.font_size;
					let box_y = top;
					let box_index = out.draws.len();
					out.draws.push(Draw::Rect(Rect::default(), Paint::Text));
					top += padding[0];
					let item_x = x + padding[3];
					let item_width = (width - padding[1] - padding[3]).max(1.);

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
							h: size * self.shaper.appearance.line_height,
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
						let task = opts
							.stylesheet
							.text(&self.shaper.appearance, Role::TaskMarker);
						let marker_size = opts.font_size * task.size;
						let r = Rect {
							x: item_x + 2.,
							y: top + size * 0.5,
							w: marker_size * 0.7,
							h: marker_size * 0.7,
						};
						out.draws.push(Draw::Rect(
							r,
							Paint::Styled(
								Role::TaskMarker,
								ColorField::BorderColor,
							),
						));
						out.draws.push(Draw::Rect(
							Rect {
								x: r.x + 1.,
								y: r.y + 1.,
								w: (r.w - 2.).max(0.),
								h: (r.h - 2.).max(0.),
							},
							Paint::Styled(
								Role::TaskMarker,
								ColorField::Background,
							),
						));
						if checked {
							out.draws.extend(self.shaper.label(
								"✓",
								opts.font_size * 0.7,
								r.x,
								r.y + r.h,
								Paint::Styled(
									Role::TaskMarker,
									ColorField::Color,
								),
							));
						}
					} else {
						let marker = start.map_or_else(
							|| "•".to_string(),
							|n| format!("{}.", n + i),
						);
						out.draws.extend(self.shaper.label(
							&marker,
							opts.font_size,
							item_x + 2.0,
							top + size * 1.15,
							Paint::Styled(Role::ListMarker, ColorField::Color),
						));
					}
					top += self
						.children(
							&item.blocks,
							item_x + indent,
							top,
							(item_width - indent).max(1.),
							opts,
							size * 0.6,
							out,
						)
						.max(size * self.shaper.appearance.line_height);
					top += padding[2];
					out.draws[box_index] = Draw::Box {
						rect: Rect {
							x,
							y: box_y,
							w: width,
							h: top - box_y,
						},
						role: Role::ListItem,
						radius: item_rule.radius.unwrap_or(0.),
						border: item_rule.border_width.unwrap_or(0.),
						left_only: false,
					};
					top += item_rule.space_after.unwrap_or(0.) * opts.font_size;
					if let Some(node) = out.text.get_mut(first_child) {
						node.separator = "";
					}
				}
				self.shaper.appearance = list_appearance;
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
					Paint::Styled(Role::Footnote, ColorField::Color),
				));
				let smaller = LayoutOptions {
					font_size: opts.font_size,
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
			return 0.;
		}
		let table_appearance = self.shaper.appearance.clone();
		let table_rule = opts.stylesheet.rule(Role::Table).clone();
		let mut minima = vec![48_f32; n];
		let mut preferred = vec![48_f32; n];
		let cell_rule = |header: bool| {
			let mut rule = opts.stylesheet.rule(Role::TableCell).clone();
			if header {
				rule.overlay(opts.stylesheet.rule(Role::TableHeader));
			}
			rule
		};
		let cell_appearance = |header: bool| {
			let base = opts.stylesheet.text(&table_appearance, Role::TableCell);
			if header {
				opts.stylesheet.text(&base, Role::TableHeader)
			} else {
				base
			}
		};
		for (row_index, row) in rows.iter().enumerate() {
			self.shaper.appearance = cell_appearance(row_index == 0);
			let rule = cell_rule(row_index == 0);
			let pad = rule
				.padding
				.as_ref()
				.map(|p| p.sides().map(|v| v * opts.font_size))
				.unwrap_or([0.; 4]);
			let size = opts.font_size * self.shaper.appearance.size;
			for (col, cell) in row.iter().enumerate().take(n) {
				let p = self.prepare(cell, size, out);
				let units = self.units(&p, size, false, false, width);
				let inset = pad[1] + pad[3];
				preferred[col] = preferred[col]
					.max(units.iter().map(|u| u.width).sum::<f32>() + inset);
				let mut segment = 0_f32;
				for u in &units {
					segment += u.width;
					if u.after.is_some() {
						minima[col] = minima[col].max(segment + inset);
						segment = 0.;
					}
				}
				minima[col] = minima[col].max(segment + inset);
			}
		}
		let min: f32 = minima.iter().sum();
		let preferred_total: f32 = preferred.iter().sum();
		let total = width.max(min);
		let widths: Vec<f32> = (0..n)
			.map(|i| {
				minima[i]
					+ if preferred_total > min {
						(total - min) * (preferred[i] - minima[i]).max(0.)
							/ (preferred_total - min)
					} else {
						(total - min) / n as f32
					}
			})
			.collect();
		let start = out.draws.len();
		let overflow_start = out.overflow.len();
		let mut top = y;
		for (row_index, row) in rows.iter().enumerate() {
			let header = row_index == 0;
			let role = if header {
				Role::TableHeader
			} else {
				Role::TableCell
			};
			let rule = cell_rule(header);
			let pad = rule
				.padding
				.as_ref()
				.map(|p| p.sides().map(|v| v * opts.font_size))
				.unwrap_or([0.; 4]);
			let before = rule.space_before.unwrap_or(0.) * opts.font_size;
			let after = rule.space_after.unwrap_or(0.) * opts.font_size;
			self.shaper.appearance = cell_appearance(header);
			let size = opts.font_size * self.shaper.appearance.size;
			let mut left = x;
			let mut row_height = size * self.shaper.appearance.line_height
				+ pad[0] + pad[2]
				+ before + after;
			let mut boxes = Vec::new();
			for col in 0..n {
				let index = out.draws.len();
				out.draws.push(Draw::Rect(Rect::default(), Paint::Text));
				boxes.push((index, left, widths[col]));
				if let Some(cell) = row.get(col) {
					let first_node = out.text.len();
					let h = self.rich(
						cell,
						left + pad[3],
						top + before + pad[0],
						(widths[col] - pad[1] - pad[3]).max(1.),
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
					row_height =
						row_height.max(h + pad[0] + pad[2] + before + after);
				}
				left += widths[col];
			}
			for (index, left, w) in boxes {
				out.draws[index] = Draw::Box {
					rect: Rect {
						x: left,
						y: top + before,
						w,
						h: (row_height - before - after).max(0.),
					},
					role,
					radius: rule.radius.unwrap_or(0.),
					border: rule
						.border_width
						.or(table_rule.border_width)
						.unwrap_or(0.),
					left_only: false,
				};
			}
			top += row_height;
		}
		let mut height = top - y;
		if total > width + 0.5 {
			let gutter = opts.stylesheet.scrollbar_gutter();
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
				gutter,
			});
			height += gutter;
		}
		self.shaper.appearance = table_appearance;
		height
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
			if old.images.entries != new.images.entries {
				let local_y = scroll - anchor.y;
				let cluster = anchor
					.layout
					.text
					.iter()
					.enumerate()
					.flat_map(|(ni, n)| n.clusters.iter().map(move |c| (ni, c)))
					.filter(|(_, c)| c.rect.y + c.rect.h >= local_y)
					.min_by(|(_, a), (_, b)| {
						let a_image = matches!(
							anchor.layout.draws[a.command],
							Draw::Image { .. }
						);
						let b_image = matches!(
							anchor.layout.draws[b.command],
							Draw::Image { .. }
						);
						a_image.cmp(&b_image).then_with(|| {
							(a.rect.y - local_y)
								.abs()
								.total_cmp(&(b.rect.y - local_y).abs())
						})
					});
				if let Some((ni, c)) = cluster
					&& let Some(next) = b.layout.text.get(ni).and_then(|n| {
						n.clusters
							.iter()
							.find(|n| n.range.contains(&c.range.start))
					}) {
					return (b.y + next.rect.y + (local_y - c.rect.y))
						.clamp(0., max);
				}
			}
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
		let units = e.units(&p, 18.0, false, true, 760.0);
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
	fn overflowing_blocks_reserve_the_configured_scrollbar_gutter() {
		let mut e = LayoutEngine::new();
		let d = document::parse(
			"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n",
		);
		let opts = LayoutOptions {
			width: 260.0,
			..Default::default()
		};
		let base = e.layout(&d, &opts);
		let bundled =
			crate::style::Stylesheet::bundled(false).scrollbar_gutter();
		assert_eq!(base.blocks[0].layout.overflow[0].gutter, bundled);
		// A wider gutter both reserves more space and grows the block.
		let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
		sheet.merge(
			&crate::style::Stylesheet::parse(
				"format_version=1\nversion=1\n[scrollbar]\ngutter=30.0",
			)
			.unwrap(),
		);
		let taller = e.layout(
			&d,
			&LayoutOptions {
				width: 260.0,
				stylesheet: Arc::new(sheet),
				..Default::default()
			},
		);
		assert_eq!(taller.blocks[0].layout.overflow[0].gutter, 30.0);
		let delta =
			taller.blocks[0].layout.height - base.blocks[0].layout.height;
		assert!((delta - (30.0 - bundled)).abs() < 0.01, "{delta}");
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

#[cfg(test)]
mod stylesheet_tests {
	use super::*;
	fn sheet(source: &str) -> Arc<crate::style::Stylesheet> {
		let mut s = (*crate::style::Stylesheet::bundled(false)).clone();
		s.merge(&crate::style::Stylesheet::parse(source).unwrap());
		Arc::new(s)
	}
	#[test]
	fn live_colors_follow_semantics_without_reflow() {
		let doc = crate::document::parse(
			"> *English 中文*\n\n# Heading\n\n***Both*** [*link*](https://example.com)",
		);
		let mut engine = LayoutEngine::new();
		let first = engine.layout(&doc, &LayoutOptions::default());
		let stylesheet = sheet(
			"format_version=1\nversion=1\n[blockquote]\ncolor='#123456'\n[em]\ncolor='#abcdef'\n[strong_em]\ncolor='#654321'\n[link]\ncolor='#102030'",
		);
		let second = engine.layout(
			&doc,
			&LayoutOptions {
				stylesheet: stylesheet.clone(),
				..Default::default()
			},
		);
		assert_eq!(second.reused, doc.blocks.len());
		assert_eq!(first.height, second.height);
		let glyph = second.blocks[0]
			.layout
			.draws
			.iter()
			.find_map(|d| {
				if let Draw::Glyph(g) = d {
					Some(g)
				} else {
					None
				}
			})
			.unwrap();
		assert_eq!(
			stylesheet.paint(glyph.paint),
			crate::style::Color(0xabcdefff).rgba()
		);
		let last = &second.blocks.last().unwrap().layout;
		let colors: Vec<_> = last
			.draws
			.iter()
			.filter_map(|d| {
				if let Draw::Glyph(g) = d {
					Some(stylesheet.paint(g.paint))
				} else {
					None
				}
			})
			.collect();
		assert!(colors.contains(&crate::style::Color(0x654321ff).rgba()));
		assert!(colors.contains(&crate::style::Color(0x102030ff).rgba()));
	}
	#[test]
	fn geometry_changes_invalidate_cache_and_keep_reading_text() {
		let doc = crate::document::parse(
			"# Heading\n\nText\n\n- item\n\n| A | B |\n|---|---|\n| C | D |",
		);
		let mut engine = LayoutEngine::new();
		let first = engine.layout(&doc, &LayoutOptions::default());
		let options = LayoutOptions {
			stylesheet: sheet(
				"format_version=1\nversion=1\n[body]\npadding=1.0\n[h1]\nsize=2.5\n[p]\nline_height=2.0\n[list_item]\npadding=0.5\n[table.cell]\npadding=1.0",
			),
			..Default::default()
		};
		let second = engine.layout(&doc, &options);
		assert_eq!(second.reused, 0);
		assert!(second.height > first.height);
		assert_eq!(
			first.extract_text(first.select_all(1).unwrap(), 1),
			second.extract_text(second.select_all(1).unwrap(), 1)
		);
		assert!(
			second.blocks[0]
				.layout
				.draws
				.iter()
				.filter_map(|d| if let Draw::Glyph(g) = d {
					Some(g.size)
				} else {
					None
				})
				.all(|size| size == 45.)
		);
		assert!(second.blocks[2].layout.draws.iter().any(|d| matches!(
			d,
			Draw::Box {
				role: Role::ListItem,
				..
			}
		)));
	}
}
