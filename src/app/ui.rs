//! Adapt application state to borrowed chrome inputs.
use super::{App, Button, chrome::Chrome};
use crate::layout::Draw;
impl App {
	fn chrome(&mut self) -> Chrome<'_> {
		let (width, height, _) = self.dimensions();
		let scrollbar = self.document_scrollbar();
		Chrome {
			ui: &mut self.ui,
			session: &self.readers.session,
			tabs: self.readers.entries(),
			active_tab: self.readers.active(),
			settings: &self.preferences.values,
			interaction: &self.interaction,
			style_entries: &self.preferences.style_entries,
			style_page: self.preferences.style_page,
			width,
			height,
			scrollbar,
			warning: self
				.preferences
				.style_warning
				.as_deref()
				.or(self.preferences.settings_warning.as_deref()),
			status: &self.status,
			status_until: self.status_until,
			error: self.error,
		}
	}
	pub(super) fn buttons(&mut self) -> Vec<Button> {
		self.chrome().buttons()
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		let session = &self.readers.session;
		let selection = self.interaction.selection.filter(|s| {
			!s.is_empty()
				&& s.anchor.revision == session.accepted_revision
				&& s.focus.revision == session.accepted_revision
		});
		if self.interaction.selection_counts.map(|(s, _)| s) != selection {
			self.interaction.selection_counts = selection.map(|s| {
				(
					s,
					markview_core::text::TextCounts::of(
						&session
							.snapshot
							.extract_text(s, session.accepted_revision),
					),
				)
			});
		}
		self.chrome().overlay()
	}
	pub(super) fn tab_at_cursor(&mut self) -> Option<usize> {
		let (x, y) = self.interaction.cursor;
		self.chrome()
			.tab_bar()
			.tab_rects()
			.into_iter()
			.find(|(rect, _)| rect.contains(x, y))
			.map(|(_, i)| i)
	}
	pub(super) fn tab_close_at_cursor(&mut self) -> Option<usize> {
		let (x, y) = self.interaction.cursor;
		self.chrome()
			.tab_bar()
			.tab_rects()
			.into_iter()
			.find_map(|(rect, i)| {
				(rect.contains(x, y) && x >= rect.x + rect.w - 24.0)
					.then_some(i)
			})
	}
}
