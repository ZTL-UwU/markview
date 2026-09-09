//! Bounded image loading shared by window and diagnostic entry points.
use anyhow::{Context, Result, bail};
use base64::Engine;
use image::{AnimationDecoder, ImageDecoder};
use markview_core::{
	document::Document,
	image::{ImageInfo, ImageSnapshot, Pixels},
};
use std::{
	collections::{HashMap, HashSet},
	fs,
	io::{Cursor, Read},
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
		mpsc,
	},
	thread,
	time::{Duration, Instant, SystemTime},
};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
const CPU_BUDGET: usize = 256 * 1024 * 1024;
static VERSION: AtomicU64 = AtomicU64::new(1);

fn pixel_bytes(pixels: &HashMap<String, Arc<Pixels>>) -> usize {
	let mut seen = HashSet::new();
	pixels
		.values()
		.filter(|p| seen.insert(Arc::as_ptr(p)))
		.map(|p| p.rgba.len())
		.sum()
}

fn cache_pixels(
	pixels: &mut HashMap<String, Arc<Pixels>>,
	aliases: &[String],
	incoming: Arc<Pixels>,
	demand: &HashMap<String, markview_core::image::ImageDemand>,
	budget: usize,
) {
	for a in aliases {
		pixels.remove(a);
	}
	let mut bytes = pixel_bytes(pixels);
	let mut victims: Vec<_> = pixels.keys().cloned().collect();
	// Evict an entire allocation, not one alias. Prefer offscreen resources.
	let visible: HashSet<_> = demand
		.keys()
		.filter_map(|a| pixels.get(a).map(Arc::as_ptr))
		.collect();
	victims.sort_by_key(|a| {
		(visible.contains(&Arc::as_ptr(&pixels[a])), a.clone())
	});
	for alias in victims {
		if bytes + incoming.rgba.len() <= budget {
			break;
		}
		if let Some(victim) = pixels.get(&alias).cloned() {
			pixels.retain(|_, p| !Arc::ptr_eq(p, &victim));
			bytes -= victim.rgba.len();
		}
	}
	if incoming.rgba.len() <= budget {
		for alias in aliases {
			pixels.insert(alias.clone(), incoming.clone());
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Source {
	File(PathBuf),
	Http(String),
	Data(String),
}

fn source(src: &str, document: &Path, offline: bool) -> Result<Source> {
	if src.is_empty() {
		bail!("Missing image source");
	}
	if Path::new(src).is_absolute() {
		return Ok(Source::File(PathBuf::from(src)));
	}
	if let Ok(url) = url::Url::parse(src) {
		return match url.scheme() {
			"http" | "https" if !offline => Ok(Source::Http(url.to_string())),
			"http" | "https" => {
				anyhow::bail!("Network images disabled (--offline)")
			}
			"file" => Ok(Source::File(
				url.to_file_path()
					.map_err(|_| anyhow::anyhow!("Invalid local file URL"))?,
			)),
			"data" => Ok(Source::Data(src.to_owned())),
			_ => anyhow::bail!("Unsupported image URL scheme"),
		};
	}
	let decoded = percent_encoding::percent_decode_str(src)
		.decode_utf8()
		.context("Invalid path encoding")?;
	let path = document
		.parent()
		.unwrap_or(Path::new("."))
		.join(decoded.as_ref());
	Ok(Source::File(fs::canonicalize(&path).unwrap_or(path)))
}

fn bounded(mut reader: impl Read) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	reader
		.by_ref()
		.take((MAX_BYTES + 1) as u64)
		.read_to_end(&mut bytes)?;
	if bytes.len() > MAX_BYTES {
		bail!("Image exceeds 32 MiB");
	}
	Ok(bytes)
}

fn fetch(
	source: &Source,
	client: &reqwest::blocking::Client,
) -> Result<Vec<u8>> {
	match source {
		Source::File(path) => {
			let file = fs::File::open(path).context("Cannot open image")?;
			if !file.metadata()?.is_file() {
				bail!("Image is not a regular file");
			}
			bounded(file)
		}
		Source::Http(url) => {
			bounded(client.get(url).send()?.error_for_status()?)
		}
		Source::Data(uri) => {
			let (header, data) =
				uri.split_once(',').context("Invalid data URI")?;
			if !header.to_ascii_lowercase().starts_with("data:image/") {
				bail!("Data URI must contain an image");
			}
			if data.len() > MAX_BYTES * 3 {
				bail!("Image exceeds 32 MiB");
			}
			let data =
				percent_encoding::percent_decode_str(data).collect::<Vec<_>>();
			let bytes = if header.to_ascii_lowercase().ends_with(";base64") {
				base64::engine::general_purpose::STANDARD.decode(data)?
			} else {
				data
			};
			if bytes.len() > MAX_BYTES {
				bail!("Image exceeds 32 MiB");
			}
			Ok(bytes)
		}
	}
}

fn dimensions(w: u32, h: u32) -> Result<()> {
	if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
		bail!("Image exceeds 16 million pixels or has invalid dimensions");
	}
	Ok(())
}

struct Decoded {
	pixels: Arc<Pixels>,
	intrinsic: (u32, u32),
	svg: bool,
}

/// The largest PNG-compressed entry of an ICO. Windows renders entries that
/// are not 32-bit RGBA, while `image`'s ICO decoder rejects them.
fn ico_png(bytes: &[u8]) -> Option<&[u8]> {
	const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
	let count = u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]) as usize;
	let mut best: Option<(u32, &[u8])> = None;
	for i in 0..count {
		let entry = bytes.get(6 + i * 16..6 + i * 16 + 16)?;
		let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
		let offset =
			u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
		let data = bytes.get(offset..offset.checked_add(size)?)?;
		if !data.starts_with(&SIGNATURE) {
			continue;
		}
		let side = |v: u8| if v == 0 { 256 } else { u32::from(v) };
		let area = side(entry[0]) * side(entry[1]);
		if best.is_none_or(|(best, _)| area > best) {
			best = Some((area, data));
		}
	}
	best.map(|(_, data)| data)
}

