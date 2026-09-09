//! Per-document state and transient read-only interaction state.
use crate::{
	document,
	layout::{LayoutOptions, LayoutSnapshot},
};
use markview_core::text::{TextCounts, TextSelection};
use std::{
	collections::HashMap,
	path::PathBuf,
	sync::Arc,
	time::{Duration, Instant},
};
use winit::keyboard::ModifiersState;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	Open,
	Smaller,
	Larger,
	Narrower,
	Wider,
	Align,
	Hyphens,
	CjkType(markview_core::style::CjkType),
	Settings,
	Reset,
	OpenConfig,
	SystemTheme,
	Styles,
	StyleToggle(usize),
	StyleUp(usize),
	StyleDown(usize),
	StylePrev,
	StyleNext,
	StylesFolder,
}

#[derive(Default)]
pub(crate) struct ReaderSession {
	pub(crate) counts: TextCounts,
	pub(crate) path: Option<PathBuf>,
	pub(crate) snapshot: LayoutSnapshot,
	pub(crate) accepted_revision: u64,
	pub(crate) accepted_content_id: u64,
	pub(crate) version: u64,
	pub(crate) content_version: u64,
	pub(crate) document: Option<Arc<document::Document>>,
	pub(crate) requested_options: Option<LayoutOptions>,
	pub(crate) scroll: f32,
	pub(crate) horizontal: HashMap<(usize, usize), f32>,
	pub(crate) follow_update: bool,
}

#[derive(Default)]
pub(crate) struct InteractionState {
	pub(crate) selection_counts: Option<(TextSelection, TextCounts)>,
	pub(crate) panel_open: bool,
	pub(crate) styles_open: bool,
	pub(crate) selection: Option<TextSelection>,
	pub(crate) pointer_down: Option<Drag>,
	pub(crate) dragged: bool,
	pub(crate) drag_at: Option<Instant>,
	pub(crate) modifiers: ModifiersState,
	pub(crate) cursor: (f32, f32),
	pub(crate) hover: Option<String>,
	pub(crate) focus: Option<Command>,
	pub(crate) pressed: Option<Command>,
	pub(crate) last_click: Option<(Instant, (f32, f32), u8)>,
}

/// An in-flight press: where it started and the link it would activate.
#[derive(Clone, Debug)]
pub(crate) struct Drag {
	pub(crate) start: (f32, f32),
	pub(crate) link: Option<String>,
}

impl InteractionState {
	pub(crate) fn reset_clicks(&mut self) {
		self.last_click = None;
	}

	pub(crate) fn click_count(&mut self, now: Instant) -> u8 {
		const CLICK_INTERVAL: Duration = Duration::from_millis(500);
		const CLICK_DISTANCE: f32 = 6.0;
		let count = match self.last_click {
			Some((at, point, count))
				if now.duration_since(at) <= CLICK_INTERVAL
					&& (self.cursor.0 - point.0)
						.hypot(self.cursor.1 - point.1)
						<= CLICK_DISTANCE =>
			{
				count % 3 + 1
			}
			_ => 1,
		};
		self.last_click = Some((now, self.cursor, count));
		count
	}

