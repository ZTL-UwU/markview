//! Commands, selection gestures and clipboard actions.
use super::*;
impl App {
	pub(super) fn action(&mut self, action: Command) {
		match action {
			Command::Styles => {
				self.interaction.panel_open = true;
				self.interaction.styles_open = !self.interaction.styles_open;
				self.style_entries = crate::stylesheet::catalog(
					crate::stylesheet::directory().as_deref(),
					self.settings.style.as_deref(),
				);
				self.style_page = 0;
				self.interaction.focus = None;
				self.redraw();
				return;
			}
			Command::StylesFolder => {
				let result = crate::stylesheet::directory()
					.ok_or_else(|| anyhow::anyhow!("No stylesheet directory"))
					.and_then(|dir| {
						std::fs::create_dir_all(&dir)?;
						open::that_detached(dir)?;
						Ok(())
					});
				if let Err(e) = result {
					self.style_warning = Some(format!("{e:#}"));
				}
				self.redraw();
				return;
			}
			Command::StylePrev => {
				self.style_page = self.style_page.saturating_sub(1);
				self.redraw();
				return;
			}
			Command::StyleNext => {
				self.style_page += 1;
				self.redraw();
				return;
			}
			Command::StyleToggle(index)
			| Command::StyleUp(index)
			| Command::StyleDown(index) => {
				let Some(entry) = self.style_entries.get(index) else {
					return;
				};
				let mut ids = self.settings.style.clone().unwrap_or_default();
				let position = ids.iter().position(|id| id == &entry.id);
				match action {
					Command::StyleToggle(_) => {
						if position.is_some() {
							ids.retain(|id| id != &entry.id);
						} else if entry.error.is_none() {
							ids.insert(0, entry.id.clone());
						} else {
							return;
						}
					}
					Command::StyleUp(_) => {
						if let Some(i) = position.filter(|i| *i > 0) {
							ids.swap(i, i - 1);
						}
					}
					Command::StyleDown(_) => {
						if let Some(i) = position.filter(|i| i + 1 < ids.len())
						{
							ids.swap(i, i + 1);
						}
					}
					_ => {}
				}
				self.settings.style = Some(ids);
				self.setting_changed(Some(Setting::Theme));
				self.reload_styles();
				self.redraw();
				return;
			}
			Command::OpenConfig => {
				let result = self.settings_store.ensure_file().and_then(|()| {
					open::that_detached(self.settings_store.path().unwrap())
						.map_err(Into::into)
				});
				if let Err(error) = result {
					self.settings_warning =
						Some(format!("Cannot open settings: {error}"));
				}
				self.redraw();
				return;
			}
			Command::SystemTheme => {
				self.args.overrides.retain(|f| *f != Setting::Theme);
				self.args.theme = None;
				self.args.style = None;
				self.settings_store.follow_system();
				self.apply_saved_settings();
				self.save_at =
					Some(Instant::now() + Duration::from_millis(250));
				return;
			}
			Command::Settings => {
				self.interaction.panel_open = !self.interaction.panel_open;
				self.interaction.styles_open = false;
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.focus =
					self.interaction.panel_open.then_some(Command::Styles);
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
		self.args
			.overrides
			.retain(|f| field.is_some_and(|changed| changed != *f));
		if field.is_none() || field == Some(Setting::Theme) {
			self.args.theme = None;
			self.args.style = None;
			self.reload_styles();
		}
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

	pub(super) fn text_under_cursor(&self) -> bool {
		let geometry = self.view_geometry();
		if !geometry
			.clip()
			.contains(self.interaction.cursor.0, self.interaction.cursor.1)
		{
			return false;
		}
		let (x, y) = geometry.document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		self.session
			.snapshot
			.contains_text(x, y, &self.session.horizontal)
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
			Ok(()) => {
				self.settings_warning = None;
				if self.args.mode == Mode::Window {
					self.apply_saved_settings();
				}
			}
			Err(e) => {
				self.settings_warning =
					Some(format!("Cannot save settings: {e}"))
			}
		}
	}
	pub(super) fn apply_saved_settings(&mut self) {
		let previous = self.settings.clone();
		let options = self.options();
		self.settings = self.settings_store.settings();
		self.settings.stylesheet = previous.stylesheet.clone();
		if self.settings_store.theme_preference().is_none() {
			self.settings.theme = self
				.window
				.as_ref()
				.and_then(|w| system_theme(w))
				.unwrap_or_default();
		}
		for field in &self.args.overrides {
			self.settings.copy_field(&previous, *field);
		}
		self.reload_styles();
		if self.options() != options
			&& self.session.requested_options.as_ref() != Some(&self.options())
		{
			self.request(false);
		}
		if self.settings != previous {
			self.redraw();
		}
	}
}