/// System fonts are shared: loading them is expensive and SVGs without text
/// do not need them at all.
fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
	static FONTS: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> =
		std::sync::OnceLock::new();
	FONTS
		.get_or_init(|| {
			let mut db = resvg::usvg::fontdb::Database::new();
			db.load_system_fonts();
			Arc::new(db)
		})
		.clone()
}

fn has_svg_text(bytes: &[u8]) -> bool {
	[b"<text".as_slice(), b"<tspan", b"<textPath"]
		.iter()
		.any(|tag| {
			bytes
				.windows(tag.len())
				.any(|w| w.eq_ignore_ascii_case(tag))
		})
}

fn decode(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Decoded> {
	let format = image::guess_format(bytes).ok();
	if format.is_none() {
		let mut options = resvg::usvg::Options::default();
		options.image_href_resolver.resolve_string = Box::new(|_, _| None);
		options.fontdb = if has_svg_text(bytes) {
			svg_fonts()
		} else {
			Default::default()
		};
		let tree = resvg::usvg::Tree::from_data(bytes, &options)
			.context("Unsupported or invalid image/SVG")?;
		let intrinsic = tree.size().to_int_size();
		dimensions(intrinsic.width(), intrinsic.height())?;
		let (w, h) = target.unwrap_or((intrinsic.width(), intrinsic.height()));
		dimensions(w, h)?;
		let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
			.context("Cannot allocate SVG")?;
		resvg::render(
			&tree,
			resvg::tiny_skia::Transform::from_scale(
				w as f32 / tree.size().width(),
				h as f32 / tree.size().height(),
			),
			&mut pixmap.as_mut(),
		);
		// tiny-skia stores premultiplied alpha; the image pipeline uses straight alpha.
		let mut rgba = pixmap.take();
		for p in rgba.chunks_exact_mut(4) {
			if p[3] > 0 {
				for i in 0..3 {
					p[i] = ((u32::from(p[i]) * 255 + u32::from(p[3]) / 2)
						/ u32::from(p[3]))
					.min(255) as u8;
				}
			}
		}
		return Ok(Decoded {
			pixels: Arc::new(Pixels {
				width: w,
				height: h,
				rgba: rgba.into(),
			}),
			intrinsic: (intrinsic.width(), intrinsic.height()),
			svg: true,
		});
	}
	let format = format.unwrap();
	let mut reader =
		image::ImageReader::with_format(Cursor::new(bytes), format);
	let mut limits = image::Limits::default();
	limits.max_alloc = Some(128 * 1024 * 1024);
	reader.limits(limits.clone());
	let mut decoder = reader.into_decoder()?;
	let (w, h) = decoder.dimensions();
	dimensions(w, h)?;
	let orientation = decoder.orientation()?;
	let mut bitmap = match format {
		image::ImageFormat::Gif => {
			let mut d =
				image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
			d.set_limits(limits)?;
			image::DynamicImage::ImageRgba8(
				d.into_frames().next().context("Empty GIF")??.into_buffer(),
			)
		}
		image::ImageFormat::Png => {
			let d = image::codecs::png::PngDecoder::with_limits(
				Cursor::new(bytes),
				limits,
			)?;
			if d.is_apng()? {
				image::DynamicImage::ImageRgba8(
					d.apng()?
						.into_frames()
						.next()
						.context("Empty APNG")??
						.into_buffer(),
				)
			} else {
				image::DynamicImage::from_decoder(decoder)?
			}
		}
		image::ImageFormat::WebP => {
			let mut d =
				image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
			d.set_limits(limits)?;
			if d.has_animation() {
				image::DynamicImage::ImageRgba8(
					d.into_frames()
						.next()
						.context("Empty WebP")??
						.into_buffer(),
				)
			} else {
				image::DynamicImage::from_decoder(decoder)?
			}
		}
		image::ImageFormat::Ico => match ico_png(bytes) {
			Some(png) => image::DynamicImage::from_decoder(
				image::codecs::png::PngDecoder::with_limits(
					Cursor::new(png),
					limits,
				)?,
			)?,
			None => image::DynamicImage::from_decoder(decoder)?,
		},
		_ => image::DynamicImage::from_decoder(decoder)?,
	};
	bitmap.apply_orientation(orientation);
	let intrinsic = (bitmap.width(), bitmap.height());
	// Stay within the baseline WebGPU 8192-pixel texture dimension.
	if bitmap.width() > 8192 || bitmap.height() > 8192 {
		bitmap =
			bitmap.resize(8192, 8192, image::imageops::FilterType::Lanczos3);
	}
	let rgba = bitmap.into_rgba8();
	Ok(Decoded {
		intrinsic,
		svg: false,
		pixels: Arc::new(Pixels {
			width: rgba.width(),
			height: rgba.height(),
			rgba: rgba.into_raw().into(),
		}),
	})
}

struct Job {
	ticket: u64,
	source: Source,
	generation: u64,
	target: Option<(u32, u32)>,
}
struct Finished {
	ticket: u64,
	source: Source,
	generation: u64,
	result: Result<Decoded>,
}
struct Entry {
	ticket: u64,
	aliases: Vec<String>,
	info: ImageInfo,
	stamp: Option<(u64, Option<SystemTime>)>,
	busy: bool,
	svg: bool,
	raster: Option<(u32, u32)>,
}

pub struct Images {
	pub snapshot: ImageSnapshot,
	entries: HashMap<Source, Entry>,
	send: Option<mpsc::Sender<Job>>,
	recv: mpsc::Receiver<Finished>,
	generation: u64,
	document: PathBuf,
	revision: u64,
	offline: bool,
	poll_at: Instant,
}

fn stamp(source: &Source) -> Option<(u64, Option<SystemTime>)> {
	if let Source::File(path) = source {
		fs::metadata(path)
			.ok()
			.map(|m| (m.len(), m.modified().ok()))
	} else {
		None
	}
}

impl Images {
	pub fn new(offline: bool) -> Self {
		let (tx, rx) = mpsc::channel::<Job>();
		let rx = Arc::new(Mutex::new(rx));
		let (done, recv) = mpsc::channel();
		for i in 0..4 {
			let rx = rx.clone();
			let done = done.clone();
			thread::Builder::new()
				.name(format!("markview-image-{i}"))
				.spawn(move || {
					let client = reqwest::blocking::Client::builder()
						.timeout(Duration::from_secs(15))
						.connect_timeout(Duration::from_secs(5))
						.referer(false)
						.redirect(reqwest::redirect::Policy::custom(
							|attempt| {
								if attempt.previous().len() >= 5 {
									attempt.error("Too many redirects")
								} else if !matches!(
									attempt.url().scheme(),
									"http" | "https"
								) {
									attempt.error("Unsupported redirect scheme")
								} else {
									attempt.follow()
								}
							},
						))
						.build();
					loop {
						let Ok(job) = rx.lock().unwrap().recv() else {
							break;
						};
						// A malformed file must not take the reader down with it.
						let result = std::panic::catch_unwind(
							std::panic::AssertUnwindSafe(|| match &client {
								Ok(client) => fetch(&job.source, client)
									.and_then(|b| decode(&b, job.target)),
								Err(e) => {
									Err(anyhow::anyhow!("Image client: {e}"))
								}
							}),
						)
						.unwrap_or_else(|_| {
							Err(anyhow::anyhow!("Image decoder failed"))
						});
						if done
							.send(Finished {
								ticket: job.ticket,
								source: job.source,
								generation: job.generation,
								result,
							})
							.is_err()
						{
							break;
						}
					}
				})
				.expect("start image loader");
		}
		Self {
			snapshot: Default::default(),
			entries: HashMap::new(),
			send: Some(tx),
			recv,
			generation: 0,
			document: PathBuf::new(),
			revision: 0,
			offline,
			poll_at: Instant::now(),
		}
	}

	pub fn prepare(&mut self, doc: &Document, path: &Path, revision: u64) {
		if self.document != path {
			self.entries.clear();
			self.snapshot = Default::default();
			self.generation += 1;
			self.document = path.into();
		}
		let reload = self.revision != revision;
		self.revision = revision;
		let mut specs = Vec::new();
		for b in &doc.blocks {
			b.images(&mut specs);
		}
		let mut wanted = HashSet::new();
		let retained_pixels: HashMap<_, _> = {
			let pixels = self.snapshot.pixels.decoded.lock().unwrap();
			self.entries
				.iter()
				.filter_map(|(source, e)| {
					e.aliases
						.iter()
						.find_map(|a| pixels.get(a).cloned())
						.map(|p| (source.clone(), p))
				})
				.collect()
		};
		for e in self.entries.values_mut() {
			e.aliases.clear();
		}
		self.snapshot.entries.clear();
		for spec in specs {
			match source(&spec.src, path, self.offline) {
				Ok(source) => {
					wanted.insert(source.clone());
					let e = self.entries.entry(source.clone()).or_insert_with(
						|| Entry {
							ticket: 0,
							aliases: Vec::new(),
							info: Default::default(),
							stamp: stamp(&source),
							busy: false,
							svg: false,
							raster: None,
						},
					);
					if !e.aliases.contains(&spec.src) {
						e.aliases.push(spec.src.clone());
					}
					if reload && e.info.error.is_some() {
						e.info.error = None;
						e.info.size = None;
					}
					self.snapshot
						.entries
						.insert(spec.src.clone(), e.info.clone());
				}
				Err(e) => {
					self.snapshot.entries.insert(
						spec.src.clone(),
						ImageInfo {
							error: Some(e.to_string()),
							..Default::default()
						},
					);
				}
			}
		}
		self.entries.retain(|s, _| wanted.contains(s));
		// A new spelling of a retained source shares its pixels immediately.
		// Remove aliases no longer present so old snapshots cannot pin them.
		{
			let mut pixels = self.snapshot.pixels.decoded.lock().unwrap();
			for (source, e) in &self.entries {
				if let Some(p) = retained_pixels.get(source) {
					for alias in &e.aliases {
						pixels.insert(alias.clone(), p.clone());
					}
				}
			}
			pixels.retain(|alias, _| self.snapshot.entries.contains_key(alias));
		}
		self.schedule();
	}

	fn schedule(&mut self) {
		let demand = self.snapshot.pixels.demand.lock().unwrap().clone();
		let pixels = self.snapshot.pixels.decoded.lock().unwrap();
		let mut running = self.entries.values().filter(|e| e.busy).count();
		let mut keys: Vec<_> = self.entries.keys().cloned().collect();
		keys.sort_by_key(|s| {
			!self.entries[s]
				.aliases
				.iter()
				.any(|a| demand.contains_key(a))
		});
		for s in keys {
			if running >= 4 {
				break;
			}
			let e = self.entries.get_mut(&s).unwrap();
			let requested = e
				.aliases
				.iter()
				.filter_map(|a| demand.get(a).copied())
				.reduce(|mut a, b| {
					a.merge(b);
					a
				});
			let target = requested.map(|d| d.size);
			let resident = e.aliases.iter().any(|a| pixels.contains_key(a));
			let resize = e.svg && target.is_some() && target != e.raster;
			if !e.busy
				&& e.info.error.is_none()
				&& (e.info.size.is_none()
					|| resize || (requested.is_some_and(|d| d.needs_pixels)
					&& !resident))
			{
				e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
				e.busy = true;
				running += 1;
				let _ = self.send.as_ref().unwrap().send(Job {
					ticket: e.ticket,
					source: s,
					generation: self.generation,
					target: if e.svg { target } else { None },
				});
			}
		}
	}

	pub fn poll(&mut self) -> bool {
		let mut changed = false;
		while let Ok(done) = self.recv.try_recv() {
			if done.generation != self.generation {
				continue;
			}
			let Some(e) = self.entries.get_mut(&done.source) else {
				continue;
			};
			if e.ticket != done.ticket {
				continue;
			}
			e.busy = false;
			e.info.version = VERSION.fetch_add(1, Ordering::Relaxed);
			match done.result {
				Ok(decoded) => {
					e.info.size = Some(decoded.intrinsic);
					e.info.error = None;
					e.svg = decoded.svg;
					e.raster =
						Some((decoded.pixels.width, decoded.pixels.height));
					let mut pixels =
						self.snapshot.pixels.decoded.lock().unwrap();
					let demand = self.snapshot.pixels.demand.lock().unwrap();
					cache_pixels(
						&mut pixels,
						&e.aliases,
						decoded.pixels,
						&demand,
						CPU_BUDGET,
					);
				}
				Err(error) => {
					e.info.error = Some(error.to_string());
					self.snapshot
						.pixels
						.decoded
						.lock()
						.unwrap()
						.retain(|s, _| !e.aliases.contains(s));
				}
			}
			for alias in &e.aliases {
				self.snapshot.entries.insert(alias.clone(), e.info.clone());
			}
			changed = true;
		}
		if Instant::now() >= self.poll_at {
			self.poll_at = Instant::now() + Duration::from_millis(500);
			for (s, e) in &mut self.entries {
				let next = stamp(s);
				if next != e.stamp && !e.busy {
					e.stamp = next;
					e.info.error = None;
					e.info.size = None;
					changed = true;
				}
			}
		}
		self.schedule();
		changed
	}

	pub fn wait(&mut self) {
		// A headless frame can have posted new SVG sizes since the last load.
		self.poll();
		while self.entries.values().any(|e| {
			e.busy || (e.info.size.is_none() && e.info.error.is_none())
		}) {
			self.poll();
			thread::sleep(Duration::from_millis(5));
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use image::{Rgb, RgbImage, Rgba, RgbaImage};

	fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
		let mut bytes = Vec::new();
		image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
			width,
			height,
			Rgba(color),
		))
		.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
		.unwrap();
		bytes
	}
	fn rgb_png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
		let mut bytes = Vec::new();
		image::DynamicImage::ImageRgb8(RgbImage::from_pixel(
			width,
			height,
			Rgb(color),
		))
		.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
		.unwrap();
		bytes
	}
	fn data_uri(mime: &str, bytes: &[u8]) -> String {
		format!(
			"data:{mime};base64,{}",
			base64::engine::general_purpose::STANDARD.encode(bytes)
		)
	}

	#[test]
	fn sources_cover_local_network_and_inline_images() {
		let document = Path::new("/docs/note.md");
		let at =
			|src: &str, offline: bool| source(src, document, offline).unwrap();
		let file = |p: &str| Source::File(PathBuf::from(p));
		assert_eq!(at("images/a b.png", false), file("/docs/images/a b.png"));
		assert_eq!(at("a%20b.png", false), file("/docs/a b.png"));
		assert_eq!(at("../up.png", false), file("/docs/../up.png"));
		assert_eq!(at("/tmp/x.png", false), file("/tmp/x.png"));
		assert_eq!(at("file:///tmp/x.png", false), file("/tmp/x.png"));
		assert_eq!(
			at("https://example.com/a.png", false),
			Source::Http("https://example.com/a.png".into())
		);
		assert_eq!(
			at("data:image/png;base64,AA==", false),
			Source::Data("data:image/png;base64,AA==".into())
		);
		assert!(source("", document, false).is_err());
		assert!(source("ftp://example.com/a.png", document, false).is_err());
		assert!(source("https://example.com/a.png", document, true).is_err());
		assert!(source("a%FF.png", document, false).is_err());
	}

	#[test]
	fn data_uris_decode_base64_and_percent_escapes() {
		let client = reqwest::blocking::Client::new();
		let bytes = png(4, 2, [1, 2, 3, 255]);
		let encoded = data_uri("image/png", &bytes);
		assert_eq!(fetch(&Source::Data(encoded), &client).unwrap(), bytes);
		let plain = "data:image/svg+xml,%3Csvg%3E%3C/svg%3E";
		assert_eq!(
			fetch(&Source::Data(plain.into()), &client).unwrap(),
			b"<svg></svg>"
		);
		assert!(
			fetch(&Source::Data("data:text/plain,hello".into()), &client)
				.is_err()
		);
		assert!(
			fetch(&Source::Data("data:image/png;base64,!!".into()), &client)
				.is_err()
		);
	}

	#[test]
	fn bitmap_and_animation_formats_use_their_first_frame() {
		let (w, h) = (12, 8);
		let mut formats =
			vec![png(w, h, [10, 20, 30, 255]), rgb_png(w, h, [10, 20, 30])];
		for format in [image::ImageFormat::Bmp, image::ImageFormat::Jpeg] {
			let mut bytes = Vec::new();
			image::DynamicImage::ImageRgb8(RgbImage::from_pixel(
				w,
				h,
				Rgb([10, 20, 30]),
			))
			.write_to(&mut Cursor::new(&mut bytes), format)
			.unwrap();
			formats.push(bytes);
		}
		// An ICO whose entry is a PNG that is not 32-bit RGBA.
		let payload = rgb_png(w, h, [10, 20, 30]);
		let mut ico = vec![0, 0, 1, 0, 1, 0];
		ico.extend([w as u8, h as u8, 0, 0, 1, 0, 32, 0]);
		ico.extend((payload.len() as u32).to_le_bytes());
		ico.extend(22u32.to_le_bytes());
		ico.extend(&payload);
		formats.push(ico);
		for bytes in formats {
			let decoded = decode(&bytes, None).unwrap();
			assert_eq!(decoded.intrinsic, (w, h));
			assert_eq!(decoded.pixels.width, w);
			assert_eq!(&decoded.pixels.rgba[..4], &[10, 20, 30, 255]);
			assert!(!decoded.svg);
		}
		let mut gif = Vec::new();
		{
			let mut encoder = image::codecs::gif::GifEncoder::new(&mut gif);
			for color in [[1u8, 0, 0, 255], [0, 2, 0, 255]] {
				encoder
					.encode_frame(image::Frame::new(RgbaImage::from_pixel(
						4,
						4,
						Rgba(color),
					)))
					.unwrap();
			}
		}
		let decoded = decode(&gif, None).unwrap();
		assert_eq!(decoded.intrinsic, (4, 4));
		assert_eq!(&decoded.pixels.rgba[..4], &[1, 0, 0, 255]);
	}

	#[test]
	fn svg_renders_at_the_intrinsic_and_requested_size() {
		let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ff0000"/></svg>"##;
		let decoded = decode(svg, None).unwrap();
		assert!(decoded.svg);
		assert_eq!(decoded.intrinsic, (40, 20));
		assert_eq!((decoded.pixels.width, decoded.pixels.height), (40, 20));
		assert_eq!(&decoded.pixels.rgba[..4], &[255, 0, 0, 255]);
		let scaled = decode(svg, Some((80, 40))).unwrap();
		assert_eq!(scaled.intrinsic, (40, 20));
		assert_eq!((scaled.pixels.width, scaled.pixels.height), (80, 40));
		assert!(decode(b"not an image", None).is_err());
		assert!(dimensions(0, 10).is_err());
		assert!(dimensions(5000, 4000).is_err());
		assert!(dimensions(4000, 4000).is_ok());
	}

	#[test]
	#[ignore = "requires a GPU; writes artifacts/images.png"]
	fn gpu_frame_draws_decoded_images() -> Result<()> {
		use crate::{
			layout::{LayoutEngine, LayoutOptions},
			render::{Renderer, View},
		};
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("note.md");
		let source = "![png](a.png)\n\n<img src=\"b.svg\" width=\"80\">\n\n![svg](b.svg)\n";
		fs::write(&path, source)?;
		fs::write(dir.path().join("a.png"), png(40, 30, [255, 0, 255, 255]))?;
		fs::write(
			dir.path().join("b.svg"),
			br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="#00ffff"/></svg>"##,
		)?;
		let doc = crate::document::parse(source.to_string());
		let mut images = Images::new(true);
		images.prepare(&doc, &path, 1);
		images.wait();
		let mut snapshot = LayoutEngine::new().layout_with_images(
			&doc,
			&LayoutOptions {
				width: 400.,
				..Default::default()
			},
			&images.snapshot,
		);
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		let target = renderer.offscreen(400, 300);
		let horizontal = HashMap::new();
		let view = View {
			selection: None,
			revision: 1,
			width: 400,
			height: 300,
			scale: 1.,
			scroll: 0.,
			left: 0.,
			top: 0.,
			bottom: 0.,
			theme: crate::render::Theme::Light,
			horizontal: &horizontal,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let submission = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		assert_eq!(
			images.snapshot.pixels.demand.lock().unwrap()["b.svg"].size,
			(80, 60)
		);
		images.wait();
		assert_eq!(
			images.snapshot.pixels.decoded.lock().unwrap()["b.svg"].width,
			80
		);
		// Updating the resource metadata through layout also updates Draw versions.
		snapshot = LayoutEngine::new().layout_with_images(
			&doc,
			&LayoutOptions {
				width: 400.,
				..Default::default()
			},
			&images.snapshot,
		);
		let submission = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts/images.png");
		fs::create_dir_all(output.parent().unwrap())?;
		renderer.save_png(&target, &output)?;
		let frame = image::open(&output)?.to_rgb8();
		let count = |want: [u8; 3]| {
			frame
				.pixels()
				.filter(|p| {
					let p = p.0;
					(0..3).all(|i| p[i].abs_diff(want[i]) <= 6)
				})
				.count()
		};
		assert!(count([255, 0, 255]) > 800, "PNG pixels missing");
		assert!(count([0, 255, 255]) > 800, "SVG pixels missing");
		Ok(())
	}

	#[test]
	fn loader_publishes_pixels_and_reports_failures() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("note.md");
		let source = "![a](a.png) ![b](missing.png)";
		fs::write(&path, source).unwrap();
		fs::write(dir.path().join("a.png"), png(6, 4, [9, 8, 7, 255])).unwrap();
		let doc = crate::document::parse(source.to_string());
		let mut images = Images::new(true);
		images.prepare(&doc, &path, 1);
		images.wait();
		assert_eq!(images.snapshot.entries["a.png"].size, Some((6, 4)));
		assert!(images.snapshot.entries["a.png"].error.is_none());
		assert!(images.snapshot.entries["missing.png"].error.is_some());
		let pixels = images.snapshot.pixels.decoded.lock().unwrap();
		assert_eq!(pixels["a.png"].width, 6);
		assert!(!pixels.contains_key("missing.png"));
	}

	#[test]
	fn renamed_alias_reuses_pixels_and_removed_aliases_are_released() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("note.md");
		fs::write(dir.path().join("a.png"), png(6, 4, [1, 2, 3, 255])).unwrap();
		let mut images = Images::new(true);
		images.prepare(&crate::document::parse("![a](a.png)"), &path, 1);
		images.wait();
		let first =
			images.snapshot.pixels.decoded.lock().unwrap()["a.png"].clone();
		let version = images.snapshot.entries["a.png"].version;
		images.prepare(&crate::document::parse("![a](./a.png)"), &path, 2);
		assert_eq!(images.snapshot.entries["./a.png"].version, version);
		let pixels = images.snapshot.pixels.decoded.lock().unwrap();
		assert!(!pixels.contains_key("a.png"));
		assert!(Arc::ptr_eq(&first, &pixels["./a.png"]));
	}

	#[test]
	fn obsolete_completion_cannot_replace_a_readded_resource() {
		let mut images = Images::new(true);
		let (send, recv) = mpsc::channel();
		images.recv = recv;
		let path = Path::new("/unused/note.md");
		let doc = crate::document::parse("![a](a.png)");
		images.prepare(&doc, path, 1);
		let src = source("a.png", path, true).unwrap();
		let old_ticket = images.entries[&src].ticket;
		images.prepare(&crate::document::parse("no image"), path, 2);
		images.prepare(&doc, path, 3);
		assert_ne!(images.entries[&src].ticket, old_ticket);
		send.send(Finished {
			source: src.clone(),
			generation: images.generation,
			ticket: old_ticket,
			result: decode(&png(2, 2, [0, 0, 0, 255]), None),
		})
		.unwrap();
		images.poll();
		assert!(images.entries[&src].busy);
		assert_eq!(images.snapshot.entries["a.png"].size, None);
	}

	#[test]
	fn pixel_budget_counts_allocations_and_evicts_even_when_all_are_visible() {
		use markview_core::image::ImageDemand;
		let a = decode(&png(2, 2, [1, 0, 0, 255]), None).unwrap().pixels;
		let b = decode(&png(2, 2, [2, 0, 0, 255]), None).unwrap().pixels;
		let mut pixels =
			HashMap::from([("a".into(), a.clone()), ("alias".into(), a)]);
		assert_eq!(pixel_bytes(&pixels), 16);
		let demand = HashMap::from([
			(
				"a".into(),
				ImageDemand {
					size: (2, 2),
					needs_pixels: false,
				},
			),
			(
				"alias".into(),
				ImageDemand {
					size: (2, 2),
					needs_pixels: false,
				},
			),
		]);
		cache_pixels(&mut pixels, &["b".into()], b, &demand, 16);
		assert_eq!(pixel_bytes(&pixels), 16);
		assert_eq!(pixels.len(), 1);
		assert!(pixels.contains_key("b"));
	}

	#[test]
	fn vector_demand_merges_alias_sizes_and_gpu_residency_avoids_refetch() {
		use markview_core::image::ImageDemand;
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("note.md");
		fs::write(dir.path().join("a.svg"),br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"/>"#).unwrap();
		let mut images = Images::new(true);
		images.prepare(
			&crate::document::parse("![a](a.svg) ![b](./a.svg)"),
			&path,
			1,
		);
		images.wait();
		*images.snapshot.pixels.demand.lock().unwrap() = HashMap::from([
			(
				"a.svg".into(),
				ImageDemand {
					size: (160, 80),
					needs_pixels: false,
				},
			),
			(
				"./a.svg".into(),
				ImageDemand {
					size: (80, 40),
					needs_pixels: false,
				},
			),
		]);
		images.wait();
		let version = images.snapshot.entries["a.svg"].version;
		assert_eq!(
			images.entries.values().next().unwrap().raster,
			Some((160, 80))
		);
		images.snapshot.pixels.decoded.lock().unwrap().clear();
		images.poll();
		assert!(!images.entries.values().next().unwrap().busy);
		assert_eq!(images.snapshot.entries["a.svg"].version, version);
	}
}
