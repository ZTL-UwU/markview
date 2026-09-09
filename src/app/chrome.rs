//! Reader controls and settings panel.
use super::*;
use markview_core::style::{ColorField as C, Role, TextAppearance};
impl App {
	pub(super) fn buttons(&mut self) -> Vec<Button> {
		let (width, height, _) = self.dimensions();
		if self.interaction.styles_open {
			style_controls(
				&self.settings,
				&self.style_entries,
				self.style_page,
				width,
				height,
			)
		} else if self.interaction.panel_open {
			controls(
				&mut self.ui,
				&self.settings,
				self.interaction.panel_open,
				width,
				height,
			)
		} else {
			toolbar_controls(&mut self.ui, width)
		}
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
				Paint::Styled(Role::Toolbar, C::Background),
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Styled(Role::Toolbar, C::BorderColor),
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: height - BOTTOM,
					w: width,
					h: BOTTOM,
				},
				Paint::Styled(Role::Toolbar, C::Background),
			),
		];
		let selection = self.interaction.selection.filter(|s| {
			!s.is_empty()
				&& s.anchor.revision == self.session.accepted_revision
				&& s.focus.revision == self.session.accepted_revision
		});
		if self.interaction.selection_counts.map(|(s, _)| s) != selection {
			self.interaction.selection_counts = selection.map(|s| {
				(
					s,
					markview_core::text::TextCounts::of(
						&self
							.session
							.snapshot
							.extract_text(s, self.session.accepted_revision),
					),
				)
			});
		}
		let warning = if self.error {
			Some(self.status.as_str())
		} else {
			self.style_warning
				.as_deref()
				.or(self.settings_warning.as_deref())
		};
		out.extend(draw_footer(
			&mut self.ui,
			self.session.counts,
			self.interaction.selection_counts.map(|(_, counts)| counts),
			warning,
			self.interaction.hover.as_deref().unwrap_or(&self.status),
			width,
			height,
		));
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
			out.extend(self.ui.label(
				title,
				26.0,
				x,
				y,
				Paint::Styled(Role::Ui, C::Color),
			));
			out.extend(self.ui.label(
				"Drop a file here or press Ctrl+O.",
				15.0,
				x,
				y + 38.0,
				Paint::Styled(Role::Ui, C::Muted),
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
					x: width - 7.,
					y: TOP,
					w: 3.,
					h: track,
				},
				Paint::Styled(Role::Scrollbar, C::Track),
			));
			out.push(Draw::Rect(
				Rect {
					x: width - 7.0,
					y,
					w: 3.0,
					h,
				},
				Paint::Styled(
					Role::Scrollbar,
					if self.interaction.cursor.0 > width - 16. {
						C::ThumbHover
					} else {
						C::Thumb
					},
				),
			));
		}
		if self.interaction.styles_open {
			out.extend(draw_styles(
				&mut self.ui,
				&self.settings,
				&self.interaction,
				&self.style_entries,
				self.style_page,
				width,
				height,
			));
		} else {
			out.extend(draw_controls(
				&mut self.ui,
				&self.settings,
				&self.interaction,
				width,
				height,
			));
		}
		out
	}
}

