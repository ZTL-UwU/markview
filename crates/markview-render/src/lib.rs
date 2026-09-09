//! Event-driven wgpu renderer. Only visible glyphs and paths are rasterized.
use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use markview_core::{
	document::fingerprint,
	layout::{Draw, Glyph, LayoutSnapshot, Paint, Rect, TextShaper},
};
use parley::FontData;
use ratex_types::{DisplayItem, PathCommand};
use std::{
	collections::HashMap,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};
use swash::{
	FontRef,
	scale::{Render, ScaleContext, Source},
	zeno::{Format, Vector},
};
use winit::window::Window;

#[derive(
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	Eq,
	serde::Serialize,
	serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
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

const ATLAS_SIZE: u32 = 2048;

/// Keep layout advances fractional, but bake horizontal subpixel coverage into
/// the raster itself. Atlas texels must land on physical pixels one-to-one:
/// translating an antialiased bitmap fractionally filters its edges twice.
/// Four phases bound the cache cost and position error to 1/8 physical pixel.
#[derive(Debug)]
struct GlyphOrigin {
	x: f32,
	y: f32,
	phase: u8,
}

impl GlyphOrigin {
	fn new(x: f32, y: f32, scale: f32) -> Self {
		let phased_x = (x * scale * 4.0).round();
		Self {
			x: (phased_x / 4.0).floor(),
			y: (y * scale).round(),
			phase: phased_x.rem_euclid(4.0) as u8,
		}
	}
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
	position: [f32; 2],
	uv: [f32; 2],
	color: [f32; 4],
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum RasterKey {
	Glyph {
		font: u64,
		index: u32,
		id: u16,
		size: u32,
		phase: u8,
		coords: u64,
	},
	Path {
		path: u64,
		size: u32,
		tx: i32,
		ty: i32,
		w: u32,
		h: u32,
	},
}

#[derive(Clone, Copy, Default)]
struct Entry {
	x: u32,
	y: u32,
	w: u32,
	h: u32,
	left: f32,
	top: f32,
}

pub struct View<'a> {
	pub selection: Option<markview_core::text::TextSelection>,
	pub revision: u64,
	pub width: u32,
	pub height: u32,
	pub scale: f32,
	pub scroll: f32,
	pub left: f32,
	pub top: f32,
	pub bottom: f32,
	pub theme: Theme,
	pub horizontal: &'a HashMap<(usize, usize), f32>,
}

impl View<'_> {
	pub fn viewport(&self) -> markview_core::scene::Viewport {
		markview_core::scene::Viewport {
			width: self.width as f32 / self.scale,
			height: self.height as f32 / self.scale,
			left: self.left,
			top: self.top,
			bottom: self.bottom,
			scroll: self.scroll,
		}
	}
}

pub struct Renderer {
	instance: wgpu::Instance,
	surface: Option<wgpu::Surface<'static>>,
	config: Option<wgpu::SurfaceConfiguration>,
	device: wgpu::Device,
	queue: wgpu::Queue,
	format: wgpu::TextureFormat,
	pub adapter_name: String,
	lost: Arc<AtomicBool>,
	pipeline: wgpu::RenderPipeline,
	bind_group: wgpu::BindGroup,
	atlas: wgpu::Texture,
	cache: HashMap<RasterKey, Entry>,
	shelf: (u32, u32, u32),
	scaler: ScaleContext,
	math_fonts: HashMap<String, FontData>,
	fallback: Option<TextShaper>,
	vertices: Vec<Vertex>,
	vertex_buffer: wgpu::Buffer,
	vertex_capacity: usize,
	atlas_full: bool,
}

