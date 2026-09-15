use crate::cli::Mode;
use std::time::{Duration, Instant};

use super::{App, Event};

impl App {
	pub(super) fn handle_event(
		&mut self,
		event: Event,
		cx: &mut gpui::Context<Self>,
	) {
		match event {
			Event::StylesChanged => {
				self.preferences.style_entries = crate::stylesheet::scan(
					crate::stylesheet::directory().as_deref(),
				);
				self.reload_styles();
				self.redraw();
			}
			Event::SettingsChanged => {
				if self.preferences.reload() {
					self.apply_saved_settings(None);
				}
				self.redraw();
			}
			Event::Open(path) => {
				self.dialog_open = false;
				if let Some(path) = path {
					self.open(path);
				}
			}
			Event::Changed(path)
				if self.readers.session.path.as_ref() == Some(&path) =>
			{
				self.readers.session.content_version += 1;
				self.request(true)
			}
			Event::Ready(mut update)
				if update.version == self.readers.session.version
					&& self.readers.session.path.as_ref()
						== Some(&update.path) =>
			{
				match update.result.take() {
					Some(Ok(reader)) => {
						if !reader.complete
							&& self.interaction.selection.is_some_and(|s| {
								s.anchor.block.max(s.focus.block)
									>= reader.layout.blocks.len()
							}) {
							self.commit(cx);
							return;
						}
						if !self
							.readers
							.session
							.can_display(&reader, self.viewport())
						{
							self.commit(cx);
							return;
						}
						let first = self.readers.session.displayed_version
							!= update.version;
						let complete = reader.complete;
						let reading_changed =
							!self.readers.session.extends_prefix(&reader)
								&& !self
									.readers
									.session
									.snapshot
									.same_reading_text(&reader.layout);
						let rebased =
							self.interaction.selection.and_then(|s| {
								self.readers.session.snapshot.rebase_selection(
									&reader.layout,
									s,
									self.readers.session.accepted_revision,
									reader.content_version,
								)
							});
						if self.readers.session.accept(reader, self.viewport())
						{
							self.interaction.clear_selection();
						} else {
							if reading_changed {
								self.interaction.clear_selection();
								self.interaction.selection_counts = None;
							}
							self.interaction.selection = rebased;
						}
						self.error = false;
						self.readers.session.displayed_version = update.version;
						if complete && self.readers.session.select_all_pending {
							self.interaction.selection =
								self.readers.session.snapshot.select_all(
									self.readers.session.accepted_revision,
								);
							self.readers.session.select_all_pending = false;
						}
						self.refresh_hover();
						self.status = if !complete {
							"Loading…".into()
						} else if self.readers.session.snapshot.math_errors > 0
						{
							format!(
								"{} formulas shown as source",
								self.readers.session.snapshot.math_errors
							)
						} else {
							String::new()
						};
						self.apply_anchor();
						self.title = format!(
							"{} — Markview",
							update
								.path
								.file_name()
								.unwrap_or_default()
								.to_string_lossy()
						);
						if complete {
							eprintln!(
								"full layout complete: {:.2} ms; {} blocks",
								update.requested.elapsed().as_secs_f64()
									* 1000.,
								self.readers.session.snapshot.blocks.len()
							);
						}
						if first {
							self.first_frame = Some(*update);
						}
					}
					Some(Err(error)) => {
						self.readers.session.layout_pending =
							!self.readers.session.snapshot_complete;
						self.readers.session.pending_scroll = None;
						self.readers.session.pending_anchor = None;
						self.readers.session.select_all_pending = false;
						self.error = true;
						self.status = error;
						if self.args.mode == Mode::Smoke {
							self.fail(self.status.clone(), cx);
							return;
						}
					}
					None => {}
				}
				self.redraw();
			}
			_ => {}
		}
		self.commit(cx);
	}

	pub(super) fn tick(&mut self, cx: &mut gpui::Context<Self>) {
		let now = Instant::now();
		self.auto_scroll_tabs(now);
		self.readers.release_inactive(now);
		if self.status_until.is_some_and(|until| until <= now) {
			self.status_until = None;
			self.status.clear();
			self.error = false;
			self.redraw();
		}
		if self.preferences.save_deadline().is_some_and(|d| d <= now) {
			self.flush_settings();
			self.redraw();
		}
		if self.interaction.drag_at.is_some_and(|d| d <= now) {
			self.interaction.drag_at = None;
			if self.interaction.pointer_down.is_some() {
				self.scroll_by(
					if self.interaction.cursor.1 < super::TOP + 24.0 {
						-14.0
					} else {
						14.0
					},
				);
				self.update_drag();
			}
		}
		if self.reflow_at.is_some_and(|d| d <= now) {
			self.reflow_at = None;
			if self.readers.session.requested_options.as_ref()
				!= Some(&self.options())
			{
				self.request(false);
			}
			self.scroll_by(0.0);
		}
		if self.args.mode == Mode::Smoke
			&& self.started.elapsed() > Duration::from_secs(30)
		{
			self.fail("Native window smoke test timed out".into(), cx);
			return;
		}
		self.commit(cx);
	}

	pub(super) fn schedule_tick(&mut self, cx: &mut gpui::Context<Self>) {
		self.timer_gen += 1;
		let generation = self.timer_gen;
		let deadline = self
			.reflow_at
			.into_iter()
			.chain(self.status_until)
			.chain(self.preferences.save_deadline())
			.chain(self.interaction.drag_at)
			.chain(self.tab_strip.scroll_at)
			.chain(self.readers.release_deadline())
			.chain(
				(self.args.mode == Mode::Smoke)
					.then_some(self.started + Duration::from_secs(30)),
			)
			.min();
		let Some(deadline) = deadline else {
			return;
		};
		let wait = deadline.saturating_duration_since(Instant::now());
		cx.spawn(async move |this, cx| {
			gpui::Timer::after(wait).await;
			this.update(cx, |app, cx| {
				if app.timer_gen == generation {
					app.tick(cx);
				}
			})
			.ok();
		})
		.detach();
	}
}
