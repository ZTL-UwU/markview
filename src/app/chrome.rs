//! Reader controls and settings panel.
use super::*;
use markview_core::style::{CjkType, ColorField as C, Role, TextAppearance};
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
		out.extend(self.draw_tabs());
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
		let warning = if self.error
			&& !self
				.status_until
				.is_some_and(|until| until > Instant::now())
		{
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
			if self
				.status_until
				.is_some_and(|until| until > Instant::now())
			{
				&self.status
			} else {
				self.interaction
					.hover_image
					.as_deref()
					.or(self.interaction.hover.as_deref())
					.unwrap_or("")
			},
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
		if let Some(bar) = self.document_scrollbar() {
			let held = self
				.interaction
				.scrollbar
				.is_some_and(|drag| drag.target == ScrollbarAxis::Document);
			let (x, y) = self.interaction.cursor;
			// Hovering anywhere on the bar thickens it; only the thumb itself
			// takes the hover color.
			let (track, thumb) = bar.bars(held || bar.hit(x, y));
			out.push(Draw::Rect(
				track,
				Paint::Styled(Role::Scrollbar, C::Track),
			));
			out.push(Draw::Rect(
				thumb,
				Paint::Styled(
					Role::Scrollbar,
					if held || bar.on_thumb(x, y) {
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

	pub(super) fn tab_at_cursor(&mut self) -> Option<usize> {
		let point = self.interaction.cursor;
		self.tab_rects()
			.into_iter()
			.find(|(rect, _)| rect.contains(point.0, point.1))
			.map(|(_, index)| index)
	}

	pub(super) fn tab_close_at_cursor(&mut self) -> Option<usize> {
		let point = self.interaction.cursor;
		self.tab_rects().into_iter().find_map(|(rect, index)| {
			(rect.contains(point.0, point.1)
				&& point.0 >= rect.x + rect.w - 24.0)
				.then_some(index)
		})
	}

	fn tab_rects(&mut self) -> Vec<(Rect, usize)> {
		let (width, _, _) = self.dimensions();
		let right = toolbar_right_edge(&mut self.ui, width);
		let mut x = 10.0;
		let mut out = Vec::new();
		for (index, tab) in self.tabs.iter().enumerate() {
			let name = tab
				.path
				.file_name()
				.unwrap_or(tab.path.as_os_str())
				.to_string_lossy();
			let label_width = self.ui.text_width(&name, 12.0);
			let w = (label_width + 34.0).clamp(92.0, 240.0);
			if x + w > right - 4.0 {
				break;
			}
			out.push((
				Rect {
					x,
					y: 4.0,
					w,
					h: 32.0,
				},
				index,
			));
			x += w + 2.0;
		}
		out
	}

	fn draw_tabs(&mut self) -> Vec<Draw> {
		let mut out = Vec::new();
		for (rect, index) in self.tab_rects() {
			let active = index == self.active_tab;
			out.push(Draw::Box {
				rect,
				role: Role::Toolbar,
				radius: 0.0,
				border: 1.0,
				left_only: false,
			});
			let fill = if active {
				C::ActiveBackground
			} else if rect
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
			{
				C::HoverBackground
			} else {
				C::Background
			};
			out.push(Draw::Rect(rect, Paint::Styled(Role::Toolbar, fill)));
			let name = self.tabs[index]
				.path
				.file_name()
				.unwrap_or(self.tabs[index].path.as_os_str())
				.to_string_lossy();
			let name = self.ui.fit(&name, 12.0, rect.w - 26.0);
			out.extend(self.ui.label(
				&name,
				12.0,
				rect.x + 12.0,
				rect.y + 21.0,
				Paint::Styled(
					Role::Toolbar,
					if active { C::Color } else { C::Muted },
				),
			));
			out.extend(self.ui.label(
				"×",
				16.0,
				rect.x + rect.w - 19.0,
				rect.y + 21.0,
				Paint::Styled(Role::Toolbar, C::Muted),
			));
		}
		out
	}

	/// The document scrollbar while it is visible. Drawing and pointer
	/// handling share this geometry, so the thumb always agrees with what a
	/// press grabs.
	pub(super) fn document_scrollbar(&self) -> Option<Scrollbar> {
		if self.interaction.panel_open {
			return None;
		}
		let (width, height, _) = self.dimensions();
		let metrics = self.settings.stylesheet.scrollbar_metrics();
		let band = metrics.band();
		let track = Rect {
			x: width - band - 2.0,
			y: TOP,
			w: band,
			h: (height - TOP - BOTTOM).max(0.0),
		};
		Scrollbar::vertical(
			track,
			self.session.scroll,
			self.session.snapshot.height,
			self.viewport(),
			metrics,
		)
	}

	/// The horizontal scrollbar of one overflowing block, in window
	/// coordinates. The bar sits in the gutter the layout reserved below the
	/// block's content.
	pub(super) fn overflow_scrollbar(
		&self,
		block: usize,
		overflow: usize,
	) -> Option<Scrollbar> {
		let geometry = self.view_geometry();
		let placed = self.session.snapshot.blocks.get(block)?;
		let o = placed.layout.overflow.get(overflow)?;
		let metrics = self.settings.stylesheet.overflow_scrollbar_metrics();
		let track = Rect {
			x: geometry.left + o.rect.x,
			y: geometry.top - geometry.scroll + placed.y + o.rect.y + o.rect.h,
			w: o.rect.w,
			h: metrics.overflow_band(o.gutter),
		};
		Scrollbar::horizontal(
			track,
			self.session
				.horizontal
				.get(&(block, overflow))
				.copied()
				.unwrap_or(0.0),
			o.content_width,
			o.rect.w,
			metrics,
		)
	}

	/// The horizontal scrollbar under a window point, with the block and
	/// overflow index it belongs to.
	pub(super) fn overflow_scrollbar_at(
		&self,
		x: f32,
		y: f32,
	) -> Option<(usize, usize, Scrollbar)> {
		let geometry = self.view_geometry();
		if !geometry.clip().contains(x, y) {
			return None;
		}
		let (_, dy) = geometry.document_point(x, y);
		for (block, placed) in self.session.snapshot.blocks.iter().enumerate() {
			let local = dy - placed.y;
			if local < 0.0 || local > placed.layout.height {
				continue;
			}
			for overflow in 0..placed.layout.overflow.len() {
				if let Some(bar) = self.overflow_scrollbar(block, overflow)
					&& bar.hit(x, y)
				{
					return Some((block, overflow, bar));
				}
			}
		}
		None
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
	let h = 480.0_f32.min((height - 32.0).max(0.0));
	Rect {
		x: (width - w) / 2.0,
		y: (height - h) / 2.0,
		w,
		h,
	}
}
fn row_geometry(rect: Rect) -> (f32, f32) {
	let top = if rect.h < 360.0 { 60.0 } else { 94.0 };
	(top, (rect.h - top - 48.0) / 6.0)
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
			vec![
				("SC", Command::CjkType(CjkType::Sc)),
				("TC", Command::CjkType(CjkType::Tc)),
				("JP", Command::CjkType(CjkType::Jp)),
				("none", Command::CjkType(CjkType::None)),
			],
		]
		.into_iter()
		.enumerate()
		{
			let count = entries.len();
			let button_width =
				(168.0 - 4.0 * (count - 1) as f32) / count as f32;
			for (j, (label, action)) in entries.into_iter().enumerate() {
				buttons.push(Button {
					label,
					action,
					rect: Rect {
						x: rect.x + rect.w - 20.0 - 168.0
							+ j as f32 * (button_width + 4.0),
						y: rect.y + top + i as f32 * row,
						w: button_width,
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
	let mut x = toolbar_right_edge(shaper, width);
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

fn toolbar_right_edge(shaper: &mut TextShaper, width: f32) -> f32 {
	const TEXT_SIZE: f32 = 13.0;
	const HORIZONTAL_PADDING: f32 = 18.0;
	const GAP: f32 = 4.0;
	let old_appearance = shaper.appearance.clone();
	shaper.appearance =
		shaper.stylesheet.text(&TextAppearance::default(), Role::Ui);
	let button_widths = ["Open", "Settings"]
		.into_iter()
		.map(|label| shaper.text_width(label, TEXT_SIZE) + HORIZONTAL_PADDING)
		.collect::<Vec<_>>();
	shaper.appearance = old_appearance;
	width - button_widths.iter().sum::<f32>() - GAP - 16.0
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
			format!("CJK type · {:?}", settings.cjk_type),
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
				held_overflow: None,
				hovered_overflow: None,
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
