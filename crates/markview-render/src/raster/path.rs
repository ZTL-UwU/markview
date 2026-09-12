use super::{Entry, RasterCache, RasterKey};
use crate::{View, intersect};
use markview_core::{document::fingerprint, scene::Rect};
use ratex_types::PathCommand;
impl RasterCache {
	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	pub(crate) fn path(
		&mut self,
		queue: &wgpu::Queue,
		geometry: &mut crate::geometry::Geometry,
		commands: &[PathCommand],
		fill: bool,
		x: f32,
		y: f32,
		size: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		let mut b = tiny_skia::PathBuilder::new();
		let mut bits = vec![u64::from(fill)];
		for command in commands {
			match *command {
				PathCommand::MoveTo { x, y } => {
					b.move_to(x as f32, y as f32);
					bits.extend([0, x.to_bits(), y.to_bits()]);
				}
				PathCommand::LineTo { x, y } => {
					b.line_to(x as f32, y as f32);
					bits.extend([1, x.to_bits(), y.to_bits()]);
				}
				PathCommand::QuadTo { x1, y1, x, y } => {
					b.quad_to(x1 as f32, y1 as f32, x as f32, y as f32);
					bits.extend([
						2,
						x1.to_bits(),
						y1.to_bits(),
						x.to_bits(),
						y.to_bits(),
					]);
				}
				PathCommand::CubicTo {
					x1,
					y1,
					x2,
					y2,
					x,
					y,
				} => {
					b.cubic_to(
						x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32,
						y as f32,
					);
					bits.extend([
						3,
						x1.to_bits(),
						y1.to_bits(),
						x2.to_bits(),
						y2.to_bits(),
						x.to_bits(),
						y.to_bits(),
					]);
				}
				PathCommand::Close => {
					b.close();
					bits.push(4);
				}
			}
		}
		let Some(path) = b.finish() else {
			return;
		};
		let bounds = path.bounds();
		let rect = Rect {
			x: x + bounds.x() * size - 1.0,
			y: y + bounds.y() * size - 1.0,
			w: bounds.width() * size + 2.0,
			h: bounds.height() * size + 2.0,
		};
		let Some(visible) = intersect(rect, clip) else {
			return;
		};
		let px = ((visible.x - x) * view.scale).floor() as i32;
		let py = ((visible.y - y) * view.scale).floor() as i32;
		let w = (visible.w * view.scale).ceil() as u32 + 1;
		let h = (visible.h * view.scale).ceil() as u32 + 1;
		let scale = size * view.scale;
		// Tile very large paths, limiting temporary raster memory to 1 MiB.
		for ty in (0..h).step_by(512) {
			for tx in (0..w).step_by(512) {
				let (w, h) = ((w - tx).min(512), (h - ty).min(512));
				let (ox, oy) = (px + tx as i32, py + ty as i32);
				let key = RasterKey::Path {
					path: fingerprint(&bits),
					size: scale.to_bits(),
					tx: ox,
					ty: oy,
					w,
					h,
				};
				let entry = if let Some(e) = self.cache.get(&key) {
					Some(*e)
				} else {
					let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) else {
						continue;
					};
					let transform = tiny_skia::Transform::from_row(
						scale, 0.0, 0.0, scale, -ox as f32, -oy as f32,
					);
					let mut paint = tiny_skia::Paint::default();
					paint.set_color_rgba8(255, 255, 255, 255);
					if fill {
						pixmap.fill_path(
							&path,
							&paint,
							tiny_skia::FillRule::Winding,
							transform,
							None,
						);
					} else {
						pixmap.stroke_path(
							&path,
							&paint,
							&tiny_skia::Stroke {
								width: 0.04,
								..Default::default()
							},
							transform,
							None,
						);
					}
					let mask: Vec<u8> = pixmap
						.data()
						.as_chunks::<4>()
						.0
						.iter()
						.map(|p| p[3])
						.collect();
					self.insert(
						queue,
						key,
						Entry {
							w,
							h,
							..Default::default()
						},
						&mask,
					)
				};
				if let Some(e) = entry {
					geometry.quad(
						Rect {
							x: x + ox as f32 / view.scale,
							y: y + oy as f32 / view.scale,
							w: w as f32 / view.scale,
							h: h as f32 / view.scale,
						},
						Rect {
							x: e.x as f32,
							y: e.y as f32,
							w: w as f32,
							h: h as f32,
						},
						color,
						clip,
						view,
					);
				}
			}
		}
	}
}