fn draw_footer(
	shaper: &mut TextShaper,
	counts: markview_core::text::TextCounts,
	selected: Option<markview_core::text::TextCounts>,
	warning: Option<&str>,
	secondary: &str,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper.stylesheet.text(&TextAppearance::default(), Role::Ui),
		Role::Statusbar,
	);
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.0,
				y: height - BOTTOM,
				w: width,
				h: BOTTOM,
			},
			Paint::Styled(Role::Statusbar, C::Background),
		),
		Draw::Rect(
			Rect {
				x: 0.0,
				y: height - BOTTOM,
				w: width,
				h: 1.0,
			},
			Paint::Styled(Role::Statusbar, C::BorderColor),
		),
	];
	let mut text = format!("{} chars · {} words", counts.chars, counts.words);
	if let Some(selected) = selected {
		text.push_str(&format!(
			"    ·    Selected {} chars · {} words",
			selected.chars, selected.words
		));
	}
	let text = shaper.fit(&text, 11.0, width - 32.0);
	let used = shaper.text_width(&text, 11.0);
	out.extend(shaper.label(
		&text,
		11.0,
		16.0,
		height - 9.0,
		Paint::Styled(Role::Statusbar, C::Muted),
	));
	let available = width - used - 56.0;
	if !secondary.is_empty() && available >= 80.0 {
		out.extend(shaper.right_label(
			secondary,
			11.0,
			available,
			width - 16.0,
			height - 9.0,
			Paint::Styled(Role::Statusbar, C::Muted),
		));
	}
	if let Some(warning) = warning {
		out.push(Draw::Rect(
			Rect {
				x: 0.0,
				y: height - BOTTOM - 24.0,
				w: width,
				h: 24.0,
			},
			Paint::Styled(Role::Statusbar, C::Background),
		));
		let warning = shaper.fit(warning, 11.0, width - 32.0);
		out.extend(shaper.label(
			&warning,
			11.0,
			16.0,
			height - BOTTOM - 8.0,
			Paint::Styled(Role::Statusbar, C::Error),
		));
	}
	out
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
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	panel_open: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	if panel_open {
		let rect = panel_rect(width, height);
		let (top, row) = row_geometry(rect);
		let close_width = button_width(shaper, "Close", 13.0);
		let mut buttons = vec![Button {
			label: "Close",
			action: Command::Settings,
			rect: Rect {
				x: rect.x + rect.w - 20.0 - close_width,
				y: rect.y + 16.0,
				w: close_width,
				h: 28.0,
			},
		}];
		for (i, entries) in [
			vec![
				("System", Command::SystemTheme),
				("Styles…", Command::Styles),
			],
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
		let open_config_width =
			button_width(shaper, "Open settings.toml", 13.0);
		let reset_width = button_width(shaper, "Reset defaults", 13.0);
		for (label, action, x, w) in [
			(
				"Open settings.toml",
				Command::OpenConfig,
				20.0,
				open_config_width,
			),
			(
				"Reset defaults",
				Command::Reset,
				rect.w - 20.0 - reset_width,
				reset_width,
			),
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
	toolbar_controls(shaper, width)
}

fn button_width(shaper: &mut TextShaper, label: &str, size: f32) -> f32 {
	const HORIZONTAL_PADDING: f32 = 18.0;
	let old_appearance = shaper.appearance.clone();
	shaper.appearance =
		shaper.stylesheet.text(&TextAppearance::default(), Role::Ui);
	let width = shaper.text_width(label, size) + HORIZONTAL_PADDING;
	shaper.appearance = old_appearance;
	width
}

fn toolbar_controls(shaper: &mut TextShaper, width: f32) -> Vec<Button> {
	const TEXT_SIZE: f32 = 13.0;
	const HORIZONTAL_PADDING: f32 = 18.0;
	const GAP: f32 = 4.0;
	const RIGHT_INSET: f32 = 16.0;
	let entries = [("Open", Command::Open), ("Settings", Command::Settings)];
	let old_appearance = shaper.appearance.clone();
	shaper.appearance =
		shaper.stylesheet.text(&TextAppearance::default(), Role::Ui);
	let widths: Vec<f32> = entries
		.iter()
		.map(|(label, _)| {
			shaper.text_width(label, TEXT_SIZE) + HORIZONTAL_PADDING
		})
		.collect();
	shaper.appearance = old_appearance;
	let total_width = widths.iter().sum::<f32>()
		+ GAP * (entries.len() - 1) as f32
		+ RIGHT_INSET;
	let mut x = width - total_width;
	entries
		.into_iter()
		.zip(widths)
		.map(|((label, action), w)| {
			let rect = Rect {
				x,
				y: 6.0,
				w,
				h: 28.0,
			};
			x += w + GAP;
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
	shaper.appearance = shaper.stylesheet.text(
		&shaper.stylesheet.text(&TextAppearance::default(), Role::Ui),
		Role::Panel,
	);
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
		out.push(Draw::Box {
			rect,
			role: Role::Panel,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		out.push(Draw::Rect(
			Rect {
				x: rect.x,
				y: rect.y,
				w: 3.0,
				h: rect.h,
			},
			Paint::Styled(Role::Panel, C::BorderColor),
		));
		let x = rect.x + 20.0;
		out.extend(shaper.label(
			"Reading settings",
			22.0,
			x,
			rect.y + 36.0,
			Paint::Styled(Role::Panel, C::Color),
		));
		if rect.h >= 360.0 {
			out.extend(shaper.label(
				"Saved automatically · file changes apply live",
				12.0,
				x,
				rect.y + 61.0,
				Paint::Styled(Role::Panel, C::Muted),
			));
		}
		let (top, row) = row_geometry(rect);
		for (i, label) in [
			format!(
				"Styles · {}",
				settings
					.style
					.as_ref()
					.map(|ids| if ids.is_empty() {
						"Light base".into()
					} else {
						ids.join(", ")
					})
					.unwrap_or_else(|| "System".into())
			),
			format!("Text size · {:.1} px", settings.font_size),
			format!("Column width · {:.1} px", settings.width),
			"Alignment".into(),
			"English hyphenation".into(),
		]
		.iter()
		.enumerate()
		{
			let label = shaper.fit(label, 13., rect.w - 218.);
			out.extend(shaper.label(
				&label,
				13.0,
				x,
				rect.y + top + i as f32 * row + 19.0,
				Paint::Styled(Role::Panel, C::Color),
			));
		}
	}
	let buttons = if interaction.panel_open {
		controls(shaper, settings, true, width, height)
	} else {
		toolbar_controls(shaper, width)
	};
	for b in buttons {
		out.push(Draw::Box {
			rect: b.rect,
			role: Role::Button,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		if interaction.focus == Some(b.action) {
			out.push(Draw::Rect(
				b.rect,
				Paint::Styled(Role::Button, C::FocusColor),
			));
			out.push(Draw::Rect(
				Rect {
					x: b.rect.x + 1.0,
					y: b.rect.y + 1.0,
					w: b.rect.w - 2.0,
					h: b.rect.h - 2.0,
				},
				Paint::Styled(
					Role::Button,
					if interaction.pressed == Some(b.action) {
						C::ActiveBackground
					} else {
						C::Background
					},
				),
			));
		} else if b.rect.contains(interaction.cursor.0, interaction.cursor.1) {
			out.push(Draw::Rect(
				b.rect,
				Paint::Styled(Role::Button, C::HoverBackground),
			));
		} else if interaction.panel_open {
			out.push(Draw::Rect(
				b.rect,
				Paint::Styled(Role::Button, C::Background),
			));
		}
		let label_x =
			b.rect.x + (b.rect.w - shaper.text_width(b.label, 13.0)) / 2.0;
		out.extend(shaper.label(
			b.label,
			13.0,
			label_x,
			b.rect.y + b.rect.h / 2.0 + 5.0,
			Paint::Styled(Role::Button, C::Color),
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
			let mut shaper = TextShaper::new();
			let panel = panel_rect(width, height);
			for button in controls(
				&mut shaper,
				&ReaderSettings::default(),
				true,
				width,
				height,
			) {
				assert!(panel.contains(button.rect.x, button.rect.y));
				assert!(panel.contains(
					button.rect.x + button.rect.w,
					button.rect.y + button.rect.h
				));
				assert_ne!(button.action, Command::Open);
			}
			for button in controls(
				&mut shaper,
				&ReaderSettings::default(),
				false,
				width,
				height,
			) {
				assert!(button.rect.x + button.rect.w <= width);
			}
			let toolbar = controls(
				&mut shaper,
				&ReaderSettings::default(),
				false,
				width,
				height,
			);
			assert_eq!(
				toolbar.iter().map(|b| b.action).collect::<Vec<_>>(),
				vec![Command::Open, Command::Settings]
			);
			assert_eq!(toolbar[1].rect.x + toolbar[1].rect.w, width - 16.0);
			assert!(toolbar.iter().all(|b| b.rect.y + b.rect.h < TOP));
		}
	}
}

#[cfg(test)]
mod gpu_tests {
	use super::*;
	#[test]
	#[ignore = "requires a GPU; writes artifacts/refactor-ui.png"]
	fn settings_and_selection_frame() -> Result<()> {
		for (width, height, theme, panel_open, filename) in [
			(800.0, 600.0, Theme::Light, true, "refactor-ui.png"),
			(800.0, 600.0, Theme::Dark, true, "settings-dark.png"),
			(500.0, 300.0, Theme::Light, true, "settings-compact.png"),
			(800.0, 600.0, Theme::Light, false, "reader-chrome.png"),
			(
				500.0,
				300.0,
				Theme::Dark,
				false,
				"reader-chrome-compact.png",
			),
		] {
			let settings = ReaderSettings {
				theme,
				..Default::default()
			};
			let document = document::parse(
				"# Reading selections\n\nSelect **English**, 中文 and $x^2$ across lines.\n\n```rust\n\tlet answer = 42;\n```\n\n| A | B |\n|---|---|\n| one | two |\n",
			);
			let snapshot = LayoutEngine::new()
				.layout(&document, &settings.layout_options(width, false));
			let interaction = InteractionState {
				panel_open,
				focus: Some(if panel_open {
					Command::Larger
				} else {
					Command::Settings
				}),
				..Default::default()
			};
			let counts = markview_core::text::TextCounts::of(
				&snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
			);
			let mut overlay = vec![
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
			];
			overlay.extend(draw_footer(
				&mut TextShaper::new(),
				counts,
				Some(counts),
				None,
				"",
				width,
				height,
			));
			overlay.extend(draw_controls(
				&mut TextShaper::new(),
				&settings,
				&interaction,
				width,
				height,
			));
			let mut renderer = pollster::block_on(Renderer::new(None))?;
			let horizontal = HashMap::new();
			let view = View {
				hovered_link: None,
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
			if panel_open {
				let mut settings = settings.clone();
				settings.style = Some(vec!["paper".into(), "dark".into()]);
				let mut entries =
					crate::stylesheet::catalog(None, settings.style.as_deref());
				let paper =
					entries.iter_mut().find(|e| e.id == "paper").unwrap();
				paper.name = "纸与墨".into();
				paper.source = "/example/styles/paper.mvss.toml".into();
				paper.error = None;
				entries.push(crate::stylesheet::Entry {
					id: "invalid".into(),
					name: "Invalid stylesheet".into(),
					source: "/example/styles/invalid.mvss.toml".into(),
					error: Some("em.font: must not be empty".into()),
				});
				let overlay = draw_styles(
					&mut TextShaper::new(),
					&settings,
					&interaction,
					&entries,
					0,
					width,
					height,
				);
				let submission = renderer.render(
					&snapshot,
					&view,
					&overlay,
					&target.create_view(&Default::default()),
				)?;
				renderer.wait(Some(submission))?;
				renderer.save_png(
					&target,
					&output.with_file_name(format!("styles-{filename}")),
				)?;
			}
		}
		Ok(())
	}
}

fn style_rows(rect: Rect) -> usize {
	((rect.h - 142.) / 60.).floor().max(1.) as usize
}
fn style_order(
	settings: &ReaderSettings,
	entries: &[crate::stylesheet::Entry],
) -> Vec<usize> {
	let mut indices: Vec<_> = (0..entries.len()).collect();
	indices.sort_by_key(|i| {
		settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &entries[*i].id))
			.unwrap_or(usize::MAX)
	});
	indices
}
fn style_controls(
	settings: &ReaderSettings,
	entries: &[crate::stylesheet::Entry],
	page: usize,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = panel_rect(width, height);
	let rows = style_rows(r);
	let order = style_order(settings, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = vec![];
	for (label, action, x, w) in [
		("Back", Command::Styles, 20., 58.),
		("System", Command::SystemTheme, 86., 74.),
		("Close", Command::Settings, r.w - 78., 58.),
		("Open styles folder", Command::StylesFolder, 20., 146.),
	] {
		out.push(Button {
			label,
			action,
			rect: Rect {
				x: r.x + x,
				y: if action == Command::StylesFolder {
					r.y + r.h - 38.
				} else {
					r.y + 16.
				},
				w,
				h: 28.,
			},
		});
	}
	if page > 0 {
		out.push(Button {
			label: "Previous",
			action: Command::StylePrev,
			rect: Rect {
				x: r.x + r.w - 190.,
				y: r.y + r.h - 38.,
				w: 82.,
				h: 28.,
			},
		});
	}
	if (page + 1) * rows < order.len() {
		out.push(Button {
			label: "Next",
			action: Command::StyleNext,
			rect: Rect {
				x: r.x + r.w - 100.,
				y: r.y + r.h - 38.,
				w: 80.,
				h: 28.,
			},
		});
	}
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos = settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
		if e.error.is_none() || pos.is_some() {
			out.push(Button {
				label: if pos.is_some() { "Disable" } else { "Enable" },
				action: Command::StyleToggle(index),
				rect: Rect {
					x: r.x + r.w - 180.,
					y,
					w: 76.,
					h: 26.,
				},
			});
		}
		if let Some(pos) = pos {
			if pos > 0 {
				out.push(Button {
					label: "↑",
					action: Command::StyleUp(index),
					rect: Rect {
						x: r.x + r.w - 96.,
						y,
						w: 32.,
						h: 26.,
					},
				});
			}
			if settings
				.style
				.as_ref()
				.is_some_and(|ids| pos + 1 < ids.len())
			{
				out.push(Button {
					label: "↓",
					action: Command::StyleDown(index),
					rect: Rect {
						x: r.x + r.w - 58.,
						y,
						w: 32.,
						h: 26.,
					},
				});
			}
		}
	}
	out
}
fn draw_styles(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	entries: &[crate::stylesheet::Entry],
	page: usize,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper.stylesheet.text(&TextAppearance::default(), Role::Ui),
		Role::Panel,
	);
	let r = panel_rect(width, height);
	let rows = style_rows(r);
	let order = style_order(settings, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.,
				y: 0.,
				w: width,
				h: height,
			},
			Paint::Scrim,
		),
		Draw::Box {
			rect: r,
			role: Role::Panel,
			radius: 0.,
			border: 1.,
			left_only: false,
		},
	];
	let summary = if settings.style.is_none() {
		"Stylesheets · following system"
	} else {
		"Stylesheets · highest priority first"
	};
	out.extend(shaper.label(
		summary,
		13.,
		r.x + 20.,
		r.y + 66.,
		Paint::Styled(Role::Panel, C::Color),
	));
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos = settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
		let title = format!(
			"{}{} ({})",
			pos.map(|p| format!("{}. ", p + 1)).unwrap_or_default(),
			e.name,
			e.id
		);
		let title = shaper.fit(&title, 13., r.w - 212.);
		out.extend(shaper.label(
			&title,
			13.,
			r.x + 20.,
			y + 18.,
			Paint::Styled(Role::Panel, C::Color),
		));
		if e.error.is_some() && pos.is_none() {
			let rect = Rect {
				x: r.x + r.w - 180.,
				y,
				w: 76.,
				h: 26.,
			};
			out.push(Draw::Rect(
				rect,
				Paint::Styled(Role::Button, C::Background),
			));
			out.extend(shaper.label(
				"Invalid",
				12.,
				rect.x + 7.,
				rect.y + 18.,
				Paint::Styled(Role::Button, C::DisabledColor),
			));
		}
		let detail = e.error.as_deref().unwrap_or(&e.source);
		let detail = shaper.fit(detail, 10., r.w - 40.);
		out.extend(shaper.label(
			&detail,
			10.,
			r.x + 20.,
			y + 40.,
			Paint::Styled(
				Role::Panel,
				if e.error.is_some() {
					C::Error
				} else {
					C::Muted
				},
			),
		));
	}
	for b in style_controls(settings, entries, page, width, height) {
		out.push(Draw::Box {
			rect: b.rect,
			role: Role::Button,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		let hovered =
			b.rect.contains(interaction.cursor.0, interaction.cursor.1);
		out.push(Draw::Rect(
			b.rect,
			Paint::Styled(
				Role::Button,
				if interaction.pressed == Some(b.action) {
					C::ActiveBackground
				} else if hovered {
					C::HoverBackground
				} else {
					C::Background
				},
			),
		));
		if interaction.focus == Some(b.action) {
			for rect in [
				Rect { h: 1., ..b.rect },
				Rect {
					y: b.rect.y + b.rect.h - 1.,
					h: 1.,
					..b.rect
				},
				Rect { w: 1., ..b.rect },
				Rect {
					x: b.rect.x + b.rect.w - 1.,
					w: 1.,
					..b.rect
				},
			] {
				out.push(Draw::Rect(
					rect,
					Paint::Styled(Role::Button, C::FocusColor),
				));
			}
		}
		let label_x =
			b.rect.x + (b.rect.w - shaper.text_width(b.label, 12.)) / 2.0;
		out.extend(shaper.label(
			b.label,
			12.,
			label_x,
			b.rect.y + 18.,
			Paint::Styled(Role::Button, C::Color),
		));
	}
	out
}

#[cfg(test)]
mod stylesheet_tests {
	use super::*;
	#[test]
	fn stylesheet_controls_fit_and_cannot_enable_invalid_entries() {
		let entries = vec![
			crate::stylesheet::Entry {
				id: "a".into(),
				name: "A".into(),
				source: "test".into(),
				error: None,
			},
			crate::stylesheet::Entry {
				id: "broken".into(),
				name: "Broken".into(),
				source: "test".into(),
				error: Some("Invalid".into()),
			},
		];
		let settings = ReaderSettings {
			style: Some(vec!["a".into()]),
			..Default::default()
		};
		for (w, h) in [(500., 300.), (820., 600.)] {
			let panel = panel_rect(w, h);
			let buttons = style_controls(&settings, &entries, 0, w, h);
			assert!(buttons.iter().all(|b| panel.contains(b.rect.x, b.rect.y)
				&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)));
			assert!(
				!buttons.iter().any(|b| b.action == Command::StyleToggle(1))
			);
		}
	}
}
