//! Commands, selection gestures and clipboard actions.
use super::*;
impl App {
	pub(super) fn action(&mut self, action: Command) {
		match action {
			Command::Settings => {
				self.interaction.panel_open = !self.interaction.panel_open;
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.focus =
					self.interaction.panel_open.then_some(Command::Theme);
				self.refresh_hover();
				self.redraw();
				return;
			}
			Command::Reset => {
				self.settings = ReaderSettings::default();
				// Reset also drops the saved theme preference, so the system
				// theme applies again immediately and on the next launch.
				if let Some(theme) =
					self.window.as_ref().and_then(|w| system_theme(w))
				{
					self.settings.theme = theme;
				}
			}
			Command::Open => {
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
			Command::Theme => {
				self.settings.theme = if self.settings.theme == Theme::Light {
					Theme::Dark
				} else {
					Theme::Light
				};
				self.setting_changed(Some(Setting::Theme));
				self.redraw();
				return;
			}
			Command::Smaller => {
				self.settings.font_size =
					(self.settings.font_size - 1.0).max(10.0)
			}
			Command::Larger => {
				self.settings.font_size =
					(self.settings.font_size + 1.0).min(40.0)
			}
			Command::Narrower => {
				self.settings.width = (self.settings.width - 60.0).max(240.0)
			}
			Command::Wider => {
				self.settings.width = (self.settings.width + 60.0).min(1600.0)
			}
			Command::Align => self.settings.justify = !self.settings.justify,
			Command::Hyphens => {
				self.settings.hyphenate = !self.settings.hyphenate
			}
		}
		let field = match action {
			Command::Smaller | Command::Larger => Some(Setting::FontSize),
			Command::Narrower | Command::Wider => Some(Setting::Width),
			Command::Align => Some(Setting::Justify),
			Command::Hyphens => Some(Setting::Hyphenate),
			_ => None,
		};
		self.setting_changed(field);
		self.request(false);
		self.redraw();
	}
	pub(super) fn setting_changed(&mut self, field: Option<Setting>) {
		if self.args.mode == Mode::Window {
			self.settings_store.changed(&self.settings, field);
			self.save_at = Some(Instant::now() + Duration::from_millis(250));
		}
	}
	pub(super) fn text_at_cursor(&self) -> Option<TextPosition> {
		let (x, y) = self.view_geometry().document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		self.session.snapshot.hit_test_text(
			x,
			y,
			&self.session.horizontal,
			self.session.accepted_revision,
		)
	}

	pub(super) fn update_drag(&mut self) {
		let position = if self.interaction.pointer_down.is_some() {
			self.text_at_cursor()
		} else {
			None
		};
		self.interaction.move_selection(position);
		if self.interaction.pointer_down.is_some() && self.interaction.dragged {
			let (_, height, _) = self.dimensions();
			let can_scroll = (self.interaction.cursor.1 < TOP + 24.0
				&& self.session.scroll > 0.0)
				|| (self.interaction.cursor.1 > height - BOTTOM - 24.0
					&& self.session.scroll
						< (self.session.snapshot.height - self.viewport())
							.max(0.0));
			self.interaction.drag_at =
				can_scroll.then(|| Instant::now() + Duration::from_millis(16));
			self.redraw();
		}
	}

	pub(super) fn copy_selection(&mut self) {
		if let Some(selection) = self.interaction.selection
			&& !selection.is_empty()
		{
			let text = self
				.session
				.snapshot
				.extract_text(selection, self.session.accepted_revision);
			if !text.is_empty() {
				match self.clipboard.write(text) {
					Ok(()) => {
						self.status = "Copied selection".into();
						self.error = false;
					}
					Err(e) => {
						self.status = format!("Cannot copy: {e}");
						self.error = true;
					}
				}
				self.redraw();
			}
		}
	}
	pub(super) fn flush_settings(&mut self) {
		self.save_at = None;
		match self.settings_store.flush() {
			Ok(()) => self.settings_warning = None,
			Err(e) => {
				self.settings_warning =
					Some(format!("Cannot save settings: {e}"))
			}
		}
	}
}
