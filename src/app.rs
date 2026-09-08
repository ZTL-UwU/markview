use crate::{
	benchmark, document,
	layout::{
		self, Draw, LayoutEngine, LayoutOptions, LayoutSnapshot, Paint, Rect,
		Theme,
	},
	render::{Renderer, View},
	watch::{FileWatch, Request, Update, Worker, read_document},
};
use anyhow::{Context, Result, bail};
use std::{
	collections::HashMap,
	path::PathBuf,
	sync::{Arc, atomic::Ordering},
	time::{Duration, Instant},
};
use winit::{
	application::ApplicationHandler,
	dpi::{LogicalSize, PhysicalSize},
	event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
	keyboard::{Key, ModifiersState, NamedKey},
	window::{CursorIcon, Window, WindowId},
};

const TOP: f32 = 58.0;
const BOTTOM: f32 = 28.0;

#[derive(Default, PartialEq, Eq)]
enum Mode {
	#[default]
	Window,
	Render,
	Bench,
	Smoke,
}
struct Args {
	mode: Mode,
	path: Option<PathBuf>,
	output: Option<PathBuf>,
	width: u32,
	height: u32,
	scale: f32,
	scroll: f32,
	theme: Option<Theme>,
	iterations: usize,
	options: LayoutOptions,
}
impl Default for Args {
	fn default() -> Self {
		Self {
			mode: Mode::Window,
			path: None,
			output: None,
			width: 1200,
			height: 800,
			scale: 1.0,
			scroll: 0.0,
			theme: None,
			iterations: 100,
			options: LayoutOptions::default(),
		}
	}
}

fn arguments() -> Result<Option<Args>> {
	let mut out = Args::default();
	let mut args = std::env::args_os().skip(1);
	while let Some(arg) = args.next() {
		let text = arg.to_string_lossy();
		match text.as_ref() {
			"-h" | "--help" => {
				println!(
					"Markview — native Markdown reading\n\nmarkview [FILE]\nmarkview --render FILE --output preview.png [--dark] [--scale 2]\nmarkview --bench FILE [--iterations 100] [--output metrics.json]\nmarkview --smoke-test FILE [--output window.png]\n\nOptions: --width N --height N --column N --font-size N --scroll N\n         --scale N --dark --light --left --no-hyphens --greedy\n\nKeyboard: Ctrl+O open · Ctrl+T theme · Ctrl+ +/- font size\n          Ctrl+[ / ] column width · Ctrl+L alignment · Ctrl+H hyphenation\n          arrows / PageUp / PageDown / Home / End scroll\n          Shift+wheel scroll wide blocks · Tab/Enter toolbar\n          click a link to open http, https or mailto in the system browser\n\n--render and --bench use the real GPU pipeline offscreen.\n--greedy is a typography comparison mode."
				);
				return Ok(None);
			}
			"--render" => out.mode = Mode::Render,
			"--bench" => out.mode = Mode::Bench,
			"--smoke-test" => out.mode = Mode::Smoke,
			"--output" | "-o" => {
				out.output = Some(
					args.next().context("--output requires a path")?.into(),
				)
			}
			"--dark" => out.theme = Some(Theme::Dark),
			"--light" => out.theme = Some(Theme::Light),
			"--left" => out.options.justify = false,
			"--no-hyphens" => out.options.hyphenate = false,
			"--greedy" => out.options.greedy = true,
			"--width" | "--height" | "--column" | "--font-size" | "--scale"
			| "--scroll" | "--iterations" => {
				let value = args
					.next()
					.with_context(|| format!("{text} requires a number"))?;
				let number: f32 = value
					.to_string_lossy()
					.parse()
					.context("Invalid number")?;
				if !number.is_finite() || number < 0.0 {
					bail!("Invalid value for {text}");
				}
				match text.as_ref() {
					"--width" => out.width = (number as u32).clamp(320, 8192),
					"--height" => out.height = (number as u32).clamp(240, 8192),
					"--column" => {
						out.options.width = number.clamp(240.0, 1600.0)
					}
					"--font-size" => {
						out.options.font_size = number.clamp(10.0, 40.0)
					}
					"--scale" => out.scale = number.clamp(0.5, 4.0),
					"--scroll" => out.scroll = number,
					"--iterations" => {
						out.iterations = (number as usize).clamp(1, 10_000)
					}
					_ => {}
				}
			}
			"--" => {
				out.path = args.next().map(PathBuf::from);
				if args.next().is_some() {
					bail!("Open one document at a time");
				}
				break;
			}
			_ if text.starts_with('-') => {
				bail!("Unknown option {text}; use --help")
			}
			_ => {
				if out.path.is_some() {
					bail!("Open one document at a time");
				}
				out.path = Some(arg.into());
			}
		}
	}
	if out.mode != Mode::Window && out.path.is_none() {
		bail!("This mode requires a Markdown file");
	}
	if out.mode == Mode::Render && out.output.is_none() {
		bail!("--render requires --output preview.png");
	}
	Ok(Some(out))
}

