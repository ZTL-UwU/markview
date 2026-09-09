//! Shared font shaping for document text, labels and renderer fallbacks.
use crate::{
	document::TextStyle,
	scene::{Draw, Glyph, Paint},
};
use parley::{
	FontContext, FontStyle, FontWeight, LayoutContext, StyleProperty,
};
use std::{ops::Range, sync::Arc};
#[derive(Clone)]
pub(crate) struct Span {
	pub(crate) range: Range<usize>,
	pub(crate) style: TextStyle,
}
#[derive(Clone)]
pub(crate) struct Cluster {
	pub(crate) rtl: bool,
	pub(crate) range: Range<usize>,
	pub(crate) width: f32,
	pub(crate) ascent: f32,
	pub(crate) descent: f32,
	pub(crate) glyphs: Vec<Glyph>,
	pub(crate) continuation: bool,
}
/// Reusable shaping context for UI labels and document text.
pub struct TextShaper {
	fonts: FontContext,
	context: LayoutContext<usize>,
}
impl Default for TextShaper {
	fn default() -> Self {
		Self::new()
	}
}
impl TextShaper {
	pub fn new() -> Self {
		Self {
			fonts: FontContext::new(),
			context: LayoutContext::new(),
		}
	}
	pub(crate) fn shape(
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
						});
						x += g.advance;
					}
					clusters.push(Cluster {
						rtl: c.is_rtl(),
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

	/// Advance width of a UI label at `size`.
	pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
		self.shape(text, &[], size, true)
			.iter()
			.map(|c| c.width)
			.sum()
	}

	/// Shorten `text` to `max` width, keeping its start and end like a browser.
	pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
		if self.text_width(text, size) <= max {
			return text.to_string();
		}
		let chars: Vec<char> = text.chars().collect();
		let tail = 16.min(chars.len() / 3);
		let build = |head: usize| {
			let mut out: String = chars[..head].iter().collect();
			out.push('…');
			out.extend(chars[chars.len() - tail..].iter());
			out
		};
		let (mut lo, mut hi) = (0, chars.len() - tail);
		while lo < hi {
			let mid = (lo + hi).div_ceil(2);
			if self.text_width(&build(mid), size) <= max {
				lo = mid;
			} else {
				hi = mid - 1;
			}
		}
		build(lo)
	}

	/// A right-aligned label, trimmed to `max` width.
	pub fn right_label(
		&mut self,
		text: &str,
		size: f32,
		max: f32,
		right: f32,
		baseline: f32,
		paint: Paint,
	) -> Vec<Draw> {
		let text = self.fit(text, size, max);
		let width = self.text_width(&text, size);
		self.label(&text, size, (right - width).max(0.0), baseline, paint)
	}
}
