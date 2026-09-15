//! Reader chrome built from borrowed display state, with no window or worker access.
mod controls;
mod footer;
#[cfg(test)]
mod gpu_tests;
mod styles;
mod tabs;
use super::{BOTTOM, Button, TOP};
use crate::{
	layout::{Draw, Paint, Rect, Scrollbar, TextShaper},
	settings::ReaderSettings,
	state::{InteractionState, ReaderSession, ReaderTab, ScrollbarAxis},
};
pub(super) use controls::panel_rect;
use controls::{controls, draw_controls, toolbar_controls};
use footer::draw_footer;
use markview_core::style::{ColorField as C, Condition};
use std::time::Instant;
use styles::{draw_styles, style_controls};

pub(super) struct Chrome<'a> {
	pub(super) ui: &'a mut TextShaper,
	pub(super) session: &'a ReaderSession,
	pub(super) tabs: &'a [ReaderTab],
	pub(super) active_tab: usize,
	pub(super) tab_strip: &'a super::tab_strip::TabStrip,
	pub(super) tab_widths: &'a [(f32, f32)],
	pub(super) settings: &'a ReaderSettings,
	pub(super) interaction: &'a InteractionState,
	pub(super) style_entries: &'a [crate::stylesheet::Entry],
	pub(super) style_page: usize,
	pub(super) width: f32,
	pub(super) height: f32,
	pub(super) scrollbar: Option<Scrollbar>,
	pub(super) warning: Option<&'a str>,
	pub(super) status: &'a str,
	pub(super) status_until: Option<Instant>,
	pub(super) error: bool,
}
impl Chrome<'_> {
	pub(super) fn buttons(&mut self) -> Vec<Button> {
		let (width, height, _) = (self.width, self.height, 1.0);
		if self.interaction.styles_open {
			style_controls(
				self.settings,
				self.style_entries,
				self.style_page,
				width,
				height,
			)
		} else if self.interaction.panel_open {
			controls(
				self.ui,
				self.settings,
				self.interaction.panel_open,
				width,
				height,
			)
		} else {
			toolbar_controls(self.ui, width)
		}
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		let (width, height, _) = (self.width, self.height, 1.0);
		let mut out = vec![
			Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: width,
					h: TOP,
				},
				Paint::Styled(Condition::Toolbar, C::Background),
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Styled(Condition::Toolbar, C::BorderColor),
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: height - BOTTOM,
					w: width,
					h: BOTTOM,
				},
				Paint::Styled(Condition::Toolbar, C::Background),
			),
		];
		out.extend(self.tab_bar().draw_tabs());
		let warning = if self.error
			&& self
				.status_until
				.is_none_or(|until| until <= Instant::now())
		{
			Some(self.status)
		} else {
			self.warning
		};
		out.extend(draw_footer(
			self.ui,
			(!self.session.layout_pending).then_some(self.session.counts),
			self.interaction.selection_counts.map(|(_, counts)| counts),
			warning,
			if self
				.status_until
				.is_some_and(|until| until > Instant::now())
			{
				self.status
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
			} else if self.session.layout_pending
				|| self.session.document.is_none()
			{
				"Opening document…"
			} else {
				"The document is empty"
			};
			out.extend(self.ui.label(
				title,
				26.0,
				x,
				y,
				Paint::Styled(Condition::Ui, C::Color),
			));
			out.extend(self.ui.label(
				if self.session.path.is_some()
					&& !self.error && self.session.layout_pending
				{
					"Preparing the first page…"
				} else {
					"Drop a file here or press Ctrl+O."
				},
				15.0,
				x,
				y + 38.0,
				Paint::Styled(Condition::Ui, C::Muted),
			));
		}
		if let Some(bar) = self.scrollbar {
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
				Paint::Styled(Condition::Scrollbar, C::Track),
			));
			out.push(Draw::Rect(
				thumb,
				Paint::Styled(
					Condition::Scrollbar,
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
				self.ui,
				self.settings,
				self.interaction,
				self.style_entries,
				self.style_page,
				width,
				height,
			));
		} else {
			out.extend(draw_controls(
				self.ui,
				self.settings,
				self.interaction,
				width,
				height,
			));
		}
		out
	}

	pub(super) fn tab_bar(&mut self) -> tabs::TabBar<'_> {
		tabs::TabBar {
			ui: self.ui,
			strip: self.tab_strip,
			widths: self.tab_widths,
			tabs: self.tabs,
			active_tab: self.active_tab,
			cursor: self.interaction.cursor,
			width: self.width,
		}
	}
}
