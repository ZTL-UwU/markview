//! Client-side window frame: shadow, border, and corner radius.
use gpui::{
	BoxShadow, CursorStyle, Decorations, Point, ResizeEdge, Size, Tiling,
	Window, WindowBackgroundAppearance, hsla, point, px,
};

#[cfg(target_os = "linux")]
const SHADOW: f32 = 12.0;
#[cfg(not(target_os = "linux"))]
const SHADOW: f32 = 0.0;

#[cfg(target_os = "macos")]
const RADIUS: f32 = 0.0;
#[cfg(not(target_os = "macos"))]
const RADIUS: f32 = 12.0;

const BORDER: f32 = 1.0;

#[derive(Clone, Copy, Default)]
pub(super) struct FramePad {
	pub left: f32,
	pub top: f32,
	pub right: f32,
	pub bottom: f32,
}

impl FramePad {
	pub fn to_content(self, x: f32, y: f32) -> (f32, f32) {
		(x - self.left, y - self.top)
	}

	pub fn origin(self) -> (f32, f32) {
		(self.left, self.top)
	}
}

pub(super) fn shadow_size() -> f32 {
	SHADOW
}

pub(super) fn radius() -> f32 {
	RADIUS
}

pub(super) fn border_size() -> f32 {
	BORDER
}

pub(super) fn pad(window: &Window) -> FramePad {
	pad_for(window.window_decorations())
}

pub(super) fn pad_for(decorations: Decorations) -> FramePad {
	match decorations {
		Decorations::Server => FramePad::default(),
		Decorations::Client { tiling } => FramePad {
			left: if tiling.left { 0.0 } else { SHADOW },
			top: if tiling.top { 0.0 } else { SHADOW },
			right: if tiling.right { 0.0 } else { SHADOW },
			bottom: if tiling.bottom { 0.0 } else { SHADOW },
		},
	}
}

pub(super) fn prepare(window: &mut Window) {
	let decorations = window.window_decorations();
	let shadow = px(SHADOW);
	window.set_client_inset(shadow);
	window.set_background_appearance(match decorations {
		Decorations::Client { tiling }
			if !tiling.is_tiled() && SHADOW > 0.0 =>
		{
			WindowBackgroundAppearance::Transparent
		}
		_ => WindowBackgroundAppearance::Opaque,
	});
}

pub(super) fn corner_radius(tiling: Tiling, top: bool, left: bool) -> f32 {
	if RADIUS == 0.0 {
		return 0.0;
	}
	let vertical = if top { tiling.top } else { tiling.bottom };
	let horizontal = if left { tiling.left } else { tiling.right };
	if vertical || horizontal { 0.0 } else { RADIUS }
}

pub(super) fn resize_edge(
	pos: Point<gpui::Pixels>,
	size: Size<gpui::Pixels>,
	pad: FramePad,
) -> Option<ResizeEdge> {
	if SHADOW <= 0.0 {
		return None;
	}
	let x = f32::from(pos.x);
	let y = f32::from(pos.y);
	let w = f32::from(size.width);
	let h = f32::from(size.height);
	let left = pad.left > 0.0 && x < pad.left;
	let right = pad.right > 0.0 && x > w - pad.right;
	let top = pad.top > 0.0 && y < pad.top;
	let bottom = pad.bottom > 0.0 && y > h - pad.bottom;
	Some(match (top, bottom, left, right) {
		(true, _, true, _) => ResizeEdge::TopLeft,
		(true, _, _, true) => ResizeEdge::TopRight,
		(_, true, true, _) => ResizeEdge::BottomLeft,
		(_, true, _, true) => ResizeEdge::BottomRight,
		(true, _, _, _) => ResizeEdge::Top,
		(_, true, _, _) => ResizeEdge::Bottom,
		(_, _, true, _) => ResizeEdge::Left,
		(_, _, _, true) => ResizeEdge::Right,
		_ => return None,
	})
}

pub(super) fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
	match edge {
		ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
		ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
		ResizeEdge::TopLeft | ResizeEdge::BottomRight => {
			CursorStyle::ResizeUpLeftDownRight
		}
		ResizeEdge::TopRight | ResizeEdge::BottomLeft => {
			CursorStyle::ResizeUpRightDownLeft
		}
	}
}

pub(super) fn shadow() -> Vec<BoxShadow> {
	vec![BoxShadow {
		color: hsla(0.0, 0.0, 0.0, 0.4),
		offset: point(px(0.0), px(0.0)),
		blur_radius: px(SHADOW / 2.0),
		spread_radius: px(0.0),
	}]
}

pub(super) fn border_color(dark: bool) -> gpui::Hsla {
	if dark {
		hsla(0.0, 0.0, 1.0, 0.18)
	} else {
		hsla(0.0, 0.0, 0.0, 0.16)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{Decorations, Tiling, point, px, size};

	#[test]
	fn server_decorations_have_no_client_frame() {
		let pad = pad_for(Decorations::Server);
		assert_eq!(pad.left + pad.top + pad.right + pad.bottom, 0.0);
	}

	#[test]
	fn tiled_edges_drop_the_shadow_inset() {
		let pad = pad_for(Decorations::Client {
			tiling: Tiling {
				top: true,
				left: false,
				right: true,
				bottom: false,
			},
		});
		assert_eq!(pad.top, 0.0);
		assert_eq!(pad.right, 0.0);
		assert_eq!(pad.left, SHADOW);
		assert_eq!(pad.bottom, SHADOW);
	}

	#[test]
	fn resize_hits_shadow_corners() {
		let pad = FramePad {
			left: 12.0,
			top: 12.0,
			right: 12.0,
			bottom: 12.0,
		};
		let size = size(px(400.0), px(300.0));
		if SHADOW <= 0.0 {
			assert!(resize_edge(point(px(2.0), px(2.0)), size, pad).is_none());
			return;
		}
		assert!(matches!(
			resize_edge(point(px(2.0), px(2.0)), size, pad),
			Some(ResizeEdge::TopLeft)
		));
		assert!(resize_edge(point(px(50.0), px(50.0)), size, pad).is_none());
	}

	#[test]
	fn tiled_corners_drop_the_radius() {
		let tiling = Tiling {
			top: true,
			left: false,
			right: false,
			bottom: false,
		};
		assert_eq!(corner_radius(tiling, true, true), 0.0);
		assert_eq!(
			corner_radius(tiling, false, true),
			if RADIUS > 0.0 { RADIUS } else { 0.0 }
		);
	}
}