	pub(crate) fn begin_selection(
		&mut self,
		position: markview_core::text::TextPosition,
		link: Option<String>,
	) {
		let anchor = if self.modifiers.shift_key() {
			self.selection.map_or(position, |s| s.anchor)
		} else {
			position
		};
		self.selection = Some(TextSelection {
			anchor,
			focus: position,
		});
		self.pointer_down = Some(Drag {
			start: self.cursor,
			link,
		});
		self.dragged = self.modifiers.shift_key();
		self.focus = None;
	}
	pub(crate) fn move_selection(
		&mut self,
		position: Option<markview_core::text::TextPosition>,
	) {
		if let Some(drag) = &self.pointer_down {
			self.dragged |= (self.cursor.0 - drag.start.0)
				.hypot(self.cursor.1 - drag.start.1)
				>= 4.0;
			if self.dragged
				&& let Some(position) = position
				&& let Some(selection) = &mut self.selection
			{
				selection.focus = position;
			}
		}
	}
	pub(crate) fn finish_selection(
		&mut self,
		release_link: Option<&str>,
	) -> Option<String> {
		self.drag_at = None;
		let drag = self.pointer_down.take()?;
		drag.link
			.filter(|link| !self.dragged && release_link == Some(link.as_str()))
	}
	pub(crate) fn clear_selection(&mut self) {
		self.selection = None;
		self.pointer_down = None;
		self.drag_at = None;
	}
}
impl ReaderSession {
	pub(crate) fn accept(
		&mut self,
		reader: crate::worker::ReaderSnapshot,
		viewport: f32,
	) -> bool {
		// A metadata-only change re-reads identical bytes; only a real content
		// change may invalidate reading positions.
		let changed = reader.document.content_id != self.accepted_content_id;
		if changed {
			self.counts = reader
				.layout
				.select_all(reader.content_version)
				.map(|selection| {
					TextCounts::of(
						&reader
							.layout
							.extract_text(selection, reader.content_version),
					)
				})
				.unwrap_or_default();
		}
		self.scroll = if self.snapshot.blocks.is_empty() {
			0.0
		} else {
			crate::layout::anchored_scroll(
				&self.snapshot,
				&reader.layout,
				self.scroll,
				viewport,
				self.follow_update,
			)
		};
		self.accepted_content_id = reader.document.content_id;
		self.document = Some(reader.document);
		self.snapshot = reader.layout;
		self.accepted_revision = reader.content_version;
		self.follow_update = false;
		self.horizontal.clear();
		changed
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_core::text::{Affinity, TextPosition};
	fn position(offset: usize) -> TextPosition {
		TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset,
			affinity: Affinity::Before,
		}
	}
	#[test]
	fn click_opens_only_on_release_and_drag_never_opens_link() {
		let mut interaction = InteractionState::default();
		interaction
			.begin_selection(position(0), Some("https://example.com".into()));
		assert!(interaction.pointer_down.is_some());
		assert_eq!(
			interaction.finish_selection(Some("https://example.com")),
			Some("https://example.com".into())
		);
		interaction
			.begin_selection(position(0), Some("https://example.com".into()));
		interaction.cursor = (8.0, 0.0);
		interaction.move_selection(Some(position(4)));
		interaction.cursor = (0.0, 0.0);
		interaction.move_selection(Some(position(0)));
		assert!(
			interaction
				.finish_selection(Some("https://example.com"))
				.is_none()
		);
		interaction
			.begin_selection(position(0), Some("https://example.com".into()));
		assert!(interaction.finish_selection(None).is_none());
	}
	#[test]
	fn shift_extends_original_anchor_and_reload_clears_gesture() {
		let mut interaction = InteractionState::default();
		interaction.begin_selection(position(2), None);
		interaction.finish_selection(None);
		interaction.modifiers = ModifiersState::SHIFT;
		interaction
			.begin_selection(position(10), Some("https://example.com".into()));
		assert_eq!(interaction.selection.unwrap().anchor.offset, 2);
		assert_eq!(interaction.selection.unwrap().focus.offset, 10);
		assert!(
			interaction
				.finish_selection(Some("https://example.com"))
				.is_none()
		);
		interaction.clear_selection();
		assert!(interaction.selection.is_none());
		assert!(interaction.drag_at.is_none());
	}
	#[test]
	fn repeated_presses_within_the_interval_cycle_word_and_block_click() {
		let mut interaction = InteractionState {
			cursor: (40.0, 40.0),
			..Default::default()
		};
		let start = Instant::now();
		assert_eq!(interaction.click_count(start), 1);
		assert_eq!(
			interaction.click_count(start + Duration::from_millis(120)),
			2
		);
		assert_eq!(
			interaction.click_count(start + Duration::from_millis(240)),
			3
		);
		assert_eq!(
			interaction.click_count(start + Duration::from_millis(360)),
			1
		);
		// A pause, a pointer move or a toolbar press starts a new single click.
		assert_eq!(interaction.click_count(start + Duration::from_secs(2)), 1);
		interaction.cursor = (60.0, 40.0);
		assert_eq!(
			interaction.click_count(
				start + Duration::from_secs(2) + Duration::from_millis(100)
			),
			1
		);
		interaction.reset_clicks();
		assert_eq!(
			interaction.click_count(
				start + Duration::from_secs(2) + Duration::from_millis(200)
			),
			1
		);
	}
	#[test]
	fn identical_content_with_a_new_version_keeps_the_selection() {
		let document = Arc::new(document::parse("Hello"));
		let mut engine = crate::layout::LayoutEngine::new();
		let mut session = ReaderSession::default();
		let layout = engine.layout(&document, &LayoutOptions::default());
		assert!(session.accept(
			crate::worker::ReaderSnapshot {
				document: document.clone(),
				layout: layout.clone(),
				content_version: 1,
			},
			300.0,
		));
		// A file event re-reads the same bytes: a new revision, same content.
		assert!(!session.accept(
			crate::worker::ReaderSnapshot {
				document,
				layout,
				content_version: 2,
			},
			300.0,
		));
		assert_eq!(session.accepted_revision, 2);
	}
	#[test]
	fn text_and_layout_are_accepted_together_and_reflow_is_not_new_content() {
		let document = Arc::new(document::parse("Hello"));
		let mut engine = crate::layout::LayoutEngine::new();
		let mut session = ReaderSession::default();
		let reader = crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: engine.layout(&document, &LayoutOptions::default()),
			content_version: 1,
		};
		assert!(session.accept(reader.clone(), 300.0));
		assert_eq!(session.counts, TextCounts { chars: 5, words: 1 });
		assert!(!session.accept(reader, 300.0));
		assert_eq!(session.counts, TextCounts { chars: 5, words: 1 });
		assert!(Arc::ptr_eq(session.document.as_ref().unwrap(), &document));
	}
}
