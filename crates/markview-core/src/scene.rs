//! Immutable drawing and hit-test geometry shared by layout and rendering.
use crate::{math::MathBox, text::TextNode};
use parley::FontData;
use std::{collections::HashMap, ops::Range, sync::Arc};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Paint {
	Styled(crate::style::Role, crate::style::ColorField),
	Cascade(u128, crate::style::ColorField),
	#[default]
	Text,
	Muted,
	Accent,
	Panel,
	Glass,
	Scrim,
	Shadow,
	Border,
	Background,
	Error,
}

impl Paint {
	pub fn cascade(
		self,
		role: crate::style::Role,
		field: crate::style::ColorField,
	) -> Self {
		let chain = match self {
			Self::Cascade(v, _) => v,
			Self::Styled(r, _) => r as u128 + 1,
			_ => crate::style::Role::Body as u128 + 1,
		};
		// A repeated container replaces its earlier occurrence. This keeps the
		// finite semantic ancestry compact even for deeply nested lists/quotes.
		let mut remaining = chain;
		let mut compact = 0;
		let mut shift = 0;
		while remaining != 0 {
			let id = remaining & 63;
			remaining >>= 6;
			if id != role as u128 + 1 {
				compact |= id << shift;
				shift += 6;
			}
		}
		Self::Cascade((compact << 6) | (role as u128 + 1), field)
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
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
	pub x: f32,
	pub y: f32,
	pub w: f32,
	pub h: f32,
}
impl Rect {
	pub fn intersect(self, other: Self) -> Option<Self> {
		let x = self.x.max(other.x);
		let y = self.y.max(other.y);
		let w = (self.x + self.w).min(other.x + other.w) - x;
		let h = (self.y + self.h).min(other.y + other.h) - y;
		(w > 0.0 && h > 0.0).then_some(Self { x, y, w, h })
	}
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
	Box {
		rect: Rect,
		role: crate::style::Role,
		radius: f32,
		border: f32,
		left_only: bool,
	},
	Math {
		math: Arc<MathBox>,
		paint: Paint,
		x: f32,
		y: f32,
	},
}
impl Draw {
	pub fn translate(&mut self, x: f32, y: f32) {
		match self {
			Self::Glyph(g) => {
				g.x += x;
				g.y += y;
			}
			Self::Rect(r, _) | Self::Box { rect: r, .. } => {
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
	/// Space reserved below `rect` for this block's horizontal scrollbar, so
	/// the bar never crowds the last line of text.
	pub gutter: f32,
}

/// One clickable link fragment, in block-local coordinates.
#[derive(Clone, Debug)]
pub struct LinkRect {
	pub command: usize,
	pub rect: Rect,
	pub url: String,
}

#[derive(Debug, Default)]
pub struct BlockLayout {
	pub text: Vec<TextNode>,
	pub draws: Vec<Draw>,
	pub height: f32,
	pub width: f32,
	pub overflow: Vec<Overflow>,
	pub links: Vec<LinkRect>,
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
	pub document_box: Option<Draw>,
	pub blocks: Vec<PlacedBlock>,
	pub height: f32,
	pub width: f32,
	pub reused: usize,
	pub degraded: usize,
	pub math_errors: usize,
}

impl LayoutSnapshot {
	/// The link under a point in document coordinates: `x` from the column's
	/// left edge, `y` from the top of the document including the scroll offset.
	pub fn link_at(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<&str> {
		for (bi, block) in self.blocks.iter().enumerate() {
			let y = y - block.y;
			if y < 0.0 || y > block.layout.height {
				continue;
			}
			for link in &block.layout.links {
				let (offset, clip) =
					block.layout.command_view(link.command, bi, horizontal);
				let mut rect = Rect {
					x: link.rect.x - offset,
					..link.rect
				};
				if let Some(clip) = clip {
					let Some(clipped) = rect.intersect(clip) else {
						continue;
					};
					rect = clipped;
				}
				if rect.contains(x, y) {
					return Some(&link.url);
				}
			}
		}
		None
	}
}

/// Default painted thickness of a scrollbar at rest, in logical pixels.
pub const SCROLLBAR_THICKNESS: f32 = 8.0;
/// Default painted thickness of a hovered or dragged thumb. The interactive
/// band is at least this wide, so the grab zone is what the reader sees.
pub const SCROLLBAR_HOVER_THICKNESS: f32 = 14.0;
/// Default space an overflowing block reserves below its content for its
/// horizontal scrollbar, keeping the bar clear of the last line of text.
pub const SCROLLBAR_GUTTER: f32 = 8.0;
/// Shortest thumb a very long document may shrink to, so the bar stays
/// grabbable.
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// The thicknesses one scrollbar is painted at. A stylesheet may set both to
/// the same value, which disables the thickening on hover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
	/// Painted thickness at rest.
	pub thickness: f32,
	/// Painted thickness of the thumb while hovered or dragged.
	pub thickness_hover: f32,
}

impl Default for ScrollbarMetrics {
	fn default() -> Self {
		Self::DOCUMENT
	}
}

impl ScrollbarMetrics {
	/// The reader's vertical bar: thin at rest, thick while held.
	pub const DOCUMENT: Self = Self {
		thickness: SCROLLBAR_THICKNESS,
		thickness_hover: SCROLLBAR_HOVER_THICKNESS,
	};
	/// A wide block's horizontal bar, which does not thicken by default.
	pub const OVERFLOW: Self = Self {
		thickness: SCROLLBAR_THICKNESS,
		thickness_hover: SCROLLBAR_THICKNESS,
	};
	/// The interactive band: the widest the bar is ever painted.
	pub fn band(&self) -> f32 {
		self.thickness.max(self.thickness_hover)
	}
	/// The band a wide block's bar occupies below its content: the reserved
	/// gutter, or at least the bar itself when no gutter is configured.
	pub fn overflow_band(&self, gutter: f32) -> f32 {
		gutter.max(self.band())
	}
}

/// A scrollbar track: where the thumb sits, what it maps to, and the two
/// thicknesses it is painted at. Both the reader's document bar and every wide
/// block's bar use this, so drawing and pointer handling cannot disagree.
#[derive(Clone, Copy, Debug)]
pub struct Scrollbar {
	/// The interactive track, spanning the full grab thickness.
	pub track: Rect,
	/// The thumb inside `track`, also spanning the full grab thickness.
	pub thumb: Rect,
	/// The bar runs down the window; otherwise it runs across the content.
	vertical: bool,
	/// Scroll offset at the end of the thumb's travel.
	max_scroll: f32,
	/// Thumb travel along the track.
	travel: f32,
	/// Thumb length along the track.
	thumb_len: f32,
	/// Thicknesses this bar is painted at.
	metrics: ScrollbarMetrics,
}

/// Thumb length and its offset along a track, or `None` when the content fits
/// and no scrollbar is shown.
fn thumb_span(
	length: f32,
	scroll: f32,
	content: f32,
	viewport: f32,
) -> Option<(f32, f32)> {
	if content <= viewport || length <= 0.0 {
		return None;
	}
	let thumb = (length * viewport / content)
		.clamp(SCROLLBAR_MIN_THUMB.min(length), length);
	let at = (scroll / (content - viewport)).clamp(0.0, 1.0) * (length - thumb);
	Some((thumb, at))
}

impl Scrollbar {
	/// A vertical bar in `track`, or `None` when nothing scrolls.
	pub fn vertical(
		track: Rect,
		scroll: f32,
		content: f32,
		viewport: f32,
		metrics: ScrollbarMetrics,
	) -> Option<Self> {
		let (thumb_len, at) = thumb_span(track.h, scroll, content, viewport)?;
		Some(Self {
			track,
			thumb: Rect {
				y: track.y + at,
				h: thumb_len,
				..track
			},
			vertical: true,
			max_scroll: content - viewport,
			travel: track.h - thumb_len,
			thumb_len,
			metrics,
		})
	}

	/// A horizontal bar in `track`, or `None` when nothing scrolls.
	pub fn horizontal(
		track: Rect,
		scroll: f32,
		content: f32,
		viewport: f32,
		metrics: ScrollbarMetrics,
	) -> Option<Self> {
		let (thumb_len, at) = thumb_span(track.w, scroll, content, viewport)?;
		Some(Self {
			track,
			thumb: Rect {
				x: track.x + at,
				w: thumb_len,
				..track
			},
			vertical: false,
			max_scroll: content - viewport,
			travel: track.w - thumb_len,
			thumb_len,
			metrics,
		})
	}

	/// The track and thumb to paint. The track stays at its rest thickness; a
	/// hovered or dragged thumb may thicken, up to the interactive band.
	pub fn bars(&self, expanded: bool) -> (Rect, Rect) {
		let thickness = if expanded {
			self.metrics.thickness_hover
		} else {
			self.metrics.thickness
		};
		(
			self.paint(self.track, self.metrics.thickness),
			self.paint(self.thumb, thickness),
		)
	}

	/// `rect` narrowed to `size` across the bar's cross axis, centred on the
	/// interactive band.
	fn paint(&self, rect: Rect, size: f32) -> Rect {
		if self.vertical {
			let size = size.clamp(0.0, rect.w);
			Rect {
				x: rect.x + (rect.w - size) * 0.5,
				w: size,
				..rect
			}
		} else {
			let size = size.clamp(0.0, rect.h);
			Rect {
				y: rect.y + (rect.h - size) * 0.5,
				h: size,
				..rect
			}
		}
	}

	/// True when the point is inside the interactive track.
	pub fn hit(&self, x: f32, y: f32) -> bool {
		self.track.contains(x, y)
	}

	/// True when the point is on the thumb rather than on the empty track.
	/// The whole grab thickness counts, so a press that looks like it landed
	/// on the bar starts a drag instead of jumping the scroll offset.
	pub fn on_thumb(&self, x: f32, y: f32) -> bool {
		if !self.track.contains(x, y) {
			return false;
		}
		let (position, start) = if self.vertical {
			(y, self.thumb.y)
		} else {
			(x, self.thumb.x)
		};
		position >= start && position <= start + self.thumb_len
	}

	/// The pointer's offset inside the thumb, kept constant during a drag.
	pub fn grab(&self, x: f32, y: f32) -> f32 {
		let (pointer, start, offset) = if self.vertical {
			(y, self.track.y, self.thumb.y - self.track.y)
		} else {
			(x, self.track.x, self.thumb.x - self.track.x)
		};
		(pointer - start - offset).clamp(0.0, self.thumb_len)
	}

	/// The scroll offset for a pointer at `(x, y)` holding the thumb at
	/// `grab`.
	pub fn scroll_for(&self, x: f32, y: f32, grab: f32) -> f32 {
		if self.travel <= 0.0 {
			return 0.0;
		}
		let (pointer, start) = if self.vertical {
			(y, self.track.y)
		} else {
			(x, self.track.x)
		};
		(pointer - grab - start).clamp(0.0, self.travel) / self.travel
			* self.max_scroll
	}
}

/// Logical window coordinates. DPI conversion happens once at the platform edge.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
	pub width: f32,
	pub height: f32,
	pub left: f32,
	pub top: f32,
	pub bottom: f32,
	pub scroll: f32,
}
impl Viewport {
	pub fn clip(self) -> Rect {
		Rect {
			x: 0.0,
			y: self.top,
			w: self.width,
			h: (self.height - self.top - self.bottom).max(0.0),
		}
	}
	pub fn document_point(self, x: f32, y: f32) -> (f32, f32) {
		(x - self.left, y - self.top + self.scroll)
	}
	pub fn window_rect(self, rect: Rect) -> Rect {
		Rect {
			x: rect.x + self.left,
			y: rect.y + self.top - self.scroll,
			..rect
		}
	}
}
impl BlockLayout {
	/// Shared overflow transform for painting, link hits and text selection.
	pub fn command_view(
		&self,
		command: usize,
		block: usize,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> (f32, Option<Rect>) {
		self.overflow
			.iter()
			.enumerate()
			.find(|(_, o)| o.commands.contains(&command))
			.map(|(oi, o)| {
				(
					horizontal
						.get(&(block, oi))
						.copied()
						.unwrap_or(0.0)
						.clamp(0.0, (o.content_width - o.rect.w).max(0.0)),
					Some(o.rect),
				)
			})
			.unwrap_or((0.0, None))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn scrolled_links_cannot_be_activated_outside_their_clip() {
		let document = crate::document::parse(
			"| Link |\n|---|\n| [abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz](https://example.com) |\n",
		);
		let snapshot = crate::layout::LayoutEngine::new().layout(
			&document,
			&crate::layout::LayoutOptions {
				width: 100.0,
				..Default::default()
			},
		);
		let block = &snapshot.blocks[0];
		let overflow = &block.layout.overflow[0];
		let link = &block.layout.links[0];
		let horizontal = HashMap::from([((0, 0), 80.0)]);
		assert_eq!(
			snapshot.link_at(
				overflow.rect.x - 1.0,
				block.y + link.rect.y + 1.0,
				&horizontal
			),
			None
		);
		assert_eq!(
			snapshot.link_at(
				overflow.rect.x + 5.0,
				block.y + link.rect.y + 1.0,
				&horizontal
			),
			Some("https://example.com")
		);
	}
	#[test]
	fn viewport_translation_and_clipping_share_logical_coordinates() {
		let viewport = Viewport {
			width: 800.0,
			height: 600.0,
			left: 20.0,
			top: 68.0,
			bottom: 38.0,
			scroll: 100.0,
		};
		let rect = viewport.window_rect(Rect {
			x: 30.0,
			y: 150.0,
			w: 60.0,
			h: 25.0,
		});
		assert_eq!(viewport.document_point(rect.x, rect.y), (30.0, 150.0));
		assert!(viewport.clip().contains(rect.x, rect.y));
		assert!(!viewport.clip().contains(rect.x, 30.0));
	}
	#[test]
	fn scrollbar_thumb_spans_the_track_and_maps_to_the_scroll_range() {
		let metrics = ScrollbarMetrics::DOCUMENT;
		let track = Rect {
			x: 786.0,
			y: 40.0,
			w: metrics.band(),
			h: 732.0,
		};
		// A document that fits the viewport shows no scrollbar at all.
		assert!(
			Scrollbar::vertical(track, 0.0, 100.0, 100.0, metrics).is_none()
		);
		let top =
			Scrollbar::vertical(track, 0.0, 2000.0, 500.0, metrics).unwrap();
		assert_eq!(top.thumb.y, 40.0);
		assert_eq!(top.thumb.h, track.h * 0.25);
		let travel = track.h - top.thumb.h;
		let bottom =
			Scrollbar::vertical(track, 1500.0, 2000.0, 500.0, metrics).unwrap();
		assert!((bottom.thumb.y - (40.0 + travel)).abs() < 0.01);
		// Grabbing 10 px inside the thumb and dragging by the whole travel
		// moves the document by exactly the scroll maximum.
		let grab = top.grab(800.0, 50.0);
		assert_eq!(grab, 10.0);
		assert!(
			(top.scroll_for(800.0, 50.0 + travel, grab) - 1500.0).abs() < 0.01
		);
		assert_eq!(top.scroll_for(800.0, -460.0, grab), 0.0);
		// A press on the empty track puts the thumb's start under the pointer.
		assert_eq!(top.scroll_for(800.0, 40.0, 0.0), 0.0);
		assert_eq!(top.scroll_for(800.0, 40.0 + travel, 0.0), 1500.0);
	}
	#[test]
	fn scrollbar_grab_zone_is_the_band_and_the_thumb_thickens_on_hover() {
		let metrics = ScrollbarMetrics::DOCUMENT;
		let band = Rect {
			x: 786.0,
			y: 40.0,
			w: metrics.band(),
			h: 700.0,
		};
		let vertical =
			Scrollbar::vertical(band, 0.0, 2000.0, 500.0, metrics).unwrap();
		assert!(vertical.hit(786.0, 80.0));
		assert!(vertical.hit(799.0, 80.0));
		assert!(!vertical.hit(785.0, 80.0));
		assert!(!vertical.hit(790.0, 35.0));
		assert!(!vertical.hit(790.0, 745.0));
		// At rest the bar is centred and thin; the hovered thumb fills the
		// band while the track keeps its rest thickness.
		let (track, thumb) = vertical.bars(false);
		assert_eq!(track.w, metrics.thickness);
		assert_eq!(track.x, band.x + (band.w - metrics.thickness) * 0.5);
		assert_eq!(thumb.w, metrics.thickness);
		let (expanded_track, expanded_thumb) = vertical.bars(true);
		assert_eq!(expanded_track.x, track.x);
		assert_eq!(expanded_track.w, metrics.thickness);
		assert_eq!(expanded_thumb.y, vertical.thumb.y);
		assert_eq!(expanded_thumb.x, band.x);
		assert_eq!(expanded_thumb.w, metrics.thickness_hover);
		// Anywhere across the band at the thumb's own extent grabs the thumb,
		// so a press that looks like it landed on the bar never jumps.
		assert!(vertical.on_thumb(band.x, band.y + 1.0));
		assert!(vertical.on_thumb(band.x + band.w, vertical.thumb.y));
		assert!(
			vertical
				.on_thumb(band.x + band.w, vertical.thumb.y + vertical.thumb.h)
		);
		assert!(
			!vertical
				.on_thumb(band.x, vertical.thumb.y + vertical.thumb.h + 1.0)
		);
		assert!(!vertical.on_thumb(band.x - 1.0, vertical.thumb.y));
	}
	#[test]
	fn horizontal_scrollbar_uses_the_block_gutter_and_keeps_its_thickness() {
		let metrics = ScrollbarMetrics::OVERFLOW;
		let band = Rect {
			x: 40.0,
			y: 300.0,
			w: 200.0,
			h: SCROLLBAR_GUTTER,
		};
		let bar =
			Scrollbar::horizontal(band, 0.0, 400.0, 200.0, metrics).unwrap();
		assert_eq!(bar.thumb.w, 100.0);
		assert_eq!(bar.thumb.x, 40.0);
		// The band is the reserved gutter, or the bar itself without one.
		assert_eq!(metrics.overflow_band(SCROLLBAR_GUTTER), SCROLLBAR_GUTTER);
		assert_eq!(metrics.overflow_band(30.0), 30.0);
		assert_eq!(metrics.overflow_band(0.0), metrics.band());
		let (track, thumb) = bar.bars(false);
		assert_eq!(track.h, metrics.thickness);
		assert_eq!(track.y, band.y + (band.h - metrics.thickness) * 0.5);
		assert_eq!(thumb.h, metrics.thickness);
		// The wide block's bar does not thicken: both states paint the same.
		let (expanded_track, expanded_thumb) = bar.bars(true);
		assert_eq!(expanded_track.h, metrics.thickness);
		assert_eq!(expanded_thumb.h, metrics.thickness_hover);
		assert_eq!(expanded_thumb.y, thumb.y);
		assert!(bar.hit(100.0, 300.0));
		assert!(bar.hit(100.0, 307.0));
		assert!(!bar.hit(100.0, 309.0));
		assert!(!bar.hit(20.0, 300.0));
		assert!(bar.on_thumb(100.0, 300.0));
		assert!(!bar.on_thumb(150.0, 300.0));
		// A press on the empty track jumps the thumb under the pointer.
		assert_eq!(bar.scroll_for(240.0, 300.0, 0.0), 200.0);
	}
}
