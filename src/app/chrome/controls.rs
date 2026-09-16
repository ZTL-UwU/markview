use super::super::{Button, ChromeFrame, TITLE};
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	settings::ReaderSettings,
	state::{Command, InteractionState},
};
use markview_core::style::{
	CjkType, ColorField as C, Condition, TextAppearance,
};
pub(in crate::app) const BUTTON_RADIUS: f32 = 4.0;
pub(in crate::app) const PANEL_RADIUS: f32 = 8.0;
pub(in crate::app) const WINDOW_CONTROL: f32 = 36.0;
const TITLE_BUTTON: f32 = 24.0;

pub(in crate::app) fn panel_rect(width: f32, height: f32) -> Rect {
	let w = 540.0_f32.min((width - 32.0).max(0.0));
	let h = 480.0_f32.min((height - 32.0).max(0.0));
	Rect {
		x: (width - w) / 2.0,
		y: (height - h) / 2.0,
		w,
		h,
	}
}

pub(in crate::app) fn title_bar_leading() -> f32 {
	if cfg!(target_os = "macos") {
		78.0
	} else {
		12.0
	}
}

fn row_geometry(rect: Rect) -> (f32, f32) {
	let top = if rect.h < 360.0 { 52.0 } else { 56.0 };
	(top, (rect.h - top - 48.0) / 7.0)
}

fn btn(
	label: &'static str,
	action: Command,
	x: f32,
	y: f32,
	w: f32,
	h: f32,
	selected: bool,
) -> Button {
	Button {
		label,
		action,
		selected,
		rect: Rect { x, y, w, h },
	}
}

pub(super) fn panel_controls(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let rect = panel_rect(width, height);
	let (top, row) = row_geometry(rect);
	let mut buttons = vec![btn(
		"×",
		Command::Settings,
		rect.x + rect.w - 42.0,
		rect.y + 10.0,
		32.0,
		32.0,
		false,
	)];
	for (i, entries) in [
		vec![
			("System", Command::SystemTheme, settings.style.is_none()),
			("Styles…", Command::Styles, false),
		],
		vec![
			("A−", Command::Smaller, false),
			("A+", Command::Larger, false),
		],
		vec![
			("W−", Command::Narrower, false),
			("W+", Command::Wider, false),
		],
		vec![(
			if settings.justify {
				"Justified"
			} else {
				"Left aligned"
			},
			Command::Align,
			settings.justify,
		)],
		vec![(
			if settings.hyphenate { "On" } else { "Off" },
			Command::Hyphens,
			settings.hyphenate,
		)],
		vec![
			("Off", Command::Indent(0), settings.paragraph_indent == 0.0),
			("1 em", Command::Indent(1), settings.paragraph_indent == 1.0),
			("2 em", Command::Indent(2), settings.paragraph_indent == 2.0),
			("3 em", Command::Indent(3), settings.paragraph_indent == 3.0),
		],
		vec![
			(
				"SC",
				Command::CjkType(CjkType::Sc),
				settings.cjk_type == CjkType::Sc,
			),
			(
				"TC",
				Command::CjkType(CjkType::Tc),
				settings.cjk_type == CjkType::Tc,
			),
			(
				"JP",
				Command::CjkType(CjkType::Jp),
				settings.cjk_type == CjkType::Jp,
			),
			(
				"none",
				Command::CjkType(CjkType::None),
				settings.cjk_type == CjkType::None,
			),
		],
	]
	.into_iter()
	.enumerate()
	{
		let count = entries.len();
		let button_width = (168.0 - 4.0 * (count - 1) as f32) / count as f32;
		for (j, (label, action, selected)) in entries.into_iter().enumerate() {
			buttons.push(btn(
				label,
				action,
				rect.x + rect.w - 20.0 - 168.0
					+ j as f32 * (button_width + 4.0),
				rect.y + top + i as f32 * row,
				button_width,
				(row - 4.0).min(28.0),
				selected,
			));
		}
	}
	let open_config_width = button_width(shaper, "Open settings.toml", 12.0);
	let reset_width = button_width(shaper, "Reset defaults", 12.0);
	buttons.push(btn(
		"Open settings.toml",
		Command::OpenConfig,
		rect.x + 20.0,
		rect.y + rect.h - 38.0,
		open_config_width,
		28.0,
		false,
	));
	buttons.push(btn(
		"Reset defaults",
		Command::Reset,
		rect.x + rect.w - 20.0 - reset_width,
		rect.y + rect.h - 38.0,
		reset_width,
		28.0,
		false,
	));
	buttons
}

