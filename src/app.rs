mod anchor;
mod chrome;
mod document;
mod gpui_paint;
mod icon;
mod input;
mod interaction;
mod launch;
mod lifecycle;
mod painting;
mod pointer;
mod preferences;
mod tab_metrics;
mod tab_navigation;
mod tab_strip;
mod tabs;
mod ui;
mod viewport;
use crate::cli::LaunchOptions;
use crate::state::{Command, InteractionState};
use crate::{
	layout::{LayoutOptions, Rect, TextShaper},
	render::Theme,
	watch::FileWatch,
	worker::{Update, Worker},
};
use anyhow::Result;
use gpui::{
	App as GpuiApp, Context, CursorStyle, Decorations, FocusHandle, Window,
	WindowAppearance,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

/// Zed title bar (`h_32` + 1px), then the tab strip.
pub(super) const TITLE: f32 = 33.0;
pub(super) const TAB: f32 = 32.0;
pub(super) const TOP: f32 = TITLE + TAB;
pub(super) const BOTTOM: f32 = 24.0;

#[derive(Clone, Copy, Default)]
pub(super) struct ChromeFrame {
	client: bool,
	minimize: bool,
	maximize: bool,
}

impl ChromeFrame {
	fn from_window(window: &Window) -> Self {
		if cfg!(target_os = "macos") {
			return Self::default();
		}
		if cfg!(target_os = "windows") {
			return Self {
				client: true,
				minimize: true,
				maximize: true,
			};
		}
		let controls = window.window_controls();
		Self {
			client: matches!(
				window.window_decorations(),
				Decorations::Client { .. }
			),
			minimize: controls.minimize,
			maximize: controls.maximize,
		}
	}

	pub(super) fn control_width(self) -> f32 {
		if !self.client {
			0.0
		} else {
			chrome::WINDOW_CONTROL
				* (1 + u8::from(self.minimize) + u8::from(self.maximize)) as f32
		}
	}
}

pub fn run() -> Result<()> {
	launch::run()
}

enum Event {
	Ready(Box<Update>),
	Changed(PathBuf),
	SettingsChanged,
	StylesChanged,
	Open(Option<PathBuf>),
}
struct Button {
	rect: Rect,
	label: &'static str,
	action: Command,
	selected: bool,
}

fn system_theme(window: &Window) -> Theme {
	match window.appearance() {
		WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::Dark,
		_ => Theme::Light,
	}
}

fn modifiers_from(m: gpui::Modifiers) -> crate::state::Modifiers {
	crate::state::Modifiers {
		shift: m.shift,
		ctrl: m.control,
		alt: m.alt,
		logo: m.platform,
	}
}

struct App {
	interaction: InteractionState,
	readers: tabs::Tabs,
	tab_strip: tab_strip::TabStrip,
	tab_metrics: tab_metrics::TabMetrics,
	args: LaunchOptions,
	events: async_channel::Sender<Event>,
	worker: Worker,
	watch: Option<FileWatch>,
	_settings_watch: Option<FileWatch>,
	_styles_watch: Option<FileWatch>,
	ui: TextShaper,
	preferences: preferences::Preferences,
	clipboard: crate::platform::Clipboard,
	paste_dir: tempfile::TempDir,
	paste_serial: u32,
	status: String,
	status_until: Option<Instant>,
	error: bool,
	dialog_open: bool,
	reflow_at: Option<Instant>,
	first_frame: Option<Update>,
	started: Instant,
	fatal: Arc<std::sync::Mutex<Option<String>>>,
	width: f32,
	height: f32,
	scale: f32,
	painter: gpui_paint::GpuiPainter,
	focus: FocusHandle,
	needs_frame: bool,
	timer_gen: u64,
	last_size: (f32, f32, f32),
	title: String,
	pending_quit: bool,
	compositor: String,
	os_theme: Theme,
	chrome_frame: ChromeFrame,
	_appearance: gpui::Subscription,
	_activation: Option<gpui::Subscription>,
	_quit: Option<gpui::Subscription>,
}

impl App {
	pub(super) fn new(
		args: LaunchOptions,
		events: async_channel::Sender<Event>,
		fatal: Arc<std::sync::Mutex<Option<String>>>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let worker_events = events.clone();
		let worker = Worker::with_images(args.offline, move |update| {
			let _ = worker_events.send_blocking(Event::Ready(Box::new(update)));
		});
		let mut ui = TextShaper::new();
		let preferences = preferences::Preferences::new(&args, &mut ui);
		let settings_watch = preferences.path().map(|path| {
			let events = events.clone();
			FileWatch::new(path.to_path_buf(), move || {
				let _ = events.send_blocking(Event::SettingsChanged);
			})
		});
		let styles_watch = crate::stylesheet::directory().map(|dir| {
			let events = events.clone();
			FileWatch::directory(dir, move || {
				let _ = events.send_blocking(Event::StylesChanged);
			})
		});
		window.set_client_inset(gpui::px(5.0));
		let focus = cx.focus_handle();
		focus.focus(window);
		let appearance =
			cx.observe_window_appearance(window, |app, window, cx| {
				if app.args.mode == crate::cli::Mode::Window {
					app.apply_saved_settings(Some(window));
					app.commit(cx);
				}
			});
		let mut painter = gpui_paint::GpuiPainter::new();
		painter.set_stylesheet(preferences.values.stylesheet.clone());
		let size = window.viewport_size();
		let scale = window.scale_factor();
		let width = f32::from(size.width);
		let height = f32::from(size.height);
		eprintln!(
			"Display scale (DPR): {scale:.3}; framebuffer: {}×{} physical px; window: {:.1}×{:.1} logical px",
			(width * scale).round() as u32,
			(height * scale).round() as u32,
			width,
			height,
		);
		Self {
			interaction: InteractionState::default(),
			readers: tabs::Tabs::default(),
			tab_strip: Default::default(),
			tab_metrics: Default::default(),
			args,
			events,
			worker,
			watch: None,
			_settings_watch: settings_watch,
			_styles_watch: styles_watch,
			ui,
			preferences,
			clipboard: Default::default(),
			paste_dir: tempfile::tempdir()
				.expect("create clipboard paste directory"),
			paste_serial: 0,
			status: String::new(),
			status_until: None,
			error: false,
			dialog_open: false,
			reflow_at: None,
			first_frame: None,
			started: Instant::now(),
			fatal,
			width: width.max(1.0),
			height: height.max(1.0),
			scale,
			painter,
			focus,
			needs_frame: true,
			timer_gen: 0,
			last_size: (width.max(1.0), height.max(1.0), scale),
			title: "Markview".into(),
			pending_quit: false,
			compositor: cx.compositor_name().to_string(),
			os_theme: system_theme(window),
			chrome_frame: ChromeFrame::from_window(window),
			_appearance: appearance,
			_activation: None,
			_quit: None,
		}
	}

	fn bind(
		&mut self,
		rx: async_channel::Receiver<Event>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.args.mode == crate::cli::Mode::Window
			&& self.args.theme.is_none()
			&& self.args.style.is_none()
			&& self.preferences.theme_preference().is_none()
		{
			self.preferences.values.theme = system_theme(window);
		}
		self.compositor = cx.compositor_name().to_string();
		if self.compositor.is_empty() {
			self.compositor = "GPUI".into();
		}
		self.reload_styles();
		if let Some(path) = self.args.path.clone() {
			self.open(path);
		}
		cx.spawn(async move |this, cx| {
			while let Ok(event) = rx.recv().await {
				if this
					.update(cx, |app, cx| app.handle_event(event, cx))
					.is_err()
				{
					break;
				}
			}
		})
		.detach();
		self._activation =
			Some(cx.observe_window_activation(window, |app, window, cx| {
				if !window.is_window_active() {
					app.on_focus_lost();
					app.commit(cx);
				}
			}));
		self._quit = Some(cx.on_app_quit(|app, _cx| {
			app.flush_settings();
			if let Some(warning) = &app.preferences.settings_warning {
				eprintln!("{warning}");
			}
			async {}
		}));
		self.schedule_tick(cx);
		self.redraw();
		self.commit(cx);
	}

	fn reload_styles(&mut self) {
		if let Some(reflow) = self.preferences.reload_styles(&mut self.ui) {
			self.painter
				.set_stylesheet(self.preferences.values.stylesheet.clone());
			if reflow {
				self.request(false);
			}
		}
	}
	pub(super) fn dimensions(&self) -> (f32, f32, f32) {
		(self.width, self.height, self.scale)
	}
	pub(super) fn view_geometry(&self) -> markview_core::scene::Viewport {
		let (width, height, _) = self.dimensions();
		markview_core::scene::Viewport {
			width,
			height,
			left: ((width - self.readers.session.snapshot.width) / 2.0)
				.max(20.0),
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			scroll: self.readers.session.scroll,
		}
	}
	pub(super) fn viewport(&self) -> f32 {
		self.view_geometry().clip().h.max(1.0)
	}
	pub(super) fn redraw(&mut self) {
		self.needs_frame = true;
	}
	fn commit(&mut self, cx: &mut Context<Self>) {
		if self.pending_quit {
			cx.defer(|cx| cx.quit());
			return;
		}
		if self.needs_frame {
			self.needs_frame = false;
			cx.notify();
		}
		self.schedule_tick(cx);
	}
	pub(super) fn options(&self) -> LayoutOptions {
		self.preferences
			.values
			.layout_options(self.dimensions().0, self.args.options.greedy)
	}
	fn sync_window(&mut self, window: &Window) {
		let size = window.viewport_size();
		let scale = window.scale_factor();
		self.width = f32::from(size.width).max(1.0);
		self.height = f32::from(size.height).max(1.0);
		self.os_theme = system_theme(window);
		self.chrome_frame = ChromeFrame::from_window(window);
		if (self.scale - scale).abs() > 0.001 {
			self.painter.clear_glyphs();
			self.reflow_at = Some(Instant::now());
			eprintln!("Display scale (DPR) changed: {scale:.3}");
		}
		self.scale = scale;
		let now = (self.width, self.height, self.scale);
		if now != self.last_size {
			self.last_size = now;
			self.tab_strip.reveal_active = true;
			if self.width > 0.0 && self.height > 0.0 {
				self.worker
					.prioritize(self.readers.session.coverage(self.viewport()));
				self.reflow_at =
					Some(Instant::now() + std::time::Duration::from_millis(40));
			}
		}
	}
	fn cursor_style(&mut self) -> CursorStyle {
		if self.tab_strip.drag.is_some_and(|d| d.moving) {
			CursorStyle::ClosedHand
		} else if self.interaction.scrollbar.is_some() {
			CursorStyle::Arrow
		} else if self.interaction.pointer_down.is_some() {
			if self.text_under_cursor() {
				CursorStyle::IBeam
			} else {
				CursorStyle::Arrow
			}
		} else if self.button_at_cursor()
			|| self.tab_at_cursor().is_some()
			|| self.interaction.hover.is_some()
		{
			CursorStyle::PointingHand
		} else if !self.interaction.panel_open && self.text_under_cursor() {
			CursorStyle::IBeam
		} else {
			CursorStyle::Arrow
		}
	}

	fn fail(&mut self, message: String, cx: &mut Context<Self>) {
		*self.fatal.lock().unwrap() = Some(message);
		self.pending_quit = true;
		self.commit(cx);
	}
}

impl gpui::Focusable for App {
	fn focus_handle(&self, _: &GpuiApp) -> FocusHandle {
		self.focus.clone()
	}
}

impl gpui::Render for App {
	fn render(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> impl gpui::IntoElement {
		self.sync_window(window);
		if window.window_title() != self.title {
			window.set_window_title(&self.title);
		}
		use gpui::{DispatchPhase, HitboxBehavior, canvas, div, prelude::*};
		let reader = cx.entity();
		div()
			.id("markview")
			.size_full()
			.track_focus(&self.focus)
			.occlude()
			.key_context("Markview")
			.on_mouse_move(cx.listener(Self::on_mouse_move))
			.on_mouse_down(
				gpui::MouseButton::Left,
				cx.listener(Self::on_left_down),
			)
			.on_mouse_down(
				gpui::MouseButton::Middle,
				cx.listener(Self::on_middle_down),
			)
			.on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::on_left_up))
			.on_scroll_wheel(cx.listener(Self::on_scroll))
			.on_key_down(cx.listener(Self::on_key_down))
			.on_modifiers_changed(cx.listener(Self::on_modifiers))
			.on_drop(cx.listener(Self::on_drop_files))
			.can_drop(|value, _, _| {
				value.downcast_ref::<gpui::ExternalPaths>().is_some()
			})
			.child(
				canvas(
					|bounds, window, _| {
						window.insert_hitbox(bounds, HitboxBehavior::Normal)
					},
					move |_bounds, hitbox, window, cx| {
						reader.update(cx, |app, cx| {
							app.paint_frame(window, &hitbox, cx);
						});
						window.on_mouse_event({
							let reader = reader.clone();
							move |_: &gpui::MouseExitEvent,
							      phase,
							      _window,
							      cx| {
								if phase == DispatchPhase::Bubble {
									reader.update(cx, |app, cx| {
										app.on_cursor_left();
										app.commit(cx);
									});
								}
							}
						});
					},
				)
				.size_full(),
			)
	}
}