pub fn run() -> Result<()> {
	let Some(mut args) = arguments()? else {
		return Ok(());
	};
	if args.mode == Mode::Render || args.mode == Mode::Bench {
		args.options.width = args
			.options
			.width
			.min(args.width as f32 / args.scale - 32.0)
			.max(80.0);
		let path = args.path.as_ref().unwrap();
		if args.mode == Mode::Bench {
			return benchmark::run(
				path,
				args.output.as_deref(),
				args.width,
				args.height,
				args.scale,
				args.theme.unwrap_or_default(),
				args.iterations,
				args.options,
			);
		}
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		let mut engine = LayoutEngine::new();
		let doc = document::parse(read_document(path)?);
		let snapshot = engine.layout(&doc, &args.options);
		let target = renderer.offscreen(args.width, args.height);
		let horizontal = HashMap::new();
		let view = View {
			width: args.width,
			height: args.height,
			scale: args.scale,
			scroll: args.scroll,
			left: ((args.width as f32 / args.scale - args.options.width) / 2.0)
				.max(16.0),
			top: 24.0,
			bottom: 24.0,
			theme: args.theme.unwrap_or_default(),
			horizontal: &horizontal,
		};
		let index = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(index))?;
		let output = args.output.as_ref().unwrap();
		if let Some(parent) =
			output.parent().filter(|p| !p.as_os_str().is_empty())
		{
			std::fs::create_dir_all(parent)?;
		}
		renderer.save_png(&target, output)?;
		eprintln!(
			"Rendered {} blocks, {:.0}px tall, {} degraded paragraphs, {} formula errors; {}",
			snapshot.blocks.len(),
			snapshot.height,
			snapshot.degraded,
			snapshot.math_errors,
			renderer.adapter_name
		);
		return Ok(());
	}
	let event_loop = EventLoop::<Event>::with_user_event().build()?;
	event_loop.set_control_flow(ControlFlow::Wait);
	let mut app = App::new(args, event_loop.create_proxy());
	event_loop.run_app(&mut app)?;
	if let Some(error) = app.fatal {
		bail!("{error}");
	}
	Ok(())
}

enum Event {
	Ready(Box<Update>),
	Changed(PathBuf),
	Open(Option<PathBuf>),
	DeviceLost,
}
#[derive(Clone, Copy)]
enum Action {
	Open,
	Theme,
	Smaller,
	Larger,
	Narrower,
	Wider,
	Align,
	Hyphens,
}
struct Button {
	rect: Rect,
	label: &'static str,
	action: Action,
}

