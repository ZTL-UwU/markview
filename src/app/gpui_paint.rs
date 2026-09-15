//! Paint a layout snapshot through GPUI primitives.
use crate::render::{Theme, View};
use gpui::{
	Bounds, ContentMask, Corners, Edges, Hitbox, PathBuilder, Pixels,
	RenderImage, Rgba, Window, fill, point, px, quad, size, transparent_black,
};
use image::{Frame, RgbaImage};
use markview_core::{
	document::fingerprint,
	scene::{Draw, Glyph, LayoutSnapshot, Paint, Rect},
	shaping::TextShaper,
	style::{ColorField as C, Condition},
};
use parley::FontData;
use ratex_types::{DisplayItem, PathCommand};
use std::{collections::HashMap, sync::Arc};
use swash::{
	FontRef,
	scale::{Render, ScaleContext, Source, image::Content},
	zeno::{Command, Format, PathData, Vector},
};

pub(super) struct GpuiPainter {
	scaler: ScaleContext,
	outlines: HashMap<OutlineKey, Option<swash::scale::outline::Outline>>,
	bitmaps: HashMap<BitmapKey, Option<CachedBitmap>>,
	images: HashMap<(String, u64), Arc<RenderImage>>,
	math_fonts: HashMap<String, FontData>,
	fallback: Option<TextShaper>,
	stylesheet: Option<Arc<markview_core::style::Stylesheet>>,
	demand: HashMap<String, markview_core::image::ImageDemand>,
	bytes: u64,
	pointer: Option<(f32, f32)>,
	frame_images: markview_core::image::ImageSnapshot,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct OutlineKey {
	font: u64,
	index: u32,
	id: u16,
	size: u32,
	coords: u64,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct BitmapKey {
	font: u64,
	index: u32,
	id: u16,
	size: u32,
	phase: u8,
	coords: u64,
}

struct CachedBitmap {
	image: Arc<RenderImage>,
	left: f32,
	top: f32,
	w: f32,
	h: f32,
}

impl GpuiPainter {
	pub(super) fn new() -> Self {
		Self {
			scaler: ScaleContext::new(),
			outlines: HashMap::new(),
			bitmaps: HashMap::new(),
			images: HashMap::new(),
			math_fonts: HashMap::new(),
			fallback: None,
			stylesheet: None,
			demand: HashMap::new(),
			bytes: 0,
			pointer: None,
			frame_images: Default::default(),
		}
	}

	pub(super) fn set_pointer(&mut self, pointer: Option<(f32, f32)>) {
		self.pointer = pointer;
	}

	pub(super) fn set_stylesheet(
		&mut self,
		style: Arc<markview_core::style::Stylesheet>,
	) {
		self.stylesheet = Some(style);
	}

	pub(super) fn clear_glyphs(&mut self) {
		self.outlines.clear();
		self.bitmaps.clear();
	}

	#[allow(dead_code)]
	pub(super) fn gpu_bytes(&self) -> u64 {
		self.bytes
	}

	fn color(&self, paint: Paint, theme: Theme) -> Rgba {
		let c = self
			.stylesheet
			.as_ref()
			.map_or_else(|| theme.color(paint), |s| s.paint(paint));
		Rgba {
			r: c[0],
			g: c[1],
			b: c[2],
			a: c[3],
		}
	}

	fn hover_paint(paint: Paint) -> Paint {
		use markview_core::style::chain_push;
		match paint {
			Paint::Styled(Condition::Link, field) => {
				Paint::Styled(Condition::Hover, field)
			}
			Paint::Cascade(chain, field) => {
				Paint::Cascade(chain_push(chain, Condition::Hover), field)
			}
			paint => paint,
		}
	}

	fn math_color(
		&self,
		color: ratex_types::Color,
		theme: Theme,
		paint: Paint,
	) -> Rgba {
		if color.r == 0.0 && color.g == 0.0 && color.b == 0.0 {
			self.color(paint, theme)
		} else {
			Rgba {
				r: color.r,
				g: color.g,
				b: color.b,
				a: color.a,
			}
		}
	}

	fn bounds(rect: Rect) -> Bounds<Pixels> {
		Bounds {
			origin: point(px(rect.x), px(rect.y)),
			size: size(px(rect.w.max(0.0)), px(rect.h.max(0.0))),
		}
	}

	fn mask(clip: Rect) -> ContentMask<Pixels> {
		ContentMask {
			bounds: Self::bounds(clip),
		}
	}

	fn fill_rect(window: &mut Window, rect: Rect, color: Rgba, clip: Rect) {
		let Some(rect) = rect.intersect(clip) else {
			return;
		};
		window.paint_quad(fill(Self::bounds(rect), color));
	}

	pub(super) fn paint(
		&mut self,
		window: &mut Window,
		snapshot: &LayoutSnapshot,
		view: &View<'_>,
		overlay: &[Draw],
		hitbox: &Hitbox,
		cursor: gpui::CursorStyle,
	) {
		self.frame_images = snapshot.images.clone();
		self.demand.clear();
		self.images.retain(|(src, version), image| {
			let keep =
				snapshot.images.entries.get(src).is_some_and(|i| {
					i.version == *version && i.error.is_none()
				});
			if !keep {
				self.bytes = self.bytes.saturating_sub(
					u64::from(image.size(0).width)
						* u64::from(image.size(0).height)
						* 4,
				);
			}
			keep
		});
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: view.width as f32 / view.scale,
			h: view.height as f32 / view.scale,
		};
		window.paint_quad(fill(
			Self::bounds(full),
			self.color(Paint::Background, view.theme),
		));
		let clip = view.viewport().clip();
		let metrics = self.stylesheet.as_ref().map_or_else(
			|| markview_core::scene::ScrollbarMetrics::OVERFLOW,
			|s| s.overflow_scrollbar_metrics(),
		);
		if let Some(background) = &snapshot.document_box {
			self.draw(
				window,
				background,
				view.left,
				view.top - view.scroll,
				clip,
				view,
				false,
			);
		}
		let start = snapshot
			.blocks
			.partition_point(|b| b.y + b.layout.height < view.scroll);
		let mut backgrounds = Vec::new();
		let mut foreground = Vec::new();
		let mut tracks = Vec::new();
		for (index, block) in snapshot.blocks.iter().enumerate().skip(start) {
			let dy = view.top + block.y - view.scroll;
			if dy > clip.y + clip.h {
				break;
			}
			for (i, draw) in block.layout.draws.iter().enumerate() {
				let hovered = view.hovered_link.is_some_and(|url| {
					block.layout.links.iter().enumerate().any(|(n, link)| {
						link.url == url
							&& link.command <= i && block
							.layout
							.links
							.get(n + 1)
							.map_or(i < block.layout.draws.len(), |next| {
								i < next.command
							})
					})
				});
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
				match draw {
					Draw::Rect(
						_,
						Paint::Text
						| Paint::Cascade(_, C::Color)
						| Paint::Styled(_, C::Color),
					)
					| Draw::Clipped { .. }
					| Draw::Glyph(_)
					| Draw::Image { .. }
					| Draw::Math { .. } => foreground.push((draw, dx, dy, clip, hovered)),
					Draw::Rect(..) | Draw::Box { .. } => {
						backgrounds.push((draw, dx, dy, clip, hovered))
					}
				}
			}
			for (oi, o) in block.layout.overflow.iter().enumerate() {
				let offset =
					view.horizontal.get(&(index, oi)).copied().unwrap_or(0.0);
				let band = Rect {
					x: view.left + o.rect.x,
					y: dy + o.rect.y + o.rect.h,
					w: o.rect.w,
					h: metrics.overflow_band(o.gutter),
				};
				let Some(bar) = markview_core::scene::Scrollbar::horizontal(
					band,
					offset,
					o.content_width,
					o.rect.w,
					metrics,
				) else {
					continue;
				};
				let held = view.held_overflow == Some((index, oi));
				let hovered =
					held || view.hovered_overflow == Some((index, oi));
				let on_thumb = held
					|| self.pointer.is_some_and(|(x, y)| bar.on_thumb(x, y));
				let (track, thumb) = bar.bars(hovered);
				tracks.push((
					track,
					self.color(
						Paint::Styled(Condition::Scrollbar, C::Track),
						view.theme,
					),
				));
				tracks.push((
					thumb,
					self.color(
						Paint::Styled(
							Condition::Scrollbar,
							if on_thumb { C::ThumbHover } else { C::Thumb },
						),
						view.theme,
					),
				));
			}
		}
		for (draw, dx, dy, clip, hovered) in backgrounds {
			self.draw(window, draw, dx, dy, clip, view, hovered);
		}
		if let Some(selection) = view.selection {
			let color = self.color(
				Paint::Styled(Condition::Selection, C::Background),
				view.theme,
			);
			for rect in snapshot.selection_rects_in(
				selection,
				view.horizontal,
				view.revision,
				view.scroll..view.scroll + clip.h,
			) {
				Self::fill_rect(
					window,
					view.viewport().window_rect(rect),
					color,
					clip,
				);
			}
		}
		for (draw, dx, dy, clip, hovered) in foreground {
			self.draw(window, draw, dx, dy, clip, view, hovered);
		}
		for (rect, color) in tracks {
			Self::fill_rect(window, rect, color, clip);
		}
		for draw in overlay {
			self.draw(window, draw, 0.0, 0.0, full, view, false);
		}
		snapshot
			.images
			.pixels
			.demand
			.lock()
			.unwrap()
			.clone_from(&self.demand);
		window.set_cursor_style(cursor, hitbox);
	}

	#[expect(clippy::too_many_arguments, reason = "Draw command paint state")]
	fn draw(
		&mut self,
		window: &mut Window,
		draw: &Draw,
		dx: f32,
		dy: f32,
		clip: Rect,
		view: &View<'_>,
		hovered: bool,
	) {
		match draw {
			Draw::Clipped { rect, draws } => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				if let Some(clip) = clip.intersect(rect) {
					window.with_content_mask(
						Some(Self::mask(clip)),
						|window| {
							for draw in draws {
								self.draw(
									window, draw, dx, dy, clip, view, hovered,
								);
							}
						},
					);
				}
			}
			Draw::Rect(r, paint) => {
				let color = self.color(
					if hovered {
						Self::hover_paint(*paint)
					} else {
						*paint
					},
					view.theme,
				);
				Self::fill_rect(
					window,
					Rect {
						x: r.x + dx,
						y: r.y + dy,
						..*r
					},
					color,
					clip,
				);
			}
			Draw::Box {
				rect,
				chain,
				condition,
				radius,
				border,
				left_only,
			} => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				let Some(visible) = rect.intersect(clip) else {
					return;
				};
				let background = self.color(
					Paint::Scoped(*chain, *condition, C::Background),
					view.theme,
				);
				window.with_content_mask(Some(Self::mask(clip)), |window| {
					window.paint_quad(quad(
						Self::bounds(rect),
						Corners::all(px(*radius)),
						background,
						Edges::all(px(0.0)),
						transparent_black(),
						gpui::BorderStyle::default(),
					));
					if *border > 0.0 {
						let color = self.color(
							Paint::Scoped(*chain, *condition, C::BorderColor),
							view.theme,
						);
						if *left_only {
							Self::fill_rect(
								window,
								Rect { w: *border, ..rect },
								color,
								visible,
							);
						} else {
							window.paint_quad(quad(
								Self::bounds(rect),
								Corners::all(px(*radius)),
								Rgba {
									a: 0.0,
									..background
								},
								Edges::all(px(*border)),
								color,
								gpui::BorderStyle::default(),
							));
						}
					}
				});
			}
			Draw::Glyph(g) => self.glyph(window, g, dx, dy, clip, view),
			Draw::Image {
				src, version, rect, ..
			} => {
				let rect = Rect {
					x: rect.x + dx,
					y: rect.y + dy,
					..*rect
				};
				if rect.intersect(clip).is_none() {
					return;
				}
				self.image(window, src, *version, rect, clip, view);
			}
			Draw::Math { math, x, y, paint } => {
				self.math(window, math, *x + dx, *y + dy, *paint, clip, view);
			}
		}
	}

	fn glyph(
		&mut self,
		window: &mut Window,
		g: &Glyph,
		dx: f32,
		dy: f32,
		clip: Rect,
		view: &View<'_>,
	) {
		let x = g.x + dx;
		let y = g.y + dy;
		if y + g.size < clip.y || y - g.size * 2.0 > clip.y + clip.h {
			return;
		}
		if x + g.size * 4.0 < clip.x || x - g.size * 4.0 > clip.x + clip.w {
			return;
		}
		let color = self.color(g.paint, view.theme);
		let size = (g.size * view.scale * 4.0).round().max(1.0) as u32;
		let outline_key = OutlineKey {
			font: g.font.data.id(),
			index: g.font.index,
			id: g.id,
			size,
			coords: fingerprint(&g.coords),
		};
		if !self.outlines.contains_key(&outline_key) {
			let outline =
				FontRef::from_index(g.font.data.data(), g.font.index as usize)
					.and_then(|font| {
						let mut scaler = self
							.scaler
							.builder_with_id(
								font,
								[g.font.data.id(), g.font.index as u64],
							)
							.size(size as f32 / 4.0)
							.hint(true)
							.normalized_coords(g.coords.iter())
							.build();
						let outline = scaler.scale_outline(g.id)?;
						(!outline.is_color()).then_some(outline)
					});
			self.outlines.insert(outline_key, outline);
		}
		if let Some(Some(outline)) = self.outlines.get(&outline_key) {
			let mut builder = PathBuilder::fill();
			let origin_x = x;
			let origin_y = y;
			for command in outline.path().commands() {
				match command {
					Command::MoveTo(p) => {
						builder.move_to(point(
							px(origin_x + p.x),
							px(origin_y - p.y),
						));
					}
					Command::LineTo(p) => {
						builder.line_to(point(
							px(origin_x + p.x),
							px(origin_y - p.y),
						));
					}
					Command::QuadTo(c, p) => {
						builder.curve_to(
							point(px(origin_x + p.x), px(origin_y - p.y)),
							point(px(origin_x + c.x), px(origin_y - c.y)),
						);
					}
					Command::CurveTo(c1, c2, p) => {
						builder.cubic_bezier_to(
							point(px(origin_x + p.x), px(origin_y - p.y)),
							point(px(origin_x + c1.x), px(origin_y - c1.y)),
							point(px(origin_x + c2.x), px(origin_y - c2.y)),
						);
					}
					Command::Close => builder.close(),
				}
			}
			if let Ok(path) = builder.build() {
				window.with_content_mask(Some(Self::mask(clip)), |window| {
					window.paint_path(path, color);
				});
				return;
			}
		}
		let origin = glyph_origin(x, y, view.scale);
		let key = BitmapKey {
			font: g.font.data.id(),
			index: g.font.index,
			id: g.id,
			size,
			phase: origin.phase,
			coords: fingerprint(&g.coords),
		};
		if !self.bitmaps.contains_key(&key) {
			let raster = self.raster_glyph(g, view.scale, origin.phase);
			self.bitmaps.insert(key, raster);
		}
		let Some(Some(bitmap)) = self.bitmaps.get(&key) else {
			return;
		};
		let rect = Rect {
			x: (origin.x + bitmap.left) / view.scale,
			y: (origin.y + bitmap.top) / view.scale,
			w: bitmap.w / view.scale,
			h: bitmap.h / view.scale,
		};
		if rect.intersect(clip).is_none() {
			return;
		}
		let image = bitmap.image.clone();
		window.with_content_mask(Some(Self::mask(clip)), |window| {
			let _ = window.paint_image(
				Self::bounds(rect),
				Corners::all(px(0.0)),
				image,
				0,
				false,
			);
		});
	}

	fn raster_glyph(
		&mut self,
		g: &Glyph,
		scale: f32,
		phase: u8,
	) -> Option<CachedBitmap> {
		let font =
			FontRef::from_index(g.font.data.data(), g.font.index as usize)?;
		let size = (g.size * scale * 4.0).round().max(1.0) as u32;
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
		.render(&mut scaler, g.id)?;
		if image.placement.width == 0 || image.placement.height == 0 {
			return None;
		}
		let mut rgba = match image.content {
			Content::Mask => image
				.data
				.iter()
				.flat_map(|&a| [255, 255, 255, a])
				.collect::<Vec<_>>(),
			Content::Color => {
				let mut data = image.data;
				if matches!(image.source, Source::ColorOutline(_)) {
					unpremultiply(&mut data);
				}
				data
			}
			Content::SubpixelMask => image
				.data
				.as_chunks::<4>()
				.0
				.iter()
				.flat_map(|p| {
					let a = p[0].max(p[1]).max(p[2]);
					[255, 255, 255, a]
				})
				.collect(),
		};
		for pixel in rgba.as_chunks_mut::<4>().0 {
			pixel.swap(0, 2);
		}
		let buffer = RgbaImage::from_raw(
			image.placement.width,
			image.placement.height,
			rgba,
		)?;
		let render = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));
		self.bytes += u64::from(image.placement.width)
			* u64::from(image.placement.height)
			* 4;
		Some(CachedBitmap {
			image: render,
			left: image.placement.left as f32,
			top: -image.placement.top as f32,
			w: image.placement.width as f32,
			h: image.placement.height as f32,
		})
	}

	fn image(
		&mut self,
		window: &mut Window,
		src: &String,
		version: u64,
		rect: Rect,
		clip: Rect,
		view: &View<'_>,
	) {
		let demand = markview_core::image::ImageDemand {
			size: (
				(rect.w * view.scale).ceil().clamp(1., 4000.) as u32,
				(rect.h * view.scale).ceil().clamp(1., 4000.) as u32,
			),
			needs_pixels: !self.images.contains_key(&(src.clone(), version)),
		};
		self.demand
			.entry(src.clone())
			.and_modify(|d| d.merge(demand))
			.or_insert(demand);
		if !self.images.contains_key(&(src.clone(), version)) {
			let pixels = self
				.frame_images
				.pixels
				.decoded
				.lock()
				.unwrap()
				.get(src)
				.cloned();
			let Some(pixels) = pixels else {
				return;
			};
			let bytes = pixels.rgba.len() as u64;
			if self.bytes + bytes > 256 * 1024 * 1024 {
				return;
			}
			let mut rgba = pixels.rgba.to_vec();
			for pixel in rgba.as_chunks_mut::<4>().0 {
				pixel.swap(0, 2);
			}
			let Some(buffer) =
				RgbaImage::from_raw(pixels.width, pixels.height, rgba)
			else {
				return;
			};
			self.bytes += bytes;
			self.images.insert(
				(src.clone(), version),
				Arc::new(RenderImage::new(vec![Frame::new(buffer)])),
			);
			self.demand.get_mut(src).unwrap().needs_pixels = false;
		}
		let Some(image) = self.images.get(&(src.clone(), version)).cloned()
		else {
			return;
		};
		window.with_content_mask(Some(Self::mask(clip)), |window| {
			let _ = window.paint_image(
				Self::bounds(rect),
				Corners::all(px(0.0)),
				image,
				0,
				false,
			);
		});
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "Math display-list paint state"
	)]
	fn math(
		&mut self,
		window: &mut Window,
		math: &Arc<markview_core::math::MathBox>,
		x: f32,
		y: f32,
		paint: Paint,
		clip: Rect,
		view: &View<'_>,
	) {
		if (Rect {
			x,
			y,
			w: math.width.max(1.0),
			h: math.ascent + math.descent,
		})
		.intersect(clip)
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
					let color = self.math_color(*color, view.theme, paint);
					if let Some(data) = self.math_font(font)
						&& let Some(face) =
							FontRef::from_index(data.data.data(), 0)
					{
						let ch = ratex_font::FontId::parse(font).map_or_else(
							|| char::from_u32(*char_code).unwrap_or('\u{fffd}'),
							|id| {
								ratex_font::katex_ttf_glyph_char(id, *char_code)
							},
						);
						let id = face.charmap().map(ch);
						let g = Glyph {
							font: data,
							coords: Arc::from([]),
							id,
							size: size * *scale as f32,
							x: x + *gx as f32 * size,
							y: y + *gy as f32 * size,
							paint: Paint::Color(markview_core::style::Color(
								rgba_bits(color),
							)),
						};
						self.glyph(window, &g, 0.0, 0.0, clip, view);
					} else {
						let ch = char::from_u32(*char_code)
							.unwrap_or('\u{fffd}')
							.to_string();
						let fallback =
							self.fallback.get_or_insert_with(TextShaper::new);
						let glyphs = fallback.label(
							&ch,
							size * *scale as f32,
							x + *gx as f32 * size,
							y + *gy as f32 * size,
							Paint::Text,
						);
						for g in glyphs {
							if let Draw::Glyph(mut g) = g {
								g.paint =
									Paint::Color(markview_core::style::Color(
										rgba_bits(color),
									));
								self.glyph(window, &g, 0.0, 0.0, clip, view);
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
					let color = self.math_color(*color, view.theme, paint);
					if *dashed {
						let mut left = 0.0;
						while left < rect.w {
							Self::fill_rect(
								window,
								Rect {
									x: rect.x + left,
									w: (rect.w - left).min(size * 0.3),
									..rect
								},
								color,
								clip,
							);
							left += size * 0.5;
						}
					} else {
						Self::fill_rect(window, rect, color, clip);
					}
				}
				DisplayItem::Rect {
					x: rx,
					y: ry,
					width,
					height,
					color,
				} => Self::fill_rect(
					window,
					Rect {
						x: x + *rx as f32 * size,
						y: y + *ry as f32 * size,
						w: *width as f32 * size,
						h: *height as f32 * size,
					},
					self.math_color(*color, view.theme, paint),
					clip,
				),
				DisplayItem::Path {
					x: px,
					y: py,
					commands,
					fill,
					color,
				} => {
					let color = self.math_color(*color, view.theme, paint);
					self.path(
						window,
						commands,
						*fill,
						x + *px as f32 * size,
						y + *py as f32 * size,
						size,
						color,
						clip,
					);
				}
			}
		}
	}

	#[expect(clippy::too_many_arguments, reason = "Vector path drawing state")]
	fn path(
		&mut self,
		window: &mut Window,
		commands: &[PathCommand],
		fill: bool,
		x: f32,
		y: f32,
		size: f32,
		color: Rgba,
		clip: Rect,
	) {
		let mut builder = if fill {
			PathBuilder::fill()
		} else {
			PathBuilder::stroke(px((size * 0.06).max(0.6)))
		};
		let mut empty = true;
		for command in commands {
			empty = false;
			match *command {
				PathCommand::MoveTo { x: px, y: py } => {
					builder.move_to(point(
						gpui::px(x + px as f32 * size),
						gpui::px(y + py as f32 * size),
					));
				}
				PathCommand::LineTo { x: px, y: py } => {
					builder.line_to(point(
						gpui::px(x + px as f32 * size),
						gpui::px(y + py as f32 * size),
					));
				}
				PathCommand::QuadTo {
					x1,
					y1,
					x: px,
					y: py,
				} => {
					builder.curve_to(
						point(
							gpui::px(x + px as f32 * size),
							gpui::px(y + py as f32 * size),
						),
						point(
							gpui::px(x + x1 as f32 * size),
							gpui::px(y + y1 as f32 * size),
						),
					);
				}
				PathCommand::CubicTo {
					x1,
					y1,
					x2,
					y2,
					x: px,
					y: py,
				} => {
					builder.cubic_bezier_to(
						point(
							gpui::px(x + px as f32 * size),
							gpui::px(y + py as f32 * size),
						),
						point(
							gpui::px(x + x1 as f32 * size),
							gpui::px(y + y1 as f32 * size),
						),
						point(
							gpui::px(x + x2 as f32 * size),
							gpui::px(y + y2 as f32 * size),
						),
					);
				}
				PathCommand::Close => builder.close(),
			}
		}
		if empty {
			return;
		}
		let Ok(path) = builder.build() else {
			return;
		};
		window.with_content_mask(Some(Self::mask(clip)), |window| {
			window.paint_path(path, color);
		});
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
}

fn glyph_origin(x: f32, y: f32, scale: f32) -> GlyphOrigin {
	let phased_x = (x * scale * 4.0).round();
	GlyphOrigin {
		x: (phased_x / 4.0).floor(),
		y: (y * scale).round(),
		phase: phased_x.rem_euclid(4.0) as u8,
	}
}

struct GlyphOrigin {
	x: f32,
	y: f32,
	phase: u8,
}

fn unpremultiply(rgba: &mut [u8]) {
	for pixel in rgba.as_chunks_mut::<4>().0 {
		let a = u32::from(pixel[3]);
		for c in &mut pixel[..3] {
			*c = (u32::from(*c) * 255 + a / 2)
				.checked_div(a)
				.unwrap_or(0)
				.min(255) as u8;
		}
	}
}

fn rgba_bits(color: Rgba) -> u32 {
	let r = (color.r * 255.0).round() as u32;
	let g = (color.g * 255.0).round() as u32;
	let b = (color.b * 255.0).round() as u32;
	let a = (color.a * 255.0).round() as u32;
	(r << 24) | (g << 16) | (b << 8) | a
}