fn button_width(shaper: &mut TextShaper, label: &str, size: f32) -> f32 {
	const HORIZONTAL_PADDING: f32 = 12.0;
	let old_appearance = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let width = shaper.text_width(label, size) + HORIZONTAL_PADDING;
	shaper.appearance = old_appearance;
	width
}

pub(super) fn toolbar_controls(
	shaper: &mut TextShaper,
	width: f32,
	frame: ChromeFrame,
) -> Vec<Button> {
	const TEXT_SIZE: f32 = 12.0;
	const HORIZONTAL_PADDING: f32 = 12.0;
	const GAP: f32 = 2.0;
	let entries = [("Open", Command::Open), ("Settings", Command::Settings)];
	let old_appearance = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let widths: Vec<f32> = entries
		.iter()
		.map(|(label, _)| {
			shaper.text_width(label, TEXT_SIZE) + HORIZONTAL_PADDING
		})
		.collect();
	shaper.appearance = old_appearance;
	let y = (TITLE - TITLE_BUTTON) / 2.0;
	let mut x = toolbar_left(shaper, width, frame);
	let mut buttons: Vec<Button> = entries
		.into_iter()
		.zip(widths)
		.map(|((label, action), w)| {
			let button = btn(label, action, x, y, w, TITLE_BUTTON, false);
			x += w + GAP;
			button
		})
		.collect();
	buttons.extend(window_controls(width, frame));
	buttons
}

pub(in crate::app) fn toolbar_left(
	shaper: &mut TextShaper,
	width: f32,
	frame: ChromeFrame,
) -> f32 {
	const TEXT_SIZE: f32 = 12.0;
	const HORIZONTAL_PADDING: f32 = 12.0;
	const GAP: f32 = 2.0;
	let old_appearance = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let button_widths = ["Open", "Settings"]
		.into_iter()
		.map(|label| shaper.text_width(label, TEXT_SIZE) + HORIZONTAL_PADDING)
		.collect::<Vec<_>>();
	shaper.appearance = old_appearance;
	width
		- frame.control_width()
		- button_widths.iter().sum::<f32>()
		- GAP - 8.0
}

fn window_controls(width: f32, frame: ChromeFrame) -> Vec<Button> {
	if !frame.client {
		return Vec::new();
	}
	let mut x = width - frame.control_width();
	let mut out = Vec::new();
	for (enabled, label, action) in [
		(frame.minimize, "–", Command::Minimize),
		(frame.maximize, "□", Command::Maximize),
		(true, "×", Command::CloseWindow),
	] {
		if !enabled {
			continue;
		}
		out.push(btn(label, action, x, 0.0, WINDOW_CONTROL, TITLE, false));
		x += WINDOW_CONTROL;
	}
	out
}

