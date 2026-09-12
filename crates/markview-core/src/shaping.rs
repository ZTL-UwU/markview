//! Shared font shaping for document text, labels and renderer fallbacks.
use crate::style::{Font, Role, Stylesheet, TextAppearance, Variant};
use crate::{
	document::TextStyle,
	scene::{Draw, Glyph, Paint},
};
use anyhow::Result;
use parley::{
	FontContext, FontStyle, FontWeight, LayoutContext, StyleProperty,
};
use std::{collections::HashMap, ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone)]
struct Face {
	family: String,
	font: parley::FontData,
	style: FontStyle,
	weight: u16,
}

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
	pub stylesheet: Arc<Stylesheet>,
	pub appearance: TextAppearance,
	faces: HashMap<(Vec<Font>, u16), Vec<Face>>,
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
			stylesheet: Stylesheet::bundled(false),
			appearance: Stylesheet::bundled(false)
				.text(&TextAppearance::default(), Role::Body),
			faces: HashMap::new(),
		}
	}
	pub fn set_stylesheet(&mut self, stylesheet: Arc<Stylesheet>) {
		self.faces.clear();
		self.appearance =
			stylesheet.text(&TextAppearance::default(), Role::Body);
		self.stylesheet = stylesheet;
	}
	pub fn validate_stylesheet(
		&mut self,
		_stylesheet: &Stylesheet,
	) -> Result<()> {
		// An unavailable lookfor list is an ignored fallback, not an invalid
		// stylesheet. choose_font skips it and continues with later candidates.
		Ok(())
	}
	fn choose_font(
		&mut self,
		text: &str,
		appearance: &TextAppearance,
	) -> Option<Face> {
		let key = (appearance.font.clone(), appearance.weight);
		if !self.faces.contains_key(&key) {
			let mut faces = Vec::new();
			for candidate in &appearance.font {
				let style = match candidate.variant {
					Variant::Normal => FontStyle::Normal,
					Variant::Italic => FontStyle::Italic,
					Variant::Oblique => FontStyle::Oblique(None),
				};
				let weight = candidate.weight.unwrap_or(appearance.weight);
				let families: Vec<_> = if let Some(def) =
					self.stylesheet.fontdefs.get(&candidate.family)
				{
					def.lookfor
						.iter()
						.find_map(|name| {
							let generic = match name.as_str() {
								"serif" => Some(parley::GenericFamily::Serif),
								"sans-serif" => {
									Some(parley::GenericFamily::SansSerif)
								}
								"monospace" => {
									Some(parley::GenericFamily::Monospace)
								}
								_ => None,
							};
							if let Some(generic) = generic {
								let ids: Vec<_> = self
									.fonts
									.collection
									.generic_families(generic)
									.collect();
								ids.into_iter().find_map(|id| {
									self.fonts.collection.family(id)
								})
							} else {
								self.fonts.collection.family_by_name(name)
							}
						})
						.into_iter()
						.collect()
				} else {
					// Partial stylesheets used by low-level callers may omit the
					// fontdef table entirely. A declared-but-unselected variant,
					// however, is intentionally unavailable.
					if self.stylesheet.has_fontdef_variant(&candidate.family) {
						Vec::new()
					} else {
						self.fonts
							.collection
							.family_by_name(&candidate.family)
							.into_iter()
							.collect()
					}
				};
				for family in families {
					let Some(info) = family.match_font(
						Default::default(),
						style,
						FontWeight::new(weight as f32),
						false,
					) else {
						continue;
					};
					let axis = |tag: &[u8; 4], value: f32| {
						info.axes().iter().any(|a| {
							a.tag.to_be_bytes() == *tag
								&& a.min <= value && value <= a.max
						})
					};
					let exact_style = info.style() == style
						|| (candidate.variant == Variant::Oblique
							&& matches!(info.style(), FontStyle::Oblique(_)))
						|| match candidate.variant {
							Variant::Italic => axis(b"ital", 1.),
							Variant::Oblique => axis(b"slnt", -14.),
							Variant::Normal => {
								axis(b"ital", 0.) || axis(b"slnt", 0.)
							}
						};
					let exact_weight = info.weight()
						== FontWeight::new(weight as f32)
						|| axis(b"wght", weight as f32);
					if !exact_style || !exact_weight {
						continue;
					}
					if let Some(data) =
						info.load(Some(&mut self.fonts.source_cache))
					{
						faces.push(Face {
							family: family.name().into(),
							font: parley::FontData::new(data, info.index()),
							style: if matches!(
								info.style(),
								FontStyle::Oblique(_)
							) && candidate.variant == Variant::Oblique
							{
								info.style()
							} else {
								style
							},
							weight,
						});
					}
				}
			}
			self.faces.insert(key.clone(), faces);
		}
		self.faces[&key]
			.iter()
			.find(|face| {
				swash::FontRef::from_index(
					face.font.data.data(),
					face.font.index as usize,
				)
				.is_some_and(|font| {
					text.chars().all(|c| {
						c.is_control()
							|| matches!(c as u32,0x200c..=0x200f|0xfe00..=0xfe0f|0xe0100..=0xe01ef)
							|| font.charmap().map(c) != 0
					})
				})
			})
			.cloned()
	}
	pub(crate) fn shape(
		&mut self,
		text: &str,
		spans: &[Span],
		size: f32,
		_sans: bool,
	) -> Vec<Cluster> {
		if text.is_empty() {
			return Vec::new();
		}
		let base = self.appearance.clone();
		let appearances: Vec<_> = spans
			.iter()
			.map(|span| self.stylesheet.inline(&base, &span.style))
			.collect();
		let mut choices: Vec<(Range<usize>, Option<Face>)> = Vec::new();
		// Resolve whole joining-script words together; elsewhere resolve grapheme clusters.
		// All ranges are subsequently shaped in one paragraph, preserving bidi and context.
		for (start, word) in text.split_word_bound_indices() {
			let joining = word.chars().any(
				|c| matches!(c as u32,0x600..=0x1cff|0xa800..=0xabff|0x11000..=0x11fff),
			);
			let mut parts: Vec<(usize, &str)> = Vec::new();
			for (offset, cluster) in word.grapheme_indices(true) {
				let span_at =
					|pos| spans.iter().position(|s| s.range.contains(&pos));
				if joining
					&& let Some((previous, part)) = parts.last_mut()
					&& span_at(start + *previous) == span_at(start + offset)
				{
					*part = &word[*previous..offset + cluster.len()];
				} else {
					parts.push((offset, cluster));
				}
			}
			for (offset, part) in parts {
				let pos = start + offset;
				let appearance = spans
					.iter()
					.position(|s| s.range.contains(&pos))
					.map(|i| &appearances[i])
					.unwrap_or(&base);
				let face = self.choose_font(part, appearance);
				if let Some((range, previous)) = choices.last_mut()
					&& range.end == pos
					&& face.as_ref().map(|f| (&f.family, f.style, f.weight))
						== previous
							.as_ref()
							.map(|f| (&f.family, f.style, f.weight))
				{
					range.end = pos + part.len();
					continue;
				}
				choices.push((pos..pos + part.len(), face));
			}
		}
		let mut builder =
			self.context
				.ranged_builder(&mut self.fonts, text, 1.0, false);
		builder.push_default(StyleProperty::FontSize(size));
		builder.push_default(StyleProperty::FontFamily("sans-serif".into()));
		builder.push_default(StyleProperty::FontWeight(FontWeight::NORMAL));
		builder.push_default(StyleProperty::FontStyle(FontStyle::Normal));
		builder.push_default(StyleProperty::Brush(usize::MAX));
		for (range, face) in &choices {
			if let Some(face) = face {
				builder.push(
					StyleProperty::FontFamily(
						parley::FontFamilyName::Named(
							face.family.as_str().into(),
						)
						.into(),
					),
					range.clone(),
				);
				builder
					.push(StyleProperty::FontStyle(face.style), range.clone());
				builder.push(
					StyleProperty::FontWeight(FontWeight::new(
						face.weight as f32,
					)),
					range.clone(),
				);
			}
		}
		for (i, span) in spans.iter().enumerate() {
			builder.push(StyleProperty::Brush(i), span.range.clone());
			builder.push(
				StyleProperty::FontSize(size * appearances[i].size),
				span.range.clone(),
			);
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
							paint: appearances
								.get(index)
								.unwrap_or(&base)
								.paint,
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
		let old = self.appearance.clone();
		let role = match paint {
			Paint::Styled(r, _) => r,
			_ => Role::Ui,
		};
		let parent = if role.ui() {
			self.stylesheet.text(&TextAppearance::default(), Role::Ui)
		} else {
			old.clone()
		};
		self.appearance = self.stylesheet.text(&parent, role);
		let paint = if matches!(
			paint,
			Paint::Styled(_, crate::style::ColorField::Color)
		) {
			self.appearance.paint
		} else {
			paint
		};
		let decoration = self.appearance.decoration.clone();
		let clusters = self.shape(text, &[], size * self.appearance.size, true);
		self.appearance = old;
		let mut draws = Vec::new();
		let mut cursor = x;
		if !role.ui() {
			let width = clusters.iter().map(|c| c.width).sum();
			let ascent = clusters.iter().map(|c| c.ascent).fold(0., f32::max);
			let descent = clusters.iter().map(|c| c.descent).fold(0., f32::max);
			draws.push(Draw::Rect(
				crate::scene::Rect {
					x,
					y: baseline - ascent,
					w: width,
					h: ascent + descent,
				},
				Paint::Styled(role, crate::style::ColorField::Background),
			));
		}
		for c in clusters {
			for mut g in c.glyphs {
				g.x += cursor;
				g.y += baseline;
				g.paint = paint;
				draws.push(Draw::Glyph(g));
			}
			cursor += c.width;
		}
		for d in decoration {
			draws.push(Draw::Rect(
				crate::scene::Rect {
					x,
					y: if d == crate::style::Decoration::Strike {
						baseline - size * 0.3
					} else {
						baseline + size * 0.12
					},
					w: cursor - x,
					h: 1.,
				},
				paint,
			));
		}
		draws
	}

	/// Advance width of a UI label at `size`.
	pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
		self.shape(text, &[], size * self.appearance.size, true)
			.iter()
			.map(|c| c.width)
			.sum()
	}

	/// Shorten `text` to `max` width, keeping its start and end like a browser.
	pub fn fit(&mut self, text: &str, size: f32, max: f32) -> String {
		if self.text_width(text, size) <= max {
			return text.to_string();
		}
		let chars: Vec<&str> = text.graphemes(true).collect();
		let mut tail = 16.min(chars.len() / 3);
		while tail > 0
			&& self.text_width(
				&format!("…{}", chars[chars.len() - tail..].concat()),
				size,
			) > max
		{
			tail -= 1;
		}
		if self.text_width("…", size) > max {
			return String::new();
		}
		let build = |head: usize| {
			let mut out = chars[..head].concat();
			out.push('…');
			out.push_str(&chars[chars.len() - tail..].concat());
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

#[cfg(test)]
mod stylesheet_tests;