struct App {
	args: Args,
	proxy: EventLoopProxy<Event>,
	window: Option<Arc<Window>>,
	renderer: Option<Renderer>,
	worker: Worker,
	watch: Option<FileWatch>,
	path: Option<PathBuf>,
	snapshot: LayoutSnapshot,
	ui: LayoutEngine,
	theme: Theme,
	version: u64,
	requested_options: Option<LayoutOptions>,
	scroll: f32,
	horizontal: HashMap<(usize, usize), f32>,
	modifiers: ModifiersState,
	cursor: (f32, f32),
	hover: Option<String>,
	focus: Option<usize>,
	status: String,
	error: bool,
	dialog_open: bool,
	follow_update: bool,
	reflow_at: Option<Instant>,
	retry_at: Option<Instant>,
	first_frame: Option<Update>,
	started: Instant,
	fatal: Option<String>,
}
impl App {
	fn new(args: Args, proxy: EventLoopProxy<Event>) -> Self {
		let done = proxy.clone();
		let worker = Worker::new(move |update| {
			let _ = done.send_event(Event::Ready(Box::new(update)));
		});
		let theme = args.theme.unwrap_or_default();
		Self {
			args,
			proxy,
			window: None,
			renderer: None,
			worker,
			watch: None,
			path: None,
			snapshot: LayoutSnapshot::default(),
			ui: LayoutEngine::new(),
			theme,
			version: 0,
			requested_options: None,
			scroll: 0.0,
			horizontal: HashMap::new(),
			modifiers: ModifiersState::empty(),
			cursor: (0.0, 0.0),
			hover: None,
			focus: None,
			status: "Read only · local files".into(),
			error: false,
			dialog_open: false,
			follow_update: false,
			reflow_at: None,
			retry_at: None,
			first_frame: None,
			started: Instant::now(),
			fatal: None,
		}
	}
	fn dimensions(&self) -> (f32, f32, f32) {
		self.window.as_ref().map_or((1200.0, 800.0, 1.0), |w| {
			let s = w.scale_factor() as f32;
			let size = w.inner_size();
			(size.width as f32 / s, size.height as f32 / s, s)
		})
	}
	fn viewport(&self) -> f32 {
		(self.dimensions().1 - TOP - BOTTOM - 20.0).max(1.0)
	}
	fn redraw(&self) {
		if let Some(w) = &self.window {
			w.request_redraw();
		}
	}
	fn options(&self) -> LayoutOptions {
		LayoutOptions {
			width: self
				.args
				.options
				.width
				.min(self.dimensions().0 - 40.0)
				.max(80.0),
			..self.args.options.clone()
		}
	}
	fn request(&mut self, follow: bool) {
		if let Some(path) = &self.path {
			self.version += 1;
			self.follow_update = follow;
			self.error = false;
			self.status = "Updating…".into();
			self.requested_options = Some(self.options());
			self.worker.submit(Request {
				version: self.version,
				path: path.clone(),
				options: self.options(),
				requested: Instant::now(),
			});
			self.redraw();
		}
	}
	fn open(&mut self, path: PathBuf) {
		let path = if path.is_absolute() {
			path
		} else {
			std::env::current_dir().unwrap_or_default().join(path)
		};
		let path = std::fs::canonicalize(&path).unwrap_or(path);
		self.path = Some(path.clone());
		self.scroll = 0.0;
		self.horizontal.clear();
		let proxy = self.proxy.clone();
		let observed = path.clone();
		self.watch = Some(FileWatch::new(path, move || {
			let _ = proxy.send_event(Event::Changed(observed.clone()));
		}));
		self.request(false);
	}
	fn buttons(&self) -> Vec<Button> {
		let (width, _, _) = self.dimensions();
		let mut x = if width >= 720.0 { 126.0 } else { 12.0 };
		let entries = [
			("Open", 60.0, Action::Open),
			(
				if self.theme == Theme::Light {
					"Dark"
				} else {
					"Light"
				},
				60.0,
				Action::Theme,
			),
			("A−", 40.0, Action::Smaller),
			("A+", 40.0, Action::Larger),
			("W−", 40.0, Action::Narrower),
			("W+", 40.0, Action::Wider),
			(
				if self.args.options.justify {
					"Justify"
				} else {
					"Left"
				},
				68.0,
				Action::Align,
			),
			(
				if self.args.options.hyphenate {
					"Hyphens"
				} else {
					"No hyph."
				},
				82.0,
				Action::Hyphens,
			),
		];
		entries
			.into_iter()
			.map(|(label, w, action)| {
				let rect = Rect {
					x,
					y: 10.0,
					w,
					h: 34.0,
				};
				x += w + 2.0;
				Button {
					rect,
					label,
					action,
				}
			})
			.collect()
	}
	fn action(&mut self, action: Action) {
		match action {
			Action::Open => {
				if self.dialog_open {
					return;
				}
				self.dialog_open = true;
				let proxy = self.proxy.clone();
				std::thread::spawn(move || {
					let path = rfd::FileDialog::new()
						.add_filter(
							"Markdown",
							&["md", "markdown", "mdown", "txt"],
						)
						.pick_file();
					let _ = proxy.send_event(Event::Open(path));
				});
				return;
			}
			Action::Theme => {
				self.theme = if self.theme == Theme::Light {
					Theme::Dark
				} else {
					Theme::Light
				};
				self.redraw();
				return;
			}
			Action::Smaller => {
				self.args.options.font_size =
					(self.args.options.font_size - 1.0).max(10.0)
			}
			Action::Larger => {
				self.args.options.font_size =
					(self.args.options.font_size + 1.0).min(40.0)
			}
			Action::Narrower => {
				self.args.options.width =
					(self.args.options.width - 60.0).max(240.0)
			}
			Action::Wider => {
				self.args.options.width =
					(self.args.options.width + 60.0).min(1600.0)
			}
			Action::Align => {
				self.args.options.justify = !self.args.options.justify
			}
			Action::Hyphens => {
				self.args.options.hyphenate = !self.args.options.hyphenate
			}
		}
		self.request(false);
		self.redraw();
	}
	fn scroll_by(&mut self, dy: f32) {
		self.scroll = (self.scroll + dy)
			.clamp(0.0, (self.snapshot.height - self.viewport()).max(0.0));
		self.refresh_hover();
		self.redraw();
	}
	/// The link under a window point, using the same origin as the renderer.
	fn link_at(&self, px: f32, py: f32) -> Option<String> {
		let left =
			((self.dimensions().0 - self.snapshot.width) / 2.0).max(20.0);
		self.snapshot
			.link_at(px - left, py - TOP - 10.0 + self.scroll, &self.horizontal)
			.map(str::to_string)
	}
	/// Hover state follows scrolling and reflow, not only pointer motion.
	fn refresh_hover(&mut self) {
		let hover = self.link_at(self.cursor.0, self.cursor.1);
		if hover == self.hover {
			return;
		}
		self.hover = hover;
		if let Some(w) = &self.window {
			w.set_cursor(if self.hover.is_some() {
				CursorIcon::Pointer
			} else {
				CursorIcon::Default
			});
		}
		self.redraw();
	}
	fn open_link(&mut self, url: &str) {
		if !document::openable_link(url) {
			self.error = true;
			self.status =
				format!("Not opened: {url} — only http, https and mailto");
		} else {
			self.error = false;
			self.status = match open::that_detached(url) {
				Ok(()) => format!("Opened {url}"),
				Err(error) => {
					self.error = true;
					format!("Cannot open {url}: {error}")
				}
			};
		}
		self.redraw();
	}
	fn horizontal_by(&mut self, dx: f32) {
		let left =
			((self.dimensions().0 - self.snapshot.width) / 2.0).max(20.0);
		let (cx, cy) = (
			self.cursor.0 - left,
			self.cursor.1 - TOP - 10.0 + self.scroll,
		);
		for (bi, b) in self.snapshot.blocks.iter().enumerate() {
			for (oi, o) in b.layout.overflow.iter().enumerate() {
				if o.rect.contains(cx, cy - b.y) {
					let offset = self.horizontal.entry((bi, oi)).or_default();
					*offset = (*offset + dx)
						.clamp(0.0, (o.content_width - o.rect.w).max(0.0));
					self.refresh_hover();
					self.redraw();
					return;
				}
			}
		}
	}
	fn overlay(&mut self) -> Vec<Draw> {
		let (width, height, _) = self.dimensions();
		let mut out = vec![
			Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: width,
					h: TOP,
				},
				Paint::Background,
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Border,
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: height - BOTTOM,
					w: width,
					h: BOTTOM,
				},
				Paint::Background,
			),
		];
		if width >= 720.0 {
			out.extend(self.ui.label(
				"MARKVIEW",
				13.0,
				20.0,
				32.0,
				Paint::Accent,
			));
		}
		for (i, b) in self.buttons().iter().enumerate() {
			if self.focus == Some(i) {
				out.push(Draw::Rect(b.rect, Paint::Accent));
				out.push(Draw::Rect(
					Rect {
						x: b.rect.x + 1.0,
						y: b.rect.y + 1.0,
						w: b.rect.w - 2.0,
						h: b.rect.h - 2.0,
					},
					Paint::Panel,
				));
			} else if b.rect.contains(self.cursor.0, self.cursor.1) {
				out.push(Draw::Rect(b.rect, Paint::Panel));
			}
			out.extend(self.ui.label(
				b.label,
				13.0,
				b.rect.x + 9.0,
				b.rect.y + 22.0,
				Paint::Text,
			));
		}
		let max_chars = (width / 7.0) as usize;
		let status: String = self
			.status
			.chars()
			.take(max_chars.saturating_sub(8))
			.collect();
		out.extend(self.ui.label(
			&status,
			11.0,
			20.0,
			height - 10.0,
			if self.error {
				Paint::Error
			} else {
				Paint::Muted
			},
		));
		// Like a browser, the hovered target appears at the bottom right.
		if let Some(url) = self.hover.clone() {
			out.extend(self.ui.right_label(
				&url,
				11.0,
				(width * 0.6).max(120.0),
				width - 20.0,
				height - 10.0,
				Paint::Muted,
			));
		}
		if self.snapshot.blocks.is_empty() {
			let x = ((width - 440.0) / 2.0).max(24.0);
			let y = (height * 0.4).max(110.0);
			let title = if self.path.is_none() {
				"Open a Markdown file"
			} else if self.error {
				"Unable to read this file"
			} else {
				"The document is empty"
			};
			out.extend(self.ui.label(title, 26.0, x, y, Paint::Text));
			out.extend(self.ui.label(
				"Drop a file here or press Ctrl+O.",
				15.0,
				x,
				y + 38.0,
				Paint::Muted,
			));
		}
		let viewport = self.viewport();
		if self.snapshot.height > viewport {
			let track = height - TOP - BOTTOM;
			let h = (track * viewport / self.snapshot.height).max(20.0);
			let y = TOP
				+ self.scroll / (self.snapshot.height - viewport) * (track - h);
			out.push(Draw::Rect(
				Rect {
					x: width - 7.0,
					y,
					w: 3.0,
					h,
				},
				Paint::Muted,
			));
		}
		out
	}
	fn render(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
		let Some(window) = self.window.clone() else {
			return Ok(());
		};
		let size = window.inner_size();
		if size.width == 0 || size.height == 0 {
			return Ok(());
		}
		let overlay = self.overlay();
		let (width, _, scale) = self.dimensions();
		let view = View {
			width: size.width,
			height: size.height,
			scale,
			scroll: self.scroll,
			left: ((width - self.snapshot.width) / 2.0).max(20.0),
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			theme: self.theme,
			horizontal: &self.horizontal,
		};
		let Some(renderer) = &mut self.renderer else {
			return Ok(());
		};
		let surface = renderer.surface.as_ref().unwrap();
		let (frame, suboptimal) = match surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
			wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
			wgpu::CurrentSurfaceTexture::Outdated => {
				renderer.resize(size.width, size.height);
				self.retry_at =
					Some(Instant::now() + Duration::from_millis(16));
				return Ok(());
			}
			wgpu::CurrentSurfaceTexture::Lost => {
				renderer.surface =
					Some(renderer.instance.create_surface(window.clone())?);
				renderer.resize(size.width, size.height);
				self.retry_at =
					Some(Instant::now() + Duration::from_millis(16));
				return Ok(());
			}
			wgpu::CurrentSurfaceTexture::Timeout => {
				self.retry_at =
					Some(Instant::now() + Duration::from_millis(30));
				return Ok(());
			}
			wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
			wgpu::CurrentSurfaceTexture::Validation => {
				bail!("GPU surface validation failed")
			}
		};
		let target = frame.texture.create_view(&Default::default());
		let submission =
			renderer.render(&self.snapshot, &view, &overlay, &target)?;
		window.pre_present_notify();
		frame.present();
		if suboptimal {
			renderer.resize(size.width, size.height);
		}
		if let Some(update) = self.first_frame.take() {
			renderer.wait(Some(submission))?;
			eprintln!(
				"open→GPU complete: {:.2} ms (read {:.2}, parse {:.2}, layout {:.2}); reused {} blocks; {}",
				update.requested.elapsed().as_secs_f64() * 1000.0,
				update.read_ms,
				update.parse_ms,
				update.layout_ms,
				self.snapshot.reused,
				renderer.adapter_name
			);
			if self.args.mode == Mode::Smoke {
				eprintln!(
					"process app entry→readable GPU frame: {:.2} ms; memory {}",
					self.started.elapsed().as_secs_f64() * 1000.0,
					serde_json::to_string(&benchmark::memory())?
				);
				if let Some(output) = &self.args.output {
					if let Some(parent) =
						output.parent().filter(|p| !p.as_os_str().is_empty())
					{
						std::fs::create_dir_all(parent)?;
					}
					let texture = renderer.offscreen(size.width, size.height);
					let s = renderer.render(
						&self.snapshot,
						&view,
						&overlay,
						&texture.create_view(&Default::default()),
					)?;
					renderer.wait(Some(s))?;
					renderer.save_png(&texture, output)?;
				}
				event_loop.exit();
			}
		}
		Ok(())
	}
	fn gpu(&mut self) -> Result<()> {
		let renderer = pollster::block_on(Renderer::new(self.window.clone()))?;
		let flag = renderer.lost.clone();
		let proxy = self.proxy.clone();
		renderer.device.set_device_lost_callback(move |reason, _| {
			if reason == wgpu::DeviceLostReason::Destroyed {
				return;
			}
			flag.store(true, Ordering::Relaxed);
			let _ = proxy.send_event(Event::DeviceLost);
		});
		self.renderer = Some(renderer);
		Ok(())
	}
}

