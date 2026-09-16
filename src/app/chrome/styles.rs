use super::super::Button;
use super::controls::panel_rect;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	settings::ReaderSettings,
	state::{Command, InteractionState},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};
fn style_rows(rect: Rect) -> usize {
	((rect.h - 128.) / 60.).floor().max(1.) as usize
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
pub(super) fn style_controls(
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
	let mut out = vec![
		btn(
			"Back",
			Command::Styles,
			r.x + 12.,
			r.y + 10.,
			58.,
			32.,
			false,
		),
		btn(
			"×",
			Command::Settings,
			r.x + r.w - 42.,
			r.y + 10.,
			32.,
			32.,
			false,
		),
		btn(
			"System",
			Command::SystemTheme,
			r.x + r.w - 20. - 74.,
			r.y + 56.,
			74.,
			28.,
			settings.style.is_none(),
		),
		btn(
			"Open styles folder",
			Command::StylesFolder,
			r.x + 20.,
			r.y + r.h - 38.,
			146.,
			28.,
			false,
		),
	];
	if page > 0 {
		out.push(btn(
			"Previous",
			Command::StylePrev,
			r.x + r.w - 190.,
			r.y + r.h - 38.,
			82.,
			28.,
			false,
		));
	}
	if (page + 1) * rows < order.len() {
		out.push(btn(
			"Next",
			Command::StyleNext,
			r.x + r.w - 100.,
			r.y + r.h - 38.,
			80.,
			28.,
			false,
		));
	}
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos = settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 92. + row as f32 * 60.;
		if e.error.is_none() || pos.is_some() {
			out.push(btn(
				if pos.is_some() { "Disable" } else { "Enable" },
				Command::StyleToggle(index),
				r.x + r.w - 180.,
				y,
				76.,
				26.,
				pos.is_some(),
			));
		}
		if let Some(pos) = pos {
			if pos > 0 {
				out.push(btn(
					"↑",
					Command::StyleUp(index),
					r.x + r.w - 96.,
					y,
					32.,
					26.,
					false,
				));
			}
			if settings
				.style
				.as_ref()
				.is_some_and(|ids| pos + 1 < ids.len())
			{
				out.push(btn(
					"↓",
					Command::StyleDown(index),
					r.x + r.w - 58.,
					y,
					32.,
					26.,
					false,
				));
			}
		}
	}
	out
}
pub(super) fn draw_styles(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	entries: &[crate::stylesheet::Entry],
	page: usize,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
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
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			fill: C::Background,
			radius: super::controls::PANEL_RADIUS,
			border: 1.,
			left_only: false,
		},
		Draw::Rect(
			Rect {
				x: r.x,
				y: r.y + 48.,
				w: r.w,
				h: 1.,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		),
		Draw::Rect(
			Rect {
				x: r.x,
				y: r.y + r.h - 48.,
				w: r.w,
				h: 1.,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		),
	];
	out.extend(shaper.label(
		"Styles",
		18.,
		r.x + 78.,
		r.y + 32.,
		Paint::Styled(Condition::Panel, C::Color),
	));
	let summary = if settings.style.is_none() {
		"Stylesheets · following system"
	} else {
		"Stylesheets · highest priority first"
	};
	out.extend(shaper.label(
		summary,
		13.,
		r.x + 20.,
		r.y + 74.,
		Paint::Styled(Condition::Panel, C::Muted),
	));
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos = settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 92. + row as f32 * 60.;
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
			Paint::Styled(Condition::Panel, C::Color),
		));
		if e.error.is_some() && pos.is_none() {
			let rect = Rect {
				x: r.x + r.w - 180.,
				y,
				w: 76.,
				h: 26.,
			};
			out.push(Draw::Box {
				rect,
				chain: Condition::Button.chain(),
				condition: Condition::Button,
				fill: C::HoverBackground,
				radius: super::controls::BUTTON_RADIUS,
				border: 0.,
				left_only: false,
			});
			out.extend(shaper.label(
				"Invalid",
				12.,
				rect.x + 7.,
				rect.y + 18.,
				Paint::Styled(Condition::Button, C::DisabledColor),
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
				Condition::Panel,
				if e.error.is_some() {
					C::Error
				} else {
					C::Muted
				},
			),
		));
	}
	for b in style_controls(settings, entries, page, width, height) {
		out.extend(super::controls::paint_button(
			shaper,
			&b,
			interaction,
			false,
			12.,
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
			assert!(
				buttons
					.iter()
					.any(|b| b.action == Command::Settings && b.label == "×")
			);
		}
	}
}
