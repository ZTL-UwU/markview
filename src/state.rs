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
	pub(crate) hover_image: Option<String>,
	/// The wide block whose horizontal scrollbar the pointer is over.
	pub(crate) hover_overflow: Option<(usize, usize)>,
	pub(crate) focus: Option<Command>,
	pub(crate) pressed: Option<Command>,
	pub(crate) scrollbar: Option<ScrollbarDrag>,
	pub(crate) last_click: Option<(Instant, (f32, f32), u8)>,
}

/// Which scrollbar a press grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollbarAxis {
	/// The document's vertical scrollbar.
	Document,
	/// The horizontal scrollbar of one overflowing block.
	Overflow { block: usize, overflow: usize },
}

/// An in-flight scrollbar drag: the grabbed bar and how far inside its thumb
/// the pointer grabbed it, so the thumb never jumps under the pointer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarDrag {
	pub(crate) target: ScrollbarAxis,
	pub(crate) grab: f32,
}

/// Selection unit of an in-flight press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grain {
	Char,
	Word,
	Block,
}

/// An in-flight press: where it started and the link it would activate.
#[derive(Clone, Debug)]
pub(crate) struct Drag {
	pub(crate) start: (f32, f32),
	pub(crate) link: Option<String>,
	pub(crate) grain: Grain,
	/// The word or block a multi-click press selected, kept as the drag base.
	pub(crate) base: Option<TextSelection>,
}

/// Extends a multi-click base selection to the word or block under the pointer,
/// keeping the base as the fixed edge.
fn extend(
	base: TextSelection,
	unit: TextSelection,
	position: markview_core::text::TextPosition,
) -> TextSelection {
	let (first, last) = base.ordered();
	if position.cmp_reading(first).is_lt() {
		TextSelection {
			anchor: last,
			focus: unit.anchor,
		}
	} else if position.cmp_reading(last).is_gt() {
		TextSelection {
			anchor: first,
			focus: unit.focus,
		}
	} else {
		base
	}
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
			grain: Grain::Char,
			base: None,
		});
		self.dragged = self.modifiers.shift_key();
		self.focus = None;
	}
	/// Starts a press that already selected a word or block, so dragging
	/// extends the selection by that unit instead of by grapheme. Returns
	/// false when there is no selection to start from.
	pub(crate) fn begin_grain_selection(
		&mut self,
		selection: Option<TextSelection>,
		grain: Grain,
	) -> bool {
		let Some(selection) = selection else {
			return false;
		};
		self.selection = Some(selection);
		self.pointer_down = Some(Drag {
			start: self.cursor,
			link: None,
			grain,
			base: Some(selection),
		});
		self.dragged = false;
		self.focus = None;
		true
	}
	pub(crate) fn move_selection(
		&mut self,
		position: Option<markview_core::text::TextPosition>,
		snapshot: &LayoutSnapshot,
	) {
		let Some(drag) = &self.pointer_down else {
			return;
		};
		let (start, grain, base) = (drag.start, drag.grain, drag.base);
		self.dragged |=
			(self.cursor.0 - start.0).hypot(self.cursor.1 - start.1) >= 4.0;
		if !self.dragged {
			return;
		}
		let Some(position) = position else {
			return;
		};
		let Some(selection) = &mut self.selection else {
			return;
		};
		match (grain, base) {
			(Grain::Char, _) | (_, None) => selection.focus = position,
			(Grain::Word, Some(base)) => {
				if let Some(word) = snapshot.select_word_at(position) {
					*selection = extend(base, word, position);
				}
			}
			(Grain::Block, Some(base)) => {
				if let Some(block) = snapshot.select_block_at(position) {
					*selection = extend(base, block, position);
				}
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
		self.scrollbar = None;
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
		if changed || !self.snapshot.same_reading_text(&reader.layout) {
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
		self.horizontal.retain(|(bi, oi), offset| {
			if changed {
				return false;
			}
			if let Some(o) = self
				.snapshot
				.blocks
				.get(*bi)
				.and_then(|b| b.layout.overflow.get(*oi))
			{
				*offset =
					offset.clamp(0., (o.content_width - o.rect.w).max(0.));
				true
			} else {
				false
			}
		});
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
	fn position_in(block: usize, offset: usize) -> TextPosition {
		TextPosition {
			block,
			..position(offset)
		}
	}
	fn snapshot_of(source: &str) -> LayoutSnapshot {
		let document = Arc::new(document::parse(source));
		crate::layout::LayoutEngine::new()
			.layout(&document, &LayoutOptions::default())
	}
	#[test]
	fn click_opens_only_on_release_and_drag_never_opens_link() {
		let snapshot = LayoutSnapshot::default();
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
		interaction.move_selection(Some(position(4)), &snapshot);
		interaction.cursor = (0.0, 0.0);
		interaction.move_selection(Some(position(0)), &snapshot);
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
	fn word_and_block_drag_extend_from_the_multi_click_base() {
		let snapshot = snapshot_of("测试 中文 一下");
		let base = snapshot.select_word_at(position(7)).unwrap();
		assert_eq!(snapshot.extract_text(base, 1), "中文");
		let mut interaction = InteractionState {
			cursor: (0.0, 0.0),
			..Default::default()
		};
		assert!(interaction.begin_grain_selection(Some(base), Grain::Word));
		// Below the drag threshold the double-clicked word stays selected.
		interaction.cursor = (2.0, 0.0);
		interaction.move_selection(Some(position(17)), &snapshot);
		assert_eq!(interaction.selection.unwrap(), base);
		// Right of the base word, the far edge follows the word under the pointer.
		interaction.cursor = (60.0, 0.0);
		interaction.move_selection(Some(position(17)), &snapshot);
		assert_eq!(
			snapshot.extract_text(interaction.selection.unwrap(), 1),
			"中文 一下"
		);
		// Left of the base word, the anchor moves and the base end stays fixed.
		interaction.move_selection(Some(position(0)), &snapshot);
		assert_eq!(
			snapshot.extract_text(interaction.selection.unwrap(), 1),
			"测试 中文"
		);
		// Triple-click drags whole blocks.
		let blocks = snapshot_of("First paragraph.\n\nSecond paragraph.");
		let base = blocks.select_block_at(position(2)).unwrap();
		let mut interaction = InteractionState {
			cursor: (0.0, 0.0),
			..Default::default()
		};
		assert!(interaction.begin_grain_selection(Some(base), Grain::Block));
		interaction.cursor = (60.0, 0.0);
		interaction.move_selection(Some(position_in(1, 2)), &blocks);
		assert_eq!(
			blocks.extract_text(interaction.selection.unwrap(), 1),
			"First paragraph.\n\nSecond paragraph."
		);
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