pub(super) fn draw_controls(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	frame: ChromeFrame,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let mut out = Vec::new();
	if interaction.panel_open && !interaction.styles_open {
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
		out.push(Draw::Box {
			rect,
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			fill: C::Background,
			radius: PANEL_RADIUS,
			border: 1.,
			left_only: false,
		});
		let x = rect.x + 20.0;
		out.extend(shaper.label(
			"Settings",
			18.0,
			x,
			rect.y + 32.0,
			Paint::Styled(Condition::Panel, C::Color),
		));
		out.push(Draw::Rect(
			Rect {
				x: rect.x,
				y: rect.y + 48.0,
				w: rect.w,
				h: 1.0,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		));
		out.push(Draw::Rect(
			Rect {
				x: rect.x,
				y: rect.y + rect.h - 48.0,
				w: rect.w,
				h: 1.0,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		));
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
			format!(
				"Paragraph indent · {}",
				if settings.paragraph_indent > 0.0 {
					format!("{} em", settings.paragraph_indent)
				} else {
					"off".into()
				}
			),
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
				Paint::Styled(Condition::Panel, C::Color),
			));
		}
	}
	for b in toolbar_controls(shaper, width, frame) {
		out.extend(paint_button(shaper, &b, interaction, false, 12.0));
	}
	if interaction.panel_open && !interaction.styles_open {
		for b in panel_controls(shaper, settings, width, height) {
			out.extend(paint_button(shaper, &b, interaction, false, 12.0));
		}
	}
	out
}

