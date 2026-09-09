//! Immutable drawing and hit-test geometry shared by layout and rendering.
use crate::{math::MathBox, text::TextNode};
use parley::FontData;
use std::{collections::HashMap, ops::Range, sync::Arc};
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
}
