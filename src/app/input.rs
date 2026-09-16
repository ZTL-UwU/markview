use crate::state::{Command, Grain};
use gpui::{
	KeyDownEvent, ModifiersChangedEvent, MouseDownEvent, MouseMoveEvent,
	MouseUpEvent, ScrollDelta, ScrollWheelEvent,
};
use std::time::Instant;

use super::{App, BOTTOM, TOP, modifiers_from};

impl App {
	pub(super) fn on_mouse_move(
		&mut self,
		event: &MouseMoveEvent,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		self.interaction.modifiers = modifiers_from(event.modifiers);
		let old = self.interaction.cursor;
		self.interaction.cursor =
			(f32::from(event.position.x), f32::from(event.position.y));
		self.move_tab_drag();
		self.drag_scrollbar();
		self.update_drag();
		self.refresh_hover();
		if self.interaction.panel_open
			|| old.1 < TOP
			|| self.interaction.cursor.1 < TOP
			|| old.0 >= self.dimensions().0 - 16.
			|| self.interaction.cursor.0 >= self.dimensions().0 - 16.
		{
			self.redraw();
		}
		self.commit(cx);
	}

	pub(super) fn on_cursor_left(&mut self) {
		// A scrollbar drag survives leaving the window: the implicit pointer
		// grab still reports motion and the release, so the thumb keeps
		// following the pointer past the edges. The text selection gesture
		// still ends here.
		self.interaction.pointer_down = None;
		self.interaction.drag_at = None;
		self.interaction.hover = None;
		self.interaction.hover_overflow = None;
		self.redraw();
	}