pub enum FrameStatus {
	Ready(wgpu::SurfaceTexture, bool),
	Retry(Duration),
	Occluded,
}
impl Renderer {
	pub fn acquire(&mut self, window: Arc<Window>) -> Result<FrameStatus> {
		let size = window.inner_size();
		let surface = self.surface.as_ref().context("No window surface")?;
		Ok(match surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(frame) => {
				FrameStatus::Ready(frame, false)
			}
			wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
				FrameStatus::Ready(frame, true)
			}
			wgpu::CurrentSurfaceTexture::Lost => {
				self.surface = Some(self.instance.create_surface(window)?);
				self.resize(size.width, size.height);
				FrameStatus::Retry(Duration::from_millis(16))
			}
			wgpu::CurrentSurfaceTexture::Outdated => {
				self.resize(size.width, size.height);
				FrameStatus::Retry(Duration::from_millis(16))
			}
			wgpu::CurrentSurfaceTexture::Timeout => {
				FrameStatus::Retry(Duration::from_millis(30))
			}
			wgpu::CurrentSurfaceTexture::Occluded => FrameStatus::Occluded,
			wgpu::CurrentSurfaceTexture::Validation => {
				bail!("GPU surface validation failed")
			}
		})
	}
	pub fn on_device_lost(&self, callback: impl Fn() + Send + 'static) {
		let flag = self.lost.clone();
		self.device.set_device_lost_callback(move |reason, _| {
			if reason != wgpu::DeviceLostReason::Destroyed {
				flag.store(true, Ordering::Relaxed);
				callback();
			}
		});
	}

	pub async fn new(window: Option<Arc<Window>>) -> Result<Self> {
		let descriptor = match &window {
			Some(w) => {
				wgpu::InstanceDescriptor::new_with_display_handle_from_env(
					Box::new(w.clone()),
				)
			}
			None => {
				wgpu::InstanceDescriptor::new_without_display_handle_from_env()
			}
		};
		let instance = wgpu::Instance::new(descriptor);
		let surface = window
			.as_ref()
			.map(|w| instance.create_surface(w.clone()))
			.transpose()?;
		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::LowPower,
				compatible_surface: surface.as_ref(),
				force_fallback_adapter: false,
			})
			.await
			.context("No compatible GPU adapter (try WGPU_BACKEND=gl)")?;
		let info = adapter.get_info();
		let adapter_name = format!(
			"{} ({:?}, {:?})",
			info.name, info.backend, info.device_type
		);
		let (device, queue) = adapter
			.request_device(&wgpu::DeviceDescriptor {
				label: Some("Markview"),
				memory_hints: wgpu::MemoryHints::MemoryUsage,
				..Default::default()
			})
			.await?;
		let lost = Arc::new(AtomicBool::new(false));
		let flag = lost.clone();
		device.set_device_lost_callback(move |_, _| {
			flag.store(true, Ordering::Relaxed);
		});
		let config = surface.as_ref().map(|s| {
			let size = window.as_ref().unwrap().inner_size();
			let caps = s.get_capabilities(&adapter);
			let format = caps
				.formats
				.iter()
				.find(|f| f.is_srgb())
				.copied()
				.unwrap_or(caps.formats[0]);
			wgpu::SurfaceConfiguration {
				usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
				format,
				width: size.width.max(1),
				height: size.height.max(1),
				present_mode: wgpu::PresentMode::AutoVsync,
				desired_maximum_frame_latency: 1,
				alpha_mode: caps.alpha_modes[0],
				view_formats: vec![],
			}
		});
		if let (Some(surface), Some(config)) = (&surface, &config) {
			surface.configure(&device, config);
		}
		let format = config
			.as_ref()
			.map_or(wgpu::TextureFormat::Rgba8UnormSrgb, |c| c.format);
		let shader =
			device.create_shader_module(wgpu::ShaderModuleDescriptor {
				label: Some("text and rectangles"),
				source: wgpu::ShaderSource::Wgsl(
					include_str!("shader.wgsl").into(),
				),
			});
		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("Markview flat pipeline"), layout: None,
			vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[wgpu::VertexBufferLayout {
				array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex,
				attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
			}] },
			fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState {
				format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
			})] }),
			primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
		});
		let atlas = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("4 MiB glyph mask atlas"),
			size: wgpu::Extent3d {
				width: ATLAS_SIZE,
				height: ATLAS_SIZE,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::R8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: None,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});
		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: None,
			layout: &pipeline.get_bind_group_layout(0),
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(
						&atlas.create_view(&Default::default()),
					),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
		});
		let vertex_capacity = 65_536;
		let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("visible quads"),
			size: vertex_capacity as u64,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let mut r = Self {
			instance,
			surface,
			config,
			device,
			queue,
			format,
			adapter_name,
			lost,
			pipeline,
			bind_group,
			atlas,
			cache: HashMap::new(),
			shelf: (2, 0, 2),
			scaler: ScaleContext::new(),
			math_fonts: HashMap::new(),
			fallback: None,
			vertices: Vec::new(),
			vertex_buffer,
			vertex_capacity,
			atlas_full: false,
		};
		r.reset_atlas();
		Ok(r)
	}

	pub fn resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		if let (Some(surface), Some(config)) = (&self.surface, &mut self.config)
		{
			config.width = width;
			config.height = height;
			surface.configure(&self.device, config);
		}
	}

	pub fn gpu_bytes(&self) -> u64 {
		(ATLAS_SIZE * ATLAS_SIZE) as u64 + self.vertex_capacity as u64
	}
	pub fn clear_raster_cache(&mut self) {
		self.reset_atlas();
	}

	fn reset_atlas(&mut self) {
		self.cache.clear();
		self.shelf = (2, 0, 2);
		self.atlas_full = false;
		self.upload(0, 0, 1, 1, &[255]);
	}
	fn upload(&self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
		self.queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.atlas,
				mip_level: 0,
				origin: wgpu::Origin3d { x, y, z: 0 },
				aspect: wgpu::TextureAspect::All,
			},
			data,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(w),
				rows_per_image: Some(h),
			},
			wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
		);
	}
	fn insert(
		&mut self,
		key: RasterKey,
		mut entry: Entry,
		data: &[u8],
	) -> Option<Entry> {
		if entry.w == 0 || entry.h == 0 {
			self.cache.insert(key, entry);
			return Some(entry);
		}
		let w = entry.w + 2;
		let h = entry.h + 2;
		if self.shelf.0 + w > ATLAS_SIZE {
			self.shelf.0 = 0;
			self.shelf.1 += self.shelf.2;
			self.shelf.2 = 0;
		}
		if w > ATLAS_SIZE || self.shelf.1 + h > ATLAS_SIZE {
			self.atlas_full = true;
			return None;
		}
		entry.x = self.shelf.0 + 1;
		entry.y = self.shelf.1 + 1;
		// Clear the one-pixel border as well: atlas resets may leave old texels.
		let mut padded = vec![0u8; (w * h) as usize];
		for row in 0..entry.h as usize {
			padded[(row + 1) * w as usize + 1
				..(row + 1) * w as usize + 1 + entry.w as usize]
				.copy_from_slice(
					&data[row * entry.w as usize..(row + 1) * entry.w as usize],
				);
		}
		self.upload(self.shelf.0, self.shelf.1, w, h, &padded);
		self.shelf.0 += w;
		self.shelf.2 = self.shelf.2.max(h);
		self.cache.insert(key, entry);
		Some(entry)
	}

	fn glyph(&mut self, g: &Glyph, scale: f32, phase: u8) -> Option<Entry> {
		let size = (g.size * scale * 4.0).round().max(1.0) as u32;
		let key = RasterKey::Glyph {
			font: g.font.data.id(),
			index: g.font.index,
			id: g.id,
			size,
			phase,
			coords: fingerprint(&g.coords),
		};
		if let Some(entry) = self.cache.get(&key) {
			return Some(*entry);
		}
		let font =
			FontRef::from_index(g.font.data.data(), g.font.index as usize)?;
		let mut scaler = self
			.scaler
			.builder_with_id(font, [g.font.data.id(), g.font.index as u64])
			.size(size as f32 / 4.0)
			.hint(true)
			.normalized_coords(g.coords.iter())
			.build();
		let image = Render::new(&[
			Source::ColorOutline(0),
			Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
			Source::Outline,
		])
		.format(Format::Alpha)
		.offset(Vector::new(phase as f32 / 4.0, 0.0))
		.render(&mut scaler, g.id);
		let Some(image) = image else {
			let e = Entry::default();
			self.cache.insert(key, e);
			return Some(e);
		};
		let data = match image.content {
			swash::scale::image::Content::Mask => image.data,
			swash::scale::image::Content::Color => {
				image.data.chunks_exact(4).map(|p| p[3]).collect()
			}
			swash::scale::image::Content::SubpixelMask => image
				.data
				.chunks_exact(4)
				.map(|p| p[0].max(p[1]).max(p[2]))
				.collect(),
		};
		self.insert(
			key,
			Entry {
				w: image.placement.width,
				h: image.placement.height,
				left: image.placement.left as f32,
				top: -image.placement.top as f32,
				..Default::default()
			},
			&data,
		)
	}

	fn quad(
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
	fn solid(
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
	fn glyph_quad(
		&mut self,
		g: &Glyph,
		x: f32,
		y: f32,
		color: [f32; 4],
		clip: Rect,
		view: &View<'_>,
	) {
		if g.y + y + g.size < clip.y || g.y + y - g.size * 2.0 > clip.y + clip.h
		{
			return;
		}
		// Wide code/table rows can contain thousands of offscreen glyphs.
		// Keep a conservative overhang margin, but do not rasterize the row
		// outside its local horizontal viewport just to discard its quads.
		if g.x + x + g.size * 4.0 < clip.x
			|| g.x + x - g.size * 4.0 > clip.x + clip.w
		{
			return;
		}
		let origin = GlyphOrigin::new(g.x + x, g.y + y, view.scale);
		if let Some(e) = self.glyph(g, view.scale, origin.phase)
			&& e.w > 0
			&& e.h > 0
		{
			self.quad(
				Rect {
					x: (origin.x + e.left) / view.scale,
					y: (origin.y + e.top) / view.scale,
					w: e.w as f32 / view.scale,
					h: e.h as f32 / view.scale,
				},
				Rect {
					x: e.x as f32,
					y: e.y as f32,
					w: e.w as f32,
					h: e.h as f32,
				},
				color,
				clip,
				view,
			);
		}
	}

	fn math_font(&mut self, name: &str) -> Option<FontData> {
		if let Some(f) = self.math_fonts.get(name) {
			return Some(f.clone());
		}
		let bytes = ratex_katex_fonts::ttf_bytes(&format!("KaTeX_{name}.ttf"))?;
		let font = FontData::new(bytes.into_owned().into(), 0);
		self.math_fonts.insert(name.into(), font.clone());
		Some(font)
	}
	fn math_color(color: ratex_types::Color, theme: Theme) -> [f32; 4] {
		if color.r == 0.0 && color.g == 0.0 && color.b == 0.0 {
			theme.color(Paint::Text)
		} else {
			[color.r, color.g, color.b, color.a]
		}
	}
	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	fn path(
		&mut self,
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
					let mask: Vec<u8> =
						pixmap.data().chunks_exact(4).map(|p| p[3]).collect();
					self.insert(
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
					self.quad(
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

	fn draw(
		&mut self,
		draw: &Draw,
		dx: f32,
		dy: f32,
		clip: Rect,
		view: &View<'_>,
	) {
		match draw {
			Draw::Glyph(g) => self.glyph_quad(
				g,
				dx,
				dy,
				view.theme.color(g.paint),
				clip,
				view,
			),
			Draw::Rect(r, paint) => self.solid(
				Rect {
					x: r.x + dx,
					y: r.y + dy,
					..*r
				},
				view.theme.color(*paint),
				clip,
				view,
			),
			Draw::Math { math, x, y } => {
				let (x, y) = (x + dx, y + dy);
				if intersect(
					Rect {
						x,
						y,
						w: math.width.max(1.0),
						h: math.ascent + math.descent,
					},
					clip,
				)
				.is_none()
				{
					return;
				}
				let size = math.size;
				for item in &math.display.items {
					match item {
						DisplayItem::GlyphPath {
							x: gx,
							y: gy,
							scale,
							font,
							char_code,
							color,
						} => {
							let color = Self::math_color(*color, view.theme);
							if let Some(data) = self.math_font(font) {
								let ch = ratex_font::FontId::parse(font)
									.map_or_else(
										|| {
											char::from_u32(*char_code)
												.unwrap_or('\u{fffd}')
										},
										|id| {
											ratex_font::katex_ttf_glyph_char(
												id, *char_code,
											)
										},
									);
								if let Some(font) =
									FontRef::from_index(data.data.data(), 0)
								{
									let id = font.charmap().map(ch);
									let g = Glyph {
										font: data,
										coords: Arc::from([]),
										id,
										size: size * *scale as f32,
										x: x + *gx as f32 * size,
										y: y + *gy as f32 * size,
										paint: Paint::Text,
									};
									self.glyph_quad(
										&g, 0.0, 0.0, color, clip, view,
									);
								}
							} else {
								let ch = char::from_u32(*char_code)
									.unwrap_or('\u{fffd}')
									.to_string();
								let fallback = self
									.fallback
									.get_or_insert_with(TextShaper::new);
								let glyphs = fallback.label(
									&ch,
									size * *scale as f32,
									x + *gx as f32 * size,
									y + *gy as f32 * size,
									Paint::Text,
								);
								for g in glyphs {
									if let Draw::Glyph(g) = g {
										self.glyph_quad(
											&g, 0.0, 0.0, color, clip, view,
										);
									}
								}
							}
						}
						DisplayItem::Line {
							x: lx,
							y: ly,
							width,
							thickness,
							color,
							dashed,
						} => {
							let rect = Rect {
								x: x + *lx as f32 * size,
								y: y + *ly as f32 * size,
								w: *width as f32 * size,
								h: (*thickness as f32 * size).max(0.6),
							};
							let color = Self::math_color(*color, view.theme);
							if *dashed {
								let mut left = 0.0;
								while left < rect.w {
									self.solid(
										Rect {
											x: rect.x + left,
											w: (rect.w - left).min(size * 0.3),
											..rect
										},
										color,
										clip,
										view,
									);
									left += size * 0.5;
								}
							} else {
								self.solid(rect, color, clip, view);
							}
						}
						DisplayItem::Rect {
							x: rx,
							y: ry,
							width,
							height,
							color,
						} => self.solid(
							Rect {
								x: x + *rx as f32 * size,
								y: y + *ry as f32 * size,
								w: *width as f32 * size,
								h: *height as f32 * size,
							},
							Self::math_color(*color, view.theme),
							clip,
							view,
						),
						DisplayItem::Path {
							x: px,
							y: py,
							commands,
							fill,
							color,
						} => self.path(
							commands,
							*fill,
							x + *px as f32 * size,
							y + *py as f32 * size,
							size,
							Self::math_color(*color, view.theme),
							clip,
							view,
						),
					}
				}
			}
		}
	}

	fn prepare(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		overlay: &[Draw],
	) {
		self.vertices.clear();
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: view.width as f32 / view.scale,
			h: view.height as f32 / view.scale,
		};
		let clip = view.viewport().clip();
		let start = snapshot
			.blocks
			.partition_point(|b| b.y + b.layout.height < view.scroll);
		for (index, block) in snapshot.blocks.iter().enumerate().skip(start) {
			let dy = view.top + block.y - view.scroll;
			if dy > clip.y + clip.h {
				break;
			}
			for (i, draw) in block.layout.draws.iter().enumerate() {
				let (offset, local_clip) =
					block.layout.command_view(i, index, view.horizontal);
				let clip = if let Some(rect) = local_clip {
					let rect = Rect {
						x: view.left + rect.x,
						y: dy + rect.y,
						..rect
					};
					let Some(clipped) = clip.intersect(rect) else {
						continue;
					};
					clipped
				} else {
					clip
				};
				let dx = view.left - offset;
				self.draw(draw, dx, dy, clip, view);
			}
			for (oi, o) in block.layout.overflow.iter().enumerate() {
				let offset =
					view.horizontal.get(&(index, oi)).copied().unwrap_or(0.0);
				let track = Rect {
					x: view.left + o.rect.x,
					y: dy + o.rect.y + o.rect.h - 2.0,
					w: o.rect.w,
					h: 2.0,
				};
				self.solid(track, view.theme.color(Paint::Border), clip, view);
				self.solid(
					Rect {
						x: track.x + offset / o.content_width * track.w,
						w: track.w * track.w / o.content_width,
						..track
					},
					view.theme.color(Paint::Muted),
					clip,
					view,
				);
			}
		}
		if let Some(selection) = view.selection {
			let mut color = view.theme.color(Paint::Accent);
			color[3] = 0.28;
			for rect in snapshot.selection_rects_in(
				selection,
				view.horizontal,
				view.revision,
				view.scroll..view.scroll + clip.h,
			) {
				self.solid(
					view.viewport().window_rect(rect),
					color,
					clip,
					view,
				);
			}
		}
		for draw in overlay {
			self.draw(draw, 0.0, 0.0, full, view);
		}
	}

	pub fn render(
		&mut self,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		overlay: &[Draw],
		target: &wgpu::TextureView,
	) -> Result<wgpu::SubmissionIndex> {
		self.prepare(snapshot, view, overlay);
		if self.atlas_full {
			// Evict previous frames, then rebuild the entire current frame; never reuse stale UVs.
			self.reset_atlas();
			self.prepare(snapshot, view, overlay);
			if self.atlas_full {
				bail!(
					"Visible content exceeds the 4 MiB glyph atlas; reduce zoom"
				);
			}
		}
		let bytes = bytemuck::cast_slice(&self.vertices);
		if bytes.len() > self.vertex_capacity {
			self.vertex_capacity = bytes.len().next_power_of_two();
			self.vertex_buffer =
				self.device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("visible quads"),
					size: self.vertex_capacity as u64,
					usage: wgpu::BufferUsages::VERTEX
						| wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
		}
		if !bytes.is_empty() {
			self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
		}
		let mut encoder =
			self.device.create_command_encoder(&Default::default());
		let c = view.theme.color(Paint::Background);
		let linear = |v: f32| {
			if v <= 0.04045 {
				v as f64 / 12.92
			} else {
				((v as f64 + 0.055) / 1.055).powf(2.4)
			}
		};
		{
			let mut pass =
				encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("readable frame"),
					color_attachments: &[Some(
						wgpu::RenderPassColorAttachment {
							view: target,
							depth_slice: None,
							resolve_target: None,
							ops: wgpu::Operations {
								load: wgpu::LoadOp::Clear(wgpu::Color {
									r: linear(c[0]),
									g: linear(c[1]),
									b: linear(c[2]),
									a: 1.0,
								}),
								store: wgpu::StoreOp::Store,
							},
						},
					)],
					..Default::default()
				});
			pass.set_pipeline(&self.pipeline);
			pass.set_bind_group(0, &self.bind_group, &[]);
			pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
			pass.draw(0..self.vertices.len() as u32, 0..1);
		}
		Ok(self.queue.submit([encoder.finish()]))
	}

	pub fn wait(&self, index: Option<wgpu::SubmissionIndex>) -> Result<()> {
		self.device.poll(wgpu::PollType::Wait {
			submission_index: index,
			timeout: Some(Duration::from_secs(10)),
		})?;
		Ok(())
	}
	pub fn offscreen(&self, width: u32, height: u32) -> wgpu::Texture {
		self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("headless validation"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: self.format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT
				| wgpu::TextureUsages::COPY_SRC,
			view_formats: &[],
		})
	}
	pub fn save_png(
		&self,
		texture: &wgpu::Texture,
		path: &std::path::Path,
	) -> Result<()> {
		let w = texture.width();
		let h = texture.height();
		let pitch = (w * 4).div_ceil(256) * 256;
		let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("screenshot readback"),
			size: (pitch * h) as u64,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});
		let mut encoder =
			self.device.create_command_encoder(&Default::default());
		encoder.copy_texture_to_buffer(
			texture.as_image_copy(),
			wgpu::TexelCopyBufferInfo {
				buffer: &buffer,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(pitch),
					rows_per_image: Some(h),
				},
			},
			texture.size(),
		);
		self.queue.submit([encoder.finish()]);
		let (tx, rx) = std::sync::mpsc::channel();
		buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
			let _ = tx.send(r);
		});
		self.wait(None)?;
		rx.recv()??;
		let mapped = buffer.slice(..).get_mapped_range();
		let mut pixmap = tiny_skia::Pixmap::new(w, h)
			.context("Screenshot dimensions too large")?;
		for (src, dst) in mapped
			.chunks_exact(pitch as usize)
			.zip(pixmap.data_mut().chunks_exact_mut(w as usize * 4))
		{
			dst.copy_from_slice(&src[..w as usize * 4]);
		}
		if matches!(
			self.format,
			wgpu::TextureFormat::Bgra8UnormSrgb
				| wgpu::TextureFormat::Bgra8Unorm
		) {
			for p in pixmap.data_mut().chunks_exact_mut(4) {
				p.swap(0, 2);
			}
		}
		pixmap.save_png(path)?;
		drop(mapped);
		buffer.unmap();
		Ok(())
	}
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
	let x = a.x.max(b.x);
	let y = a.y.max(b.y);
	let w = (a.x + a.w).min(b.x + b.w) - x;
	let h = (a.y + a.h).min(b.y + b.h) - y;
	(w > 0.0 && h > 0.0).then_some(Rect { x, y, w, h })
}

