//! Reader controls and settings panel.
use super::*;
impl App {
	pub(super) fn buttons(&self) -> Vec<Button> {
		let (width, height, _) = self.dimensions();
		controls(&self.settings, self.interaction.panel_open, width, height)
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
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

		let max_chars = (width / 7.0) as usize;
		let status: String = if self.error {
			&self.status
		} else {
			self.settings_warning.as_ref().unwrap_or(&self.status)
		}
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
		if let Some(url) = self.interaction.hover.clone() {
			out.extend(self.ui.right_label(
				&url,
				11.0,
				(width * 0.6).max(120.0),
				width - 20.0,
				height - 10.0,
				Paint::Muted,
			));
		}
		if self.session.snapshot.blocks.is_empty() {
			let x = ((width - 440.0) / 2.0).max(24.0);
			let y = (height * 0.4).max(110.0);
			let title = if self.session.path.is_none() {
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
		if !self.interaction.panel_open
			&& self.session.snapshot.height > viewport
		{
			let track = height - TOP - BOTTOM;
			let h = (track * viewport / self.session.snapshot.height).max(20.0);
			let y = TOP
				+ self.session.scroll
					/ (self.session.snapshot.height - viewport)
					* (track - h);
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
		out.extend(draw_controls(
			&mut self.ui,
			&self.settings,
			&self.interaction,
			width,
			height,
		));
		out
	}
}

pub(super) fn panel_rect(width: f32, height: f32) -> Rect {
	let w = 540.0_f32.min((width - 32.0).max(0.0));
	let h = 440.0_f32.min((height - 32.0).max(0.0));
	Rect {
		x: (width - w) / 2.0,
		y: (height - h) / 2.0,
		w,
		h,
	}
}
fn row_geometry(rect: Rect) -> (f32, f32) {
	let top = if rect.h < 360.0 { 60.0 } else { 94.0 };
	(top, (rect.h - top - 48.0) / 5.0)
}
fn controls(
	settings: &ReaderSettings,
	panel_open: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let theme = if settings.theme == Theme::Light {
		"Dark"
	} else {
		"Light"
	};
	let align = if settings.justify { "Justify" } else { "Left" };
	let hyphens = if settings.hyphenate {
		"Hyphens"
	} else {
		"No hyph."
	};
	if panel_open {
		let rect = panel_rect(width, height);
		let (top, row) = row_geometry(rect);
		let mut buttons = vec![Button {
			label: "Close",
			action: Command::Settings,
			rect: Rect {
				x: rect.x + rect.w - 78.0,
				y: rect.y + 16.0,
				w: 58.0,
				h: 28.0,
			},
		}];
		for (i, entries) in [
			vec![("System", Command::SystemTheme), (theme, Command::Theme)],
			vec![("A−", Command::Smaller), ("A+", Command::Larger)],
			vec![("W−", Command::Narrower), ("W+", Command::Wider)],
			vec![(
				if settings.justify {
					"Justified"
				} else {
					"Left aligned"
				},
				Command::Align,
			)],
			vec![(
				if settings.hyphenate { "On" } else { "Off" },
				Command::Hyphens,
			)],
		]
		.into_iter()
		.enumerate()
		{
			let count = entries.len();
			for (j, (label, action)) in entries.into_iter().enumerate() {
				buttons.push(Button {
					label,
					action,
					rect: Rect {
						x: rect.x + rect.w - 188.0 + j as f32 * 88.0,
						y: rect.y + top + i as f32 * row,
						w: if count == 1 { 168.0 } else { 80.0 },
						h: (row - 4.0).min(32.0),
					},
				});
			}
		}
		for (label, action, x, w) in [
			("Open settings.toml", Command::OpenConfig, 20.0, 154.0),
			("Reset defaults", Command::Reset, rect.w - 142.0, 122.0),
		] {
			buttons.push(Button {
				label,
				action,
				rect: Rect {
					x: rect.x + x,
					y: rect.y + rect.h - 38.0,
					w,
					h: 28.0,
				},
			});
		}
		return buttons;
	}
	let entries = if width < 820.0 {
		vec![
			("Open", 60.0, Command::Open),
			("Settings", 82.0, Command::Settings),
		]
	} else {
		vec![
			("Open", 60.0, Command::Open),
			(theme, 60.0, Command::Theme),
			("A−", 40.0, Command::Smaller),
			("A+", 40.0, Command::Larger),
			("W−", 40.0, Command::Narrower),
			("W+", 40.0, Command::Wider),
			(align, 68.0, Command::Align),
			(hyphens, 82.0, Command::Hyphens),
			("Settings", 82.0, Command::Settings),
		]
	};
	let mut x = if width >= 720.0 { 126.0 } else { 12.0 };
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

fn draw_controls(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	let mut out = Vec::new();
	if interaction.panel_open {
		let rect = panel_rect(width, height);
		out.push(Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: width,
				h: height,
			},
			Paint::Scrim,
		));
		out.push(Draw::Rect(
			Rect {
				x: rect.x - 5.0,
				y: rect.y + 6.0,
				w: rect.w + 10.0,
				h: rect.h + 4.0,
			},
			Paint::Shadow,
		));
		out.push(Draw::Rect(rect, Paint::Glass));
		out.push(Draw::Rect(
			Rect {
				x: rect.x,
				y: rect.y,
				w: 3.0,
				h: rect.h,
			},
			Paint::Accent,
		));
		let x = rect.x + 20.0;
		out.extend(shaper.label(
			"Reading settings",
			22.0,
			x,
			rect.y + 36.0,
			Paint::Text,
		));
		if rect.h >= 360.0 {
			out.extend(shaper.label(
				"Saved automatically · file changes apply live",
				12.0,
				x,
				rect.y + 61.0,
				Paint::Muted,
			));
		}
		let (top, row) = row_geometry(rect);
		for (i, label) in [
			format!(
				"Theme · {}",
				if settings.theme == Theme::Light {
					"Light"
				} else {
					"Dark"
				}
			),
			format!("Text size · {:.1} px", settings.font_size),
			format!("Column width · {:.1} px", settings.width),
			"Alignment".into(),
			"English hyphenation".into(),
		]
		.iter()
		.enumerate()
		{
			out.extend(shaper.label(
				label,
				13.0,
				x,
				rect.y + top + i as f32 * row + 19.0,
				Paint::Text,
			));
		}
	}
	for b in controls(settings, interaction.panel_open, width, height) {
		if interaction.focus == Some(b.action) {
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
		} else if b.rect.contains(interaction.cursor.0, interaction.cursor.1) {
			out.push(Draw::Rect(b.rect, Paint::Border));
		} else if interaction.panel_open {
			out.push(Draw::Rect(b.rect, Paint::Panel));
		}
		out.extend(shaper.label(
			b.label,
			13.0,
			b.rect.x + 9.0,
			b.rect.y + b.rect.h / 2.0 + 5.0,
			Paint::Text,
		));
	}
	out
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn controls_fit_minimum_window_and_panel_focus_has_no_document_actions() {
		for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)]
		{
			let panel = panel_rect(width, height);
			for button in
				controls(&ReaderSettings::default(), true, width, height)
			{
				assert!(panel.contains(button.rect.x, button.rect.y));
				assert!(panel.contains(
					button.rect.x + button.rect.w,
					button.rect.y + button.rect.h
				));
				assert_ne!(button.action, Command::Open);
			}
			for button in
				controls(&ReaderSettings::default(), false, width, height)
			{
				assert!(button.rect.x + button.rect.w <= width);
			}
		}
	}
}