	pub(super) fn on_focus_lost(&mut self) {
		self.tab_strip.cancel_drag();
		self.interaction.pressed = None;
		self.interaction.pointer_down = None;
		self.interaction.drag_at = None;
		self.interaction.scrollbar = None;
		self.interaction.modifiers = Default::default();
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn on_middle_down(
		&mut self,
		_: &MouseDownEvent,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		if self.interaction.panel_open {
			return;
		}
		if let Some(index) = self.tab_at_cursor() {
			self.action(Command::CloseTab(index));
		} else if let Some(link) =
			self.link_at(self.interaction.cursor.0, self.interaction.cursor.1)
		{
			self.open_link(&link, true);
		}
		self.commit(cx);
	}

	pub(super) fn on_left_down(
		&mut self,
		event: &MouseDownEvent,
		window: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		self.interaction.modifiers = modifiers_from(event.modifiers);
		self.tab_strip.cancel_drag();
		self.readers.session.select_all_pending = false;
		// A new press always ends a drag left over from a release the
		// platform swallowed outside the window.
		self.interaction.scrollbar = None;
		if let Some(index) = (!self.interaction.panel_open)
			.then(|| self.tab_close_at_cursor())
			.flatten()
		{
			self.interaction.reset_clicks();
			self.interaction.focus = None;
			self.action(Command::CloseTab(index));
		} else if let Some(index) = (!self.interaction.panel_open)
			.then(|| self.tab_at_cursor())
			.flatten()
		{
			self.interaction.reset_clicks();
			self.interaction.focus = None;
			self.begin_tab_drag(index);
		} else if let Some(button) = self.buttons().into_iter().find(|b| {
			b.rect
				.contains(self.interaction.cursor.0, self.interaction.cursor.1)
		}) {
			self.interaction.reset_clicks();
			self.interaction.focus = Some(button.action);
			self.interaction.pressed = Some(button.action);
			if !self.window_command(button.action, window) {
				self.action(button.action);
			}
		} else if self.interaction.panel_open {
			self.interaction.reset_clicks();
			if !self.pointer_in_panel() {
				self.action(Command::Settings);
			}
		} else if self.interaction.cursor.1 < super::TOP {
			if self.interaction.click_count(Instant::now()) >= 2 {
				window.zoom_window();
			} else {
				window.start_window_move();
			}
		} else if !self.pointer_in_panel() {
			self.interaction.focus = None;
			if self.begin_scrollbar_drag() {
				self.redraw();
			} else if self.interaction.cursor.1 >= TOP + 10.0
				&& self.interaction.cursor.1
					< self.dimensions().1 - BOTTOM - 10.0
			{
				if let Some(position) = self.text_at_cursor() {
					let click_count = if self.interaction.modifiers.shift_key()
					{
						self.interaction.reset_clicks();
						1
					} else {
						self.interaction.click_count(Instant::now())
					};
					let link = self.link_at(
						self.interaction.cursor.0,
						self.interaction.cursor.1,
					);
					match click_count {
						2 => {
							let selection = self
								.readers
								.session
								.snapshot
								.select_word_at(position);
							if !self
								.interaction
								.begin_grain_selection(selection, Grain::Word)
							{
								self.interaction
									.begin_selection(position, link);
							}
						}
						3 => {
							let selection = self
								.readers
								.session
								.snapshot
								.select_block_at(position);
							if !self
								.interaction
								.begin_grain_selection(selection, Grain::Block)
							{
								self.interaction
									.begin_selection(position, link);
							}
						}
						_ => self.interaction.begin_selection(position, link),
					}
				}
				self.redraw();
			}
		}
		self.commit(cx);
	}

	pub(super) fn on_left_up(
		&mut self,
		_: &MouseUpEvent,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		self.tab_strip.cancel_drag();
		self.interaction.pressed = None;
		self.interaction.scrollbar = None;
		let link =
			self.link_at(self.interaction.cursor.0, self.interaction.cursor.1);
		if let Some(link) = self.interaction.finish_selection(link.as_deref()) {
			self.open_link(&link, false);
		}
		self.refresh_hover();
		self.redraw();
		self.commit(cx);
	}

	pub(super) fn on_scroll(
		&mut self,
		event: &ScrollWheelEvent,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		self.interaction.modifiers = modifiers_from(event.modifiers);
		if self.interaction.panel_open {
			return;
		}
		let (dx, dy) = match event.delta {
			ScrollDelta::Lines(p) => (p.x * 42.0, p.y * 42.0),
			ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
		};
		if self.scroll_tabs(if dx.abs() > dy.abs() { -dx } else { -dy }) {
			self.commit(cx);
			return;
		}
		if self.interaction.modifiers.control_key()
			|| self.interaction.modifiers.super_key()
		{
			self.action(if dy > 0.0 {
				Command::Larger
			} else {
				Command::Smaller
			});
		} else if self.interaction.modifiers.shift_key() || dx.abs() > dy.abs()
		{
			self.horizontal_by(if dx.abs() > dy.abs() { -dx } else { -dy });
		} else {
			self.scroll_by(-dy);
		}
		self.commit(cx);
	}

	pub(super) fn on_modifiers(
		&mut self,
		event: &ModifiersChangedEvent,
		_: &mut gpui::Window,
		_: &mut gpui::Context<Self>,
	) {
		self.interaction.modifiers = modifiers_from(event.modifiers);
	}

	pub(super) fn on_drop_files(
		&mut self,
		paths: &gpui::ExternalPaths,
		_: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		if let Some(path) = paths.paths().first() {
			self.open(path.clone());
		}
		self.commit(cx);
	}

	pub(super) fn on_key_down(
		&mut self,
		event: &KeyDownEvent,
		window: &mut gpui::Window,
		cx: &mut gpui::Context<Self>,
	) {
		self.interaction.modifiers = modifiers_from(event.keystroke.modifiers);
		let command = self.interaction.modifiers.control_key()
			|| self.interaction.modifiers.super_key();
		let key = event.keystroke.key.as_str();
		if command {
			match key {
				"a" if !self.panel_has_focus() => {
					if self.readers.session.layout_pending {
						self.interaction.clear_selection();
						self.readers.session.select_all_pending = true;
						self.redraw();
						self.commit(cx);
						return;
					}
					self.interaction.selection = self
						.readers
						.session
						.snapshot
						.select_all(self.readers.session.accepted_revision);
					self.redraw();
				}
				"c" if !self.panel_has_focus() => self.copy_selection(),
				"v" if !self.interaction.panel_open => self.paste_markdown(),
				"w" if !self.panel_has_focus() => {
					self.action(Command::CloseTab(self.readers.active()))
				}
				"," => self.action(Command::Settings),
				"o" if !self.panel_has_focus() => self.action(Command::Open),
				"t" => self.action(Command::Styles),
				"-" => self.action(Command::Smaller),
				"+" | "=" => self.action(Command::Larger),
				"[" => self.action(Command::Narrower),
				"]" => self.action(Command::Wider),
				"l" => self.action(Command::Align),
				"h" => self.action(Command::Hyphens),
				"q" => {
					self.pending_quit = true;
				}
				_ => {}
			}
		} else {
			if self.panel_has_focus()
				&& !matches!(key, "tab" | "enter" | "escape")
			{
				self.commit(cx);
				return;
			}
			match key {
				"down" => self.scroll_by(42.0),
				"up" => self.scroll_by(-42.0),
				"pagedown" | "space" => self.scroll_by(self.viewport() * 0.9),
				"pageup" => self.scroll_by(-self.viewport() * 0.9),
				"home" => self.scroll_by(f32::NEG_INFINITY),
				"end" => self.scroll_by(f32::INFINITY),
				"left" => self.horizontal_by(-42.0),
				"right" => self.horizontal_by(42.0),
				"tab" => {
					let buttons = self.buttons();
					let current = buttons
						.iter()
						.position(|b| Some(b.action) == self.interaction.focus);
					let index = match current {
						None => {
							if self.interaction.modifiers.shift_key() {
								buttons.len() - 1
							} else {
								0
							}
						}
						Some(i) => {
							(i + if self.interaction.modifiers.shift_key() {
								buttons.len() - 1
							} else {
								1
							}) % buttons.len()
						}
					};
					self.interaction.focus = Some(buttons[index].action);
					self.redraw();
				}
				"enter" => {
					if let Some(action) = self.interaction.focus
						&& self.buttons().iter().any(|b| b.action == action)
						&& !self.window_command(action, window)
					{
						self.action(action);
					}
				}
				"escape" => {
					self.tab_strip.cancel_drag();
					self.interaction.focus = None;
					self.interaction.panel_open = false;
					self.interaction.styles_open = false;
					self.interaction.selection = None;
					self.interaction.pointer_down = None;
					self.interaction.drag_at = None;
					self.interaction.scrollbar = None;
					self.refresh_hover();
					self.redraw();
				}
				_ => {}
			}
		}
		self.commit(cx);
	}
}