#[cfg(test)]
mod tests {
	use super::GlyphOrigin;

	#[test]
	fn glyph_texels_align_at_integer_and_fractional_dpi() {
		for scale in [1.0, 1.25, 1.6, 2.0, 3.0] {
			for n in -2000..2000 {
				let x = n as f32 / 37.0;
				let y = n as f32 / 29.0;
				let origin = GlyphOrigin::new(x, y, scale);
				assert_eq!(origin.x.fract(), 0.0);
				assert_eq!(origin.y.fract(), 0.0);
				assert!(origin.phase < 4);
				let raster_x = origin.x + origin.phase as f32 / 4.0;
				assert!((raster_x - x * scale).abs() <= 0.12501);
				assert!((origin.y - y * scale).abs() <= 0.50001);
			}
		}
	}

	#[test]
	fn subpixel_phase_carries_across_pixel_and_zero_boundaries() {
		for (x, expected_x, expected_phase) in [
			(0.99, 1.0, 0),
			(0.74, 0.0, 3),
			(-0.26, -1.0, 3),
			(-0.01, 0.0, 0),
			(-1.01, -1.0, 0),
		] {
			let origin = GlyphOrigin::new(x, 0.0, 1.0);
			assert_eq!(origin.x, expected_x);
			assert_eq!(origin.phase, expected_phase);
		}
	}

	#[test]
	fn whole_pixel_translation_reuses_raster_phase() {
		for phase in 0..4 {
			let x = phase as f32 / 4.0;
			for shift in -10..10 {
				let origin = GlyphOrigin::new(x + shift as f32, 0.0, 1.0);
				assert_eq!(origin.phase, phase);
				assert_eq!(origin.x, shift as f32);
			}
		}
	}
}
