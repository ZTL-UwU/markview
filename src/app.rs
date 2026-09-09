mod chrome;
mod interaction;
mod launch;
use crate::cli::{LaunchOptions, Mode, arguments};
use crate::settings::{ReaderSettings, Setting, SettingsStore};
use crate::state::{Command, InteractionState, ReaderSession};
use crate::{
	benchmark, document,
	file::read_document,
	layout::{Draw, LayoutEngine, LayoutOptions, Paint, Rect, TextShaper},
	render::{Renderer, Theme, View},
	watch::FileWatch,
	worker::{Request, Update, Worker},
};
use anyhow::{Result, bail};
use markview_core::text::TextPosition;
use std::{
	collections::HashMap,
	path::PathBuf,
	sync::Arc,
	time::{Duration, Instant},
};
use winit::{
	application::ApplicationHandler,
	dpi::{LogicalSize, PhysicalSize},
	event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
	keyboard::{Key, NamedKey},
	window::{CursorIcon, Window, WindowId},
};

const TOP: f32 = 58.0;
const BOTTOM: f32 = 28.0;

pub fn run() -> Result<()> {
	launch::run()
}

enum Event {
	Ready(Box<Update>),
	Changed(PathBuf),
	Open(Option<PathBuf>),
	DeviceLost,
}
struct Button {
	rect: Rect,
	label: &'static str,
	action: Command,
}

/// The desktop preference; `None` when the platform does not report one.
fn system_theme(window: &Window) -> Option<Theme> {
	window.theme().map(|theme| match theme {
		winit::window::Theme::Dark => Theme::Dark,
		_ => Theme::Light,
	})
}