#[cfg(test)]
mod gpu_tests {
	use super::*;
	#[test]
	#[ignore = "requires a GPU; writes artifacts/refactor-ui.png"]
	fn settings_and_selection_frame() -> Result<()> {
		for (width, height, theme, filename) in [
			(800.0, 600.0, Theme::Light, "refactor-ui.png"),
			(800.0, 600.0, Theme::Dark, "settings-dark.png"),
			(500.0, 300.0, Theme::Light, "settings-compact.png"),
		] {
			let settings = ReaderSettings {
				theme,
				..Default::default()
			};
			let document = document::parse(
				"# Reading selections\n\nSelect **English**, 中文 and $x^2$ across lines.\n\n```rust\n\tlet answer = 42;\n```\n\n| A | B |\n|---|---|\n| one | two |\n",
			);
			let snapshot = LayoutEngine::new()
				.layout(&document, &settings.layout_options(800.0, false));
			let interaction = InteractionState {
				panel_open: true,
				focus: Some(Command::Larger),
				..Default::default()
			};
			let overlay = draw_controls(
				&mut TextShaper::new(),
				&settings,
				&interaction,
				width,
				height,
			);
			let mut renderer = pollster::block_on(Renderer::new(None))?;
			let horizontal = HashMap::new();
			let view = View {
				width: (width * 1.25) as u32,
				height: (height * 1.25) as u32,
				scale: 1.25,
				left: 20.0,
				top: TOP + 10.0,
				bottom: BOTTOM + 10.0,
				scroll: 0.0,
				theme: settings.theme,
				horizontal: &horizontal,
				selection: snapshot.select_all(1),
				revision: 1,
			};
			let target = renderer.offscreen(view.width, view.height);
			let submission = renderer.render(
				&snapshot,
				&view,
				&overlay,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("artifacts")
				.join(filename);
			std::fs::create_dir_all(output.parent().unwrap())?;
			renderer.save_png(&target, &output)?;
		}
		Ok(())
	}
}