pub(super) fn paint_button(
	shaper: &mut TextShaper,
	button: &Button,
	interaction: &InteractionState,
	always_fill: bool,
	size: f32,
) -> Vec<Draw> {
	let hovered = button
		.rect
		.contains(interaction.cursor.0, interaction.cursor.1);
	let focused = interaction.focus == Some(button.action);
	let pressed = interaction.pressed == Some(button.action);
	let window_control = matches!(
		button.action,
		Command::Minimize | Command::Maximize | Command::CloseWindow
	);
	let fill = if pressed {
		Some(C::ActiveBackground)
	} else if hovered && button.action == Command::CloseWindow {
		Some(C::Error)
	} else if hovered {
		Some(C::HoverBackground)
	} else if button.selected {
		Some(C::ActiveBackground)
	} else if always_fill {
		Some(C::Background)
	} else {
		None
	};
	let mut out = Vec::new();
	if fill.is_some() || focused {
		out.push(Draw::Box {
			rect: button.rect,
			chain: Condition::Button.chain(),
			condition: Condition::Button,
			fill: fill.unwrap_or(C::Background),
			radius: if window_control { 0.0 } else { BUTTON_RADIUS },
			border: if focused { 1. } else { 0. },
			left_only: false,
		});
	}
	let size = if button.label == "×" && !window_control {
		16.0
	} else {
		size
	};
	let label_x = button.rect.x
		+ (button.rect.w - shaper.text_width(button.label, size)) / 2.0;
	out.extend(shaper.label(
		button.label,
		size,
		label_x,
		button.rect.y + button.rect.h / 2.0 + size * 0.38,
		Paint::Styled(Condition::Button, C::Color),
	));
	out
}
#[cfg(test)]
mod tests {
	use super::*;
	use crate::app::TITLE;
	use crate::layout::Draw;
	#[test]
	fn panel_exposes_first_line_indent_presets() {
		let mut shaper = TextShaper::new();
		let buttons = panel_controls(
			&mut shaper,
			&ReaderSettings::default(),
			1200.0,
			800.0,
		);
		for (em, label) in [(0, "Off"), (1, "1 em"), (2, "2 em"), (3, "3 em")] {
			let button = buttons
				.iter()
				.find(|b| b.action == Command::Indent(em))
				.expect("indent preset");
			assert_eq!(button.label, label);
			assert_eq!(button.selected, em == 0);
		}
	}
	#[test]
	fn settings_modal_uses_dismiss_and_selected_toggles() {
		let mut shaper = TextShaper::new();
		let settings = ReaderSettings {
			justify: true,
			hyphenate: true,
			paragraph_indent: 2.0,
			cjk_type: CjkType::Jp,
			..Default::default()
		};
		let buttons = panel_controls(&mut shaper, &settings, 1200.0, 800.0);
		let close = buttons
			.iter()
			.find(|b| b.action == Command::Settings)
			.expect("dismiss");
		assert_eq!(close.label, "×");
		assert!(
			buttons
				.iter()
				.any(|b| b.action == Command::Align && b.selected)
		);
		assert!(
			buttons
				.iter()
				.any(|b| b.action == Command::Hyphens && b.selected)
		);
		assert!(
			buttons
				.iter()
				.any(|b| b.action == Command::Indent(2) && b.selected)
		);
		assert!(
			buttons.iter().any(
				|b| b.action == Command::CjkType(CjkType::Jp) && b.selected
			)
		);
		assert!(
			buttons
				.iter()
				.any(|b| b.action == Command::SystemTheme && b.selected)
		);
	}
	#[test]
	fn controls_fit_minimum_window_and_panel_focus_has_no_document_actions() {
		for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)]
		{
			let mut shaper = TextShaper::new();
			let panel = panel_rect(width, height);
			for button in panel_controls(
				&mut shaper,
				&ReaderSettings::default(),
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
			let toolbar =
				toolbar_controls(&mut shaper, width, ChromeFrame::default());
			assert_eq!(
				toolbar.iter().map(|b| b.action).collect::<Vec<_>>(),
				vec![Command::Open, Command::Settings]
			);
			assert!(
				(toolbar[1].rect.x + toolbar[1].rect.w - (width - 8.0)).abs()
					< 0.01
			);
			assert!(toolbar.iter().all(|b| b.rect.y + b.rect.h <= TITLE));
		}
	}
	#[test]
	fn client_chrome_places_window_controls_on_the_title_bar() {
		let mut shaper = TextShaper::new();
		let frame = ChromeFrame {
			client: true,
			minimize: true,
			maximize: true,
		};
		let buttons = toolbar_controls(&mut shaper, 800.0, frame);
		assert_eq!(
			buttons.iter().map(|b| b.action).collect::<Vec<_>>(),
			vec![
				Command::Open,
				Command::Settings,
				Command::Minimize,
				Command::Maximize,
				Command::CloseWindow
			]
		);
		assert!(
			(buttons.last().unwrap().rect.x + WINDOW_CONTROL - 800.0).abs()
				< 0.01
		);
		assert!(buttons.iter().all(|b| b.rect.y + b.rect.h <= TITLE));
	}
	#[test]
	fn toolbar_buttons_are_ghost_until_hovered() {
		let mut shaper = TextShaper::new();
		let buttons =
			toolbar_controls(&mut shaper, 800.0, ChromeFrame::default());
		let idle = paint_button(
			&mut shaper,
			&buttons[0],
			&InteractionState::default(),
			false,
			12.0,
		);
		assert!(!idle.iter().any(|d| matches!(d, Draw::Box { .. })));
		let hovered = paint_button(
			&mut shaper,
			&buttons[0],
			&InteractionState {
				cursor: (buttons[0].rect.x + 1.0, buttons[0].rect.y + 1.0),
				..Default::default()
			},
			false,
			12.0,
		);
		assert!(hovered.iter().any(|d| matches!(
			d,
			Draw::Box {
				fill: C::HoverBackground,
				..
			}
		)));
	}
	#[test]
	fn settings_modal_has_no_offset_shadow() {
		let mut shaper = TextShaper::new();
		let draws = draw_controls(
			&mut shaper,
			&ReaderSettings::default(),
			&InteractionState {
				panel_open: true,
				..Default::default()
			},
			ChromeFrame::default(),
			800.0,
			600.0,
		);
		assert!(
			!draws
				.iter()
				.any(|d| matches!(d, Draw::Rect(_, Paint::Shadow)))
		);
		assert!(
			draws
				.iter()
				.any(|d| matches!(d, Draw::Rect(_, Paint::Scrim)))
		);
	}
	#[test]
	fn selected_toggle_uses_active_fill() {
		let mut shaper = TextShaper::new();
		let button = btn("On", Command::Hyphens, 80.0, 80.0, 40.0, 24.0, true);
		let draws = paint_button(
			&mut shaper,
			&button,
			&InteractionState::default(),
			false,
			12.0,
		);
		assert!(draws.iter().any(|d| matches!(
			d,
			Draw::Box {
				fill: C::ActiveBackground,
				..
			}
		)));
	}
}