struct App {
	interaction: InteractionState,
	session: ReaderSession,
	args: LaunchOptions,
	proxy: EventLoopProxy<Event>,
	window: Option<Arc<Window>>,
	renderer: Option<Renderer>,
	worker: Worker,
	watch: Option<FileWatch>,
	ui: TextShaper,
	settings: ReaderSettings,
	settings_store: SettingsStore,
	settings_warning: Option<String>,
	save_at: Option<Instant>,
	clipboard: crate::platform::Clipboard,
	status: String,
	error: bool,
	dialog_open: bool,
	reflow_at: Option<Instant>,
	retry_at: Option<Instant>,
	first_frame: Option<Update>,
	started: Instant,
	fatal: Option<String>,
}
impl App {
	fn new(args: LaunchOptions, proxy: EventLoopProxy<Event>) -> Self {
		let done = proxy.clone();
		let worker = Worker::new(move |update| {
			let _ = done.send_event(Event::Ready(Box::new(update)));
		});
		let (settings_store, settings_warning) =
			SettingsStore::load(if args.mode == Mode::Window {
				crate::settings::config_path()
			} else {
				None
			});
		let mut settings = settings_store.settings();
		let explicit = ReaderSettings {
			theme: args.theme.unwrap_or_default(),
			font_size: args.options.font_size,
			width: args.options.width,
			justify: args.options.justify,
			hyphenate: args.options.hyphenate,
		};
		for field in &args.overrides {
			settings.copy_field(&explicit, *field);
		}
		Self {
			interaction: InteractionState::default(),
			session: ReaderSession::default(),
			args,
			proxy,
			window: None,
			renderer: None,
			worker,
			watch: None,
			ui: TextShaper::new(),
			settings,
			settings_store,
			settings_warning,
			save_at: None,
			clipboard: Default::default(),
			status: "Read only · local files".into(),
			error: false,
			dialog_open: false,
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
	fn view_geometry(&self) -> markview_core::scene::Viewport {
		let (width, height, _) = self.dimensions();
		markview_core::scene::Viewport {
			width,
			height,
			left: ((width - self.session.snapshot.width) / 2.0).max(20.0),
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			scroll: self.session.scroll,
		}
	}
	fn viewport(&self) -> f32 {
		self.view_geometry().clip().h.max(1.0)
	}
	fn redraw(&self) {
		if let Some(w) = &self.window {
			w.request_redraw();
		}
	}
	fn options(&self) -> LayoutOptions {
		self.settings
			.layout_options(self.dimensions().0, self.args.options.greedy)
	}
	fn request(&mut self, follow: bool) {
		if let Some(path) = &self.session.path {
			self.session.version += 1;
			self.session.follow_update |= follow;
			self.error = false;
			self.status = "Updating…".into();
			self.session.requested_options = Some(self.options());
			self.worker.submit(Request {
				version: self.session.version,
				content_version: self.session.content_version,
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
		self.interaction.clear_selection();
		self.session.path = Some(path.clone());
		self.session.content_version += 1;
		self.session.scroll = 0.0;
		self.session.horizontal.clear();
		let proxy = self.proxy.clone();
		let observed = path.clone();
		self.watch = Some(FileWatch::new(path, move || {
			let _ = proxy.send_event(Event::Changed(observed.clone()));
		}));
		self.request(false);
	}
	fn pointer_in_panel(&self) -> bool {
		let (width, height, _) = self.dimensions();
		self.interaction.panel_open
			&& chrome::panel_rect(width, height)
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
	}
	fn panel_has_focus(&self) -> bool {
		self.interaction.panel_open && self.interaction.focus.is_some()
	}
	fn scroll_by(&mut self, dy: f32) {
		self.session.scroll = (self.session.scroll + dy).clamp(
			0.0,
			(self.session.snapshot.height - self.viewport()).max(0.0),
		);
		self.refresh_hover();
		self.redraw();
	}
	/// The link under a window point, using the same origin as the renderer.
	fn link_at(&self, px: f32, py: f32) -> Option<String> {
		let geometry = self.view_geometry();
		if !geometry.clip().contains(px, py) {
			return None;
		}
		let (x, y) = geometry.document_point(px, py);
		self.session
			.snapshot
			.link_at(x, y, &self.session.horizontal)
			.map(str::to_string)
	}

	/// Hover state follows scrolling and reflow, not only pointer motion.
	fn refresh_hover(&mut self) {
		let hover = if self.pointer_in_panel()
			|| self.interaction.pointer_down.is_some()
		{
			None
		} else {
			self.link_at(self.interaction.cursor.0, self.interaction.cursor.1)
		};
		if hover == self.interaction.hover {
			return;
		}
		self.interaction.hover = hover;
		if let Some(w) = &self.window {
			w.set_cursor(if self.interaction.hover.is_some() {
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
		let (cx, cy) = self.view_geometry().document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		for (bi, b) in self.session.snapshot.blocks.iter().enumerate() {
			for (oi, o) in b.layout.overflow.iter().enumerate() {
				if o.rect.contains(cx, cy - b.y) {
					let offset =
						self.session.horizontal.entry((bi, oi)).or_default();
					*offset = (*offset + dx)
						.clamp(0.0, (o.content_width - o.rect.w).max(0.0));
					self.refresh_hover();
					self.redraw();
					return;
				}
			}
		}
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
			selection: self.interaction.selection,
			revision: self.session.accepted_revision,
			width: size.width,
			height: size.height,
			scale,
			scroll: self.session.scroll,
			left: ((width - self.session.snapshot.width) / 2.0).max(20.0),
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			theme: self.settings.theme,
			horizontal: &self.session.horizontal,
		};
		let Some(renderer) = &mut self.renderer else {
			return Ok(());
		};
		let (frame, suboptimal) = match renderer.acquire(window.clone())? {
			crate::render::FrameStatus::Ready(frame, suboptimal) => {
				(frame, suboptimal)
			}
			crate::render::FrameStatus::Retry(delay) => {
				self.retry_at = Some(Instant::now() + delay);
				return Ok(());
			}
			crate::render::FrameStatus::Occluded => return Ok(()),
		};
		let target = frame.texture.create_view(&Default::default());
		let submission = renderer.render(
			&self.session.snapshot,
			&view,
			&overlay,
			&target,
		)?;
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
				self.session.snapshot.reused,
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
						&self.session.snapshot,
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
		let proxy = self.proxy.clone();
		renderer.on_device_lost(move || {
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
			if self.args.theme.is_none()
				&& self.settings_store.theme_preference().is_none()
				&& let Some(theme) = system_theme(&window)
			{
				self.settings.theme = theme;
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
			Event::Changed(path)
				if self.session.path.as_ref() == Some(&path) =>
			{
				self.session.content_version += 1;
				self.request(true)
			}
			Event::Ready(mut update)
				if update.version == self.session.version =>
			{
				match update.result.take() {
					Some(Ok(reader)) => {
						if self.session.accept(reader, self.viewport()) {
							self.interaction.clear_selection();
						}
						self.error = false;
						self.refresh_hover();
						self.status = if self.session.snapshot.math_errors > 0 {
							format!(
								"Watching file · {} formulas shown as source",
								self.session.snapshot.math_errors
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
					Some(Err(error)) => {
						self.error = true;
						self.status = error;
						if self.args.mode == Mode::Smoke {
							self.fatal = Some(self.status.clone());
							event_loop.exit();
						}
					}
					None => {}
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
			WindowEvent::ModifiersChanged(m) => {
				self.interaction.modifiers = m.state()
			}
			WindowEvent::CursorMoved { position, .. } => {
				let scale = self.dimensions().2;
				let old = self.interaction.cursor;
				self.interaction.cursor =
					(position.x as f32 / scale, position.y as f32 / scale);
				self.update_drag();
				self.refresh_hover();
				if self.interaction.panel_open
					|| old.1 < TOP || self.interaction.cursor.1 < TOP
				{
					self.redraw();
				}
			}
			WindowEvent::CursorLeft { .. } => {
				// Leaving the window can drop the release event; end the drag.
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				if self.interaction.hover.take().is_some() {
					if let Some(w) = &self.window {
						w.set_cursor(CursorIcon::Default);
					}
					self.redraw();
				}
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Pressed,
				..
			} => {
				if let Some(button) = self.buttons().into_iter().find(|b| {
					b.rect.contains(
						self.interaction.cursor.0,
						self.interaction.cursor.1,
					)
				}) {
					self.interaction.focus = Some(button.action);
					self.action(button.action);
				} else if !self.pointer_in_panel()
					&& self.interaction.cursor.1 >= TOP + 10.0
					&& self.interaction.cursor.1
						< self.dimensions().1 - BOTTOM - 10.0
				{
					self.interaction.focus = None;
					if self.interaction.cursor.0 > self.dimensions().0 - 16.0 {
						let fraction = ((self.interaction.cursor.1 - TOP)
							/ (self.dimensions().1 - TOP - BOTTOM))
							.clamp(0.0, 1.0);
						self.session.scroll = fraction
							* (self.session.snapshot.height - self.viewport())
								.max(0.0);
					} else if let Some(position) = self.text_at_cursor() {
						let link = self.link_at(
							self.interaction.cursor.0,
							self.interaction.cursor.1,
						);
						self.interaction.begin_selection(position, link);
					}
					self.redraw();
				}
			}
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Released,
				..
			} => {
				let link = self.link_at(
					self.interaction.cursor.0,
					self.interaction.cursor.1,
				);
				if let Some(link) =
					self.interaction.finish_selection(link.as_deref())
				{
					self.open_link(&link);
				}
				self.refresh_hover();
				self.redraw();
			}
			WindowEvent::Focused(false) => {
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.modifiers = Default::default();
			}
			WindowEvent::MouseWheel { delta, .. } => {
				if self.pointer_in_panel() {
					return;
				}
				let (dx, dy) = match delta {
					MouseScrollDelta::LineDelta(x, y) => (x * 42.0, y * 42.0),
					MouseScrollDelta::PixelDelta(p) => (
						p.x as f32 / self.dimensions().2,
						p.y as f32 / self.dimensions().2,
					),
				};
				if self.interaction.modifiers.control_key()
					|| self.interaction.modifiers.super_key()
				{
					self.action(if dy > 0.0 {
						Command::Larger
					} else {
						Command::Smaller
					});
				} else if self.interaction.modifiers.shift_key()
					|| dx.abs() > dy.abs()
				{
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
				let command = self.interaction.modifiers.control_key()
					|| self.interaction.modifiers.super_key();
				if command {
					if let Key::Character(c) = &event.logical_key {
						match c.to_lowercase().as_str() {
							"a" if !self.panel_has_focus() => {
								self.interaction.selection = self
									.session
									.snapshot
									.select_all(self.session.accepted_revision);
								self.redraw();
							}
							"c" if !self.panel_has_focus() => {
								self.copy_selection()
							}
							"," => self.action(Command::Settings),
							"o" if !self.panel_has_focus() => {
								self.action(Command::Open)
							}
							"t" => self.action(Command::Theme),
							"-" => self.action(Command::Smaller),
							"+" | "=" => self.action(Command::Larger),
							"[" => self.action(Command::Narrower),
							"]" => self.action(Command::Wider),
							"l" => self.action(Command::Align),
							"h" => self.action(Command::Hyphens),
							"q" => event_loop.exit(),
							_ => {}
						}
					}
				} else {
					if self.panel_has_focus()
						&& !matches!(
							event.logical_key,
							Key::Named(
								NamedKey::Tab
									| NamedKey::Enter | NamedKey::Escape
							)
						) {
						return;
					}
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
							self.scroll_by(-self.session.snapshot.height)
						}
						Key::Named(NamedKey::End) => {
							self.scroll_by(self.session.snapshot.height)
						}
						Key::Named(NamedKey::ArrowLeft) => {
							self.horizontal_by(-42.0)
						}
						Key::Named(NamedKey::ArrowRight) => {
							self.horizontal_by(42.0)
						}
						Key::Named(NamedKey::Tab) => {
							let buttons = self.buttons();
							let current = buttons.iter().position(|b| {
								Some(b.action) == self.interaction.focus
							});
							let index = match current {
								None => {
									if self.interaction.modifiers.shift_key() {
										buttons.len() - 1
									} else {
										0
									}
								}
								Some(i) => {
									(i + if self
										.interaction
										.modifiers
										.shift_key()
									{
										buttons.len() - 1
									} else {
										1
									}) % buttons.len()
								}
							};
							self.interaction.focus =
								Some(buttons[index].action);
							self.redraw();
						}
						Key::Named(NamedKey::Enter) => {
							if let Some(action) = self.interaction.focus
								&& self
									.buttons()
									.iter()
									.any(|b| b.action == action)
							{
								self.action(action);
							}
						}
						Key::Named(NamedKey::Escape) => {
							self.interaction.focus = None;
							self.interaction.panel_open = false;
							self.interaction.selection = None;
							self.interaction.pointer_down = None;
							self.interaction.drag_at = None;
							self.refresh_hover();
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
		if self.save_at.is_some_and(|d| d <= now) {
			self.flush_settings();
			self.redraw();
		}
		if self.interaction.drag_at.is_some_and(|d| d <= now) {
			self.interaction.drag_at = None;
			if self.interaction.pointer_down.is_some() {
				self.scroll_by(if self.interaction.cursor.1 < TOP + 24.0 {
					-14.0
				} else {
					14.0
				});
				self.update_drag();
			}
		}
		if self.reflow_at.is_some_and(|d| d <= now) {
			self.reflow_at = None;
			if self.session.requested_options.as_ref() != Some(&self.options())
			{
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
			.chain(self.save_at)
			.chain(self.interaction.drag_at)
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