impl ApplicationHandler<Event> for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}
		let result = (|| -> Result<()> {
			let window = Arc::new(
				event_loop.create_window(
					Window::default_attributes()
						.with_title("Markview")
						.with_inner_size(LogicalSize::new(
							self.args.width,
							self.args.height,
						))
						.with_min_inner_size(LogicalSize::new(500, 300)),
				)?,
			);
			if self.args.theme.is_none() {
				self.theme =
					if window.theme() == Some(winit::window::Theme::Dark) {
						Theme::Dark
					} else {
						Theme::Light
					};
			}
			let size = window.inner_size();
			let scale = window.scale_factor();
			eprintln!(
				"Display scale (DPR): {scale:.3}; framebuffer: {}×{} physical px; window: {:.1}×{:.1} logical px",
				size.width,
				size.height,
				size.width as f64 / scale,
				size.height as f64 / scale,
			);
			self.window = Some(window);
			self.gpu()?;
			if let Some(path) = self.args.path.clone() {
				self.open(path);
			}
			self.redraw();
			Ok(())
		})();
		if let Err(e) = result {
			self.fatal = Some(format!("{e:#}"));
			event_loop.exit();
		}
	}
	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
		match event {
			Event::Open(path) => {
				self.dialog_open = false;
				if let Some(path) = path {
					self.open(path);
				}
			}
			Event::Changed(path) if self.path.as_ref() == Some(&path) => {
				self.request(true)
			}
			Event::Ready(mut update) if update.version == self.version => {
				match std::mem::replace(&mut update.result, Err(String::new()))
				{
					Ok(snapshot) => {
						self.scroll = if self.snapshot.blocks.is_empty() {
							0.0
						} else {
							layout::anchored_scroll(
								&self.snapshot,
								&snapshot,
								self.scroll,
								self.viewport(),
								self.follow_update,
							)
						};
						self.snapshot = snapshot;
						self.horizontal.clear();
						self.error = false;
						self.refresh_hover();
						self.status = if self.snapshot.math_errors > 0 {
							format!(
								"Watching file · {} formulas shown as source",
								self.snapshot.math_errors
							)
						} else if self.watch.as_ref().is_some_and(|w| w.polling)
						{
							"Watching file · periodic checks".into()
						} else {
							"Watching file · read only".into()
						};
						if let Some(w) = &self.window {
							w.set_title(&format!(
								"{} — Markview",
								update
									.path
									.file_name()
									.unwrap_or_default()
									.to_string_lossy()
							));
						}
						self.first_frame = Some(*update);
					}
					Err(error) => {
						self.error = true;
						self.status = error;
						if self.args.mode == Mode::Smoke {
							self.fatal = Some(self.status.clone());
							event_loop.exit();
						}
					}
				}
				self.redraw();
			}
			Event::DeviceLost => {
				if let Err(e) = self.gpu() {
					self.fatal = Some(format!("GPU recovery failed: {e:#}"));
					event_loop.exit();
				} else {
					self.redraw();
				}
			}
			_ => {}
		}
	}
	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		_: WindowId,
		event: WindowEvent,
	) {
		match event {
			WindowEvent::CloseRequested => event_loop.exit(),
			WindowEvent::Resized(PhysicalSize { width, height }) => {
				if let Some(r) = &mut self.renderer {
					r.resize(width, height);
				}
				if width > 0 && height > 0 {
					self.reflow_at =
						Some(Instant::now() + Duration::from_millis(40));
					self.redraw();
				}
			}
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				eprintln!("Display scale (DPR) changed: {scale_factor:.3}");
				if let Some(r) = &mut self.renderer {
					r.clear_raster_cache();
				}
				self.reflow_at = Some(Instant::now());
				self.redraw();
			}
			WindowEvent::Occluded(false) => self.redraw(),
			WindowEvent::DroppedFile(path) => self.open(path),
			WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
			WindowEvent::CursorMoved { position, .. } => {
				let scale = self.dimensions().2;
				let old = self.cursor;
				self.cursor =
					(position.x as f32 / scale, position.y as f32 / scale);
				self.refresh_hover();
				if old.1 < TOP || self.cursor.1 < TOP {
					self.redraw();
				}
			}
			WindowEvent::CursorLeft { .. } if self.hover.take().is_some() => {
				if let Some(w) = &self.window {
					w.set_cursor(CursorIcon::Default);
				}
				self.redraw();
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Pressed,
				..
			} => {
				if let Some((i, b)) =
					self.buttons().iter().enumerate().find(|(_, b)| {
						b.rect.contains(self.cursor.0, self.cursor.1)
					}) {
					self.focus = Some(i);
					self.action(b.action);
				} else if let Some(url) = self.hover.clone() {
					self.open_link(&url);
				} else {
					self.focus = None;
					let (_, h, _) = self.dimensions();
					if self.cursor.0 > self.dimensions().0 - 16.0 {
						let fraction = ((self.cursor.1 - TOP)
							/ (h - TOP - BOTTOM))
							.clamp(0.0, 1.0);
						self.scroll = fraction
							* (self.snapshot.height - self.viewport()).max(0.0);
						self.redraw();
					}
				}
			}
			WindowEvent::MouseWheel { delta, .. } => {
				let (dx, dy) = match delta {
					MouseScrollDelta::LineDelta(x, y) => (x * 42.0, y * 42.0),
					MouseScrollDelta::PixelDelta(p) => (
						p.x as f32 / self.dimensions().2,
						p.y as f32 / self.dimensions().2,
					),
				};
				if self.modifiers.control_key() || self.modifiers.super_key() {
					self.action(if dy > 0.0 {
						Action::Larger
					} else {
						Action::Smaller
					});
				} else if self.modifiers.shift_key() || dx.abs() > dy.abs() {
					self.horizontal_by(if dx.abs() > dy.abs() {
						-dx
					} else {
						-dy
					});
				} else {
					self.scroll_by(-dy);
				}
			}
			WindowEvent::KeyboardInput { event, .. }
				if event.state == ElementState::Pressed =>
			{
				let command =
					self.modifiers.control_key() || self.modifiers.super_key();
				if command {
					if let Key::Character(c) = &event.logical_key {
						match c.to_lowercase().as_str() {
							"o" => self.action(Action::Open),
							"t" => self.action(Action::Theme),
							"-" => self.action(Action::Smaller),
							"+" | "=" => self.action(Action::Larger),
							"[" => self.action(Action::Narrower),
							"]" => self.action(Action::Wider),
							"l" => self.action(Action::Align),
							"h" => self.action(Action::Hyphens),
							"q" => event_loop.exit(),
							_ => {}
						}
					}
				} else {
					match event.logical_key {
						Key::Named(NamedKey::ArrowDown) => self.scroll_by(42.0),
						Key::Named(NamedKey::ArrowUp) => self.scroll_by(-42.0),
						Key::Named(NamedKey::PageDown | NamedKey::Space) => {
							self.scroll_by(self.viewport() * 0.9)
						}
						Key::Named(NamedKey::PageUp) => {
							self.scroll_by(-self.viewport() * 0.9)
						}
						Key::Named(NamedKey::Home) => {
							self.scroll_by(-self.snapshot.height)
						}
						Key::Named(NamedKey::End) => {
							self.scroll_by(self.snapshot.height)
						}
						Key::Named(NamedKey::ArrowLeft) => {
							self.horizontal_by(-42.0)
						}
						Key::Named(NamedKey::ArrowRight) => {
							self.horizontal_by(42.0)
						}
						Key::Named(NamedKey::Tab) => {
							self.focus = Some(self.focus.map_or(0, |i| {
								(i + if self.modifiers.shift_key() {
									7
								} else {
									1
								}) % 8
							}));
							self.redraw();
						}
						Key::Named(NamedKey::Enter) => {
							if let Some(i) = self.focus {
								self.action(self.buttons()[i].action);
							}
						}
						Key::Named(NamedKey::Escape) => {
							self.focus = None;
							self.redraw();
						}
						_ => {}
					}
				}
			}
			WindowEvent::RedrawRequested => {
				if let Err(e) = self.render(event_loop) {
					self.error = true;
					self.status = format!("Rendering failed: {e:#}");
					eprintln!("{}", self.status);
					if self.args.mode == Mode::Smoke {
						self.fatal = Some(self.status.clone());
						event_loop.exit();
					}
				}
			}
			_ => {}
		}
	}
	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		let now = Instant::now();
		if self.reflow_at.is_some_and(|d| d <= now) {
			self.reflow_at = None;
			if self.requested_options.as_ref() != Some(&self.options()) {
				self.request(false);
			}
			self.scroll_by(0.0);
		}
		if self.retry_at.is_some_and(|d| d <= now) {
			self.retry_at = None;
			self.redraw();
		}
		if self.args.mode == Mode::Smoke
			&& self.started.elapsed() > Duration::from_secs(30)
		{
			self.fatal = Some("Native window smoke test timed out".into());
			event_loop.exit();
			return;
		}
		let deadline = self
			.reflow_at
			.into_iter()
			.chain(self.retry_at)
			.chain(
				(self.args.mode == Mode::Smoke)
					.then_some(self.started + Duration::from_secs(30)),
			)
			.min();
		event_loop.set_control_flow(
			deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
		);
	}
}
