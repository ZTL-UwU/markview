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
	Rect {
		x: (width - 310.0).max(0.0),
		y: TOP,
		w: 310.0_f32.min(width),
		h: (height - TOP - BOTTOM).max(0.0),
	}
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
		return [
			(theme, Command::Theme),
			("Close", Command::Settings),
			("Smaller", Command::Smaller),
			("Larger", Command::Larger),
			("Narrower", Command::Narrower),
			("Wider", Command::Wider),
			(align, Command::Align),
			(hyphens, Command::Hyphens),
			("Reset defaults", Command::Reset),
		]
		.into_iter()
		.enumerate()
		.map(|(i, (label, action))| Button {
			label,
			action,
			rect: Rect {
				x: rect.x + 12.0 + (i % 2) as f32 * 145.0,
				y: TOP + 64.0 + (i / 2) as f32 * 28.0,
				w: 139.0,
				h: 26.0,
			},
		})
		.collect();
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
		out.push(Draw::Rect(panel_rect(width, height), Paint::Panel));
		let x = panel_rect(width, height).x + 12.0;
		out.extend(shaper.label(
			"Reading settings",
			17.0,
			x,
			TOP + 24.0,
			Paint::Text,
		));
		out.extend(shaper.label(
			&format!(
				"Size {:.0} · Width {:.0}",
				settings.font_size, settings.width
			),
			12.0,
			x,
			TOP + 46.0,
			Paint::Muted,
		));
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
			out.push(Draw::Rect(b.rect, Paint::Panel));
		}
		out.extend(shaper.label(
			b.label,
			13.0,
			b.rect.x + 9.0,
			b.rect.y + if interaction.panel_open { 18.0 } else { 22.0 },
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
		let settings = ReaderSettings::default();
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
			800.0,
			600.0,
		);
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		let horizontal = HashMap::new();
		let view = View {
			width: 1000,
			height: 750,
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
			.join("artifacts/refactor-ui.png");
		std::fs::create_dir_all(output.parent().unwrap())?;
		renderer.save_png(&target, &output)?;
		Ok(())
	}
}
