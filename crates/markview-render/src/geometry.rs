//! Clipped quads and reusable frame vertex storage.
use crate::{View, intersect, raster::ATLAS_SIZE};
use bytemuck::{Pod, Zeroable};
use markview_core::scene::Rect;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
	position: [f32; 2],
	uv: [f32; 2],
	color: [f32; 4],
}

pub(super) struct Geometry {
	vertices: Vec<Vertex>,
	vertex_buffer: wgpu::Buffer,
	vertex_capacity: usize,
}
impl Geometry {
	pub(super) fn clear(&mut self) {
		self.vertices.clear();
	}
	pub(super) fn len(&self) -> u32 {
		self.vertices.len() as u32
	}
	pub(super) fn capacity_bytes(&self) -> u64 {
		self.vertex_capacity as u64
	}
	pub(super) fn buffer(&self) -> &wgpu::Buffer {
		&self.vertex_buffer
	}
	pub(super) fn upload(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
	) {
		let bytes = bytemuck::cast_slice(&self.vertices);
		if bytes.len() > self.vertex_capacity {
			self.vertex_capacity = bytes.len().next_power_of_two();
			self.vertex_buffer =
				device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("visible quads"),
					size: self.vertex_capacity as u64,
					usage: wgpu::BufferUsages::VERTEX
						| wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
		}
		if !bytes.is_empty() {
			queue.write_buffer(&self.vertex_buffer, 0, bytes);
		}
	}
	pub(super) fn new(device: &wgpu::Device) -> Self {
		let vertex_capacity = 65_536;
		let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("visible quads"),
			size: vertex_capacity as u64,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		Self {
			vertices: Vec::new(),
			vertex_buffer,
			vertex_capacity,
		}
	}
	pub(super) fn rounded(
		&mut self,
		r: Rect,
		radius: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if color[3] == 0.
			|| r.w <= 0.
			|| r.h <= 0.
			|| r.intersect(clip).is_none()
		{
			return;
		}
		let radius = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
		if radius < 0.5 {
			self.solid(r, color, clip, view);
			return;
		}
		self.solid(
			Rect {
				y: r.y + radius,
				h: (r.h - 2. * radius).max(0.),
				..r
			},
			color,
			clip,
			view,
		);
		let steps = (radius * view.scale).ceil().max(1.) as usize;
		for i in 0..steps {
			let y = i as f32 * radius / steps as f32;
			let h = radius / steps as f32;
			let d = radius - y - h / 2.;
			let inset = radius - (radius * radius - d * d).max(0.).sqrt();
			for top in [r.y + y, r.y + r.h - y - h] {
				self.solid(
					Rect {
						x: r.x + inset,
						y: top,
						w: (r.w - 2. * inset).max(0.),
						h,
					},
					color,
					clip,
					view,
				);
			}
		}
	}
	pub(super) fn rounded_border(
		&mut self,
		r: Rect,
		radius: f32,
		border: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		let b = border.min(r.w / 2.).min(r.h / 2.).max(0.);
		let radius = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
		let step = 1. / view.scale;
		let start = ((clip.y - r.y).max(0.) / step).floor() as usize;
		let rows =
			((clip.y + clip.h - r.y).min(r.h).max(0.) / step).ceil() as usize;
		for i in start..rows {
			let y = i as f32 * step;
			let h = step.min(r.h - y);
			let cy = y + h / 2.;
			let edge = |rad: f32, dy: f32| {
				if dy >= rad {
					0.
				} else {
					rad - (rad * rad - (rad - dy).powi(2)).max(0.).sqrt()
				}
			};
			let inset = edge(radius, cy.min(r.h - cy));
			if cy < b || cy >= r.h - b {
				self.solid(
					Rect {
						x: r.x + inset,
						y: r.y + y,
						w: (r.w - 2. * inset).max(0.),
						h,
					},
					color,
					clip,
					view,
				);
			} else {
				let inner =
					b + edge((radius - b).max(0.), (cy - b).min(r.h - b - cy));
				for x in [r.x + inset, r.x + r.w - inner] {
					self.solid(
						Rect {
							x,
							y: r.y + y,
							w: (inner - inset).max(0.),
							h,
						},
						color,
						clip,
						view,
					);
				}
			}
		}
	}
	pub(super) fn quad(
		&mut self,
		rect: Rect,
		uv: Rect,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		let Some(r) = intersect(rect, clip) else {
			return;
		};
		let uv = Rect {
			x: uv.x + (r.x - rect.x) / rect.w * uv.w,
			y: uv.y + (r.y - rect.y) / rect.h * uv.h,
			w: uv.w * r.w / rect.w,
			h: uv.h * r.h / rect.h,
		};
		let x0 = r.x * view.scale / view.width as f32 * 2.0 - 1.0;
		let x1 = (r.x + r.w) * view.scale / view.width as f32 * 2.0 - 1.0;
		let y0 = 1.0 - r.y * view.scale / view.height as f32 * 2.0;
		let y1 = 1.0 - (r.y + r.h) * view.scale / view.height as f32 * 2.0;
		let v = |x, y, u, v| Vertex {
			position: [x, y],
			uv: [u / ATLAS_SIZE as f32, v / ATLAS_SIZE as f32],
			color,
		};
		let a = v(x0, y0, uv.x, uv.y);
		let b = v(x1, y0, uv.x + uv.w, uv.y);
		let c = v(x1, y1, uv.x + uv.w, uv.y + uv.h);
		let d = v(x0, y1, uv.x, uv.y + uv.h);
		self.vertices.extend_from_slice(&[a, b, c, a, c, d]);
	}
	pub(super) fn solid(
		&mut self,
		rect: Rect,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		self.quad(
			rect,
			Rect {
				x: 0.5,
				y: 0.5,
				w: 0.0,
				h: 0.0,
			},
			color,
			clip,
			view,
		);
	}
}
