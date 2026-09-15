//! Versioned image textures, GPU budget and per-frame demand publication.
use crate::gpu::Gpu;
use markview_core::{
	image::{ImageDemand, ImageSnapshot},
	scene::Rect,
};
use std::{collections::HashMap, ops::Range};
pub(super) type ImageKey = (String, u64);
pub(super) struct ImageTextures {
	pub(super) pipeline: wgpu::RenderPipeline,
	cache: HashMap<ImageKey, (wgpu::BindGroup, u64)>,
	// None selects the shared color glyph atlas, in document paint order.
	runs: Vec<(Range<u32>, Option<ImageKey>)>,
	demand: HashMap<String, ImageDemand>,
	images: ImageSnapshot,
}
impl ImageTextures {
	pub(super) fn new(pipeline: wgpu::RenderPipeline) -> Self {
		Self {
			pipeline,
			cache: HashMap::new(),
			runs: Vec::new(),
			demand: HashMap::new(),
			images: Default::default(),
		}
	}
	pub(super) fn begin(&mut self, snapshot: &ImageSnapshot) {
		self.runs.clear();
		self.images = snapshot.clone();
		self.cache.retain(|(src, version), _| {
			snapshot
				.entries
				.get(src)
				.is_some_and(|i| i.version == *version && i.error.is_none())
		});
		self.demand.clear();
	}
	pub(super) fn publish(&self) {
		self.images
			.pixels
			.demand
			.lock()
			.unwrap()
			.clone_from(&self.demand);
	}
	pub(super) fn bytes(&self) -> u64 {
		self.cache.values().map(|(_, bytes)| bytes).sum()
	}
	pub(super) fn runs(&self) -> &[(Range<u32>, Option<ImageKey>)] {
		&self.runs
	}
	pub(super) fn bind_group(&self, key: &ImageKey) -> &wgpu::BindGroup {
		&self.cache[key].0
	}
	pub(super) fn record(&mut self, range: Range<u32>, key: ImageKey) {
		self.runs.push((range, Some(key)));
	}
	pub(super) fn record_color_glyph(&mut self, range: Range<u32>) {
		if let Some((previous, None)) = self.runs.last_mut()
			&& previous.end == range.start
		{
			previous.end = range.end;
			return;
		}
		self.runs.push((range, None));
	}
	pub(super) fn prepare(
		&mut self,
		src: &String,
		version: u64,
		rect: Rect,
		scale: f32,
		gpu: &Gpu,
	) -> Option<ImageKey> {
		let key = (src.clone(), version);
		let demand = markview_core::image::ImageDemand {
			size: (
				(rect.w * scale).ceil().clamp(1., 4000.) as u32,
				(rect.h * scale).ceil().clamp(1., 4000.) as u32,
			),
			needs_pixels: !self.cache.contains_key(&key),
		};
		self.demand
			.entry(src.clone())
			.and_modify(|d| d.merge(demand))
			.or_insert(demand);
		if !self.cache.contains_key(&key) {
			let pixels =
				self.images.pixels.decoded.lock().unwrap().get(src).cloned();
			let pixels = pixels?;
			if pixels.width > gpu.device.limits().max_texture_dimension_2d
				|| pixels.height > gpu.device.limits().max_texture_dimension_2d
			{
				return None;
			}
			let bytes = pixels.rgba.len() as u64;
			if self.cache.values().map(|(_, b)| b).sum::<u64>() + bytes
				> 256 * 1024 * 1024
			{
				self.cache.retain(|key, _| {
					self.runs.iter().any(|(_, used)| used.as_ref() == Some(key))
				});
			}
			if self.cache.values().map(|(_, b)| b).sum::<u64>() + bytes
				> 256 * 1024 * 1024
			{
				return None;
			}
			let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
				label: Some("document image"),
				size: wgpu::Extent3d {
					width: pixels.width,
					height: pixels.height,
					depth_or_array_layers: 1,
				},
				mip_level_count: 1,
				sample_count: 1,
				dimension: wgpu::TextureDimension::D2,
				format: wgpu::TextureFormat::Rgba8UnormSrgb,
				usage: wgpu::TextureUsages::TEXTURE_BINDING
					| wgpu::TextureUsages::COPY_DST,
				view_formats: &[],
			});
			gpu.queue.write_texture(
				texture.as_image_copy(),
				&pixels.rgba,
				wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(pixels.width * 4),
					rows_per_image: Some(pixels.height),
				},
				texture.size(),
			);
			let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
				mag_filter: wgpu::FilterMode::Linear,
				min_filter: wgpu::FilterMode::Linear,
				..Default::default()
			});
			let group =
				gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
					label: Some("document image"),
					layout: &self.pipeline.get_bind_group_layout(0),
					entries: &[
						wgpu::BindGroupEntry {
							binding: 0,
							resource: wgpu::BindingResource::TextureView(
								&texture.create_view(&Default::default()),
							),
						},
						wgpu::BindGroupEntry {
							binding: 1,
							resource: wgpu::BindingResource::Sampler(&sampler),
						},
					],
				});
			self.cache.insert(key.clone(), (group, bytes));
		}
		self.demand.get_mut(src).unwrap().needs_pixels = false;
		Some(key)
	}
}
