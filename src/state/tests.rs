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
			complete: true,
		},
		300.0,
	));
	// A file event re-reads the same bytes: a new revision, same content.
	assert!(!session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout,
			content_version: 2,
			complete: true,
		},
		300.0,
	));
	assert_eq!(session.accepted_revision, 2);
}

#[test]
fn pending_pages_accumulate_reverse_and_resolve_without_blank_frames() {
	let mut session = ReaderSession {
		layout_pending: true,
		..Default::default()
	};
	session.snapshot.height = 700.;
	for _ in 0..3 {
		session.scroll_by(540., 600.);
	}
	assert_eq!(session.scroll, 0.);
	assert_eq!(session.pending_scroll, Some(1620.));
	session.snapshot.height = 1500.;
	session.resolve_scroll(600.);
	assert_eq!(session.scroll, 0.);
	session.snapshot.height = 2300.;
	session.resolve_scroll(600.);
	assert_eq!(session.scroll, 1620.);
	assert_eq!(session.pending_scroll, None);
	session.scroll_by(f32::INFINITY, 600.);
	assert_eq!(session.scroll, 1620.);
	session.scroll_by(-540., 600.);
	assert_eq!(session.scroll, 1080.);
	session.scroll_by(5400., 600.);
	session.scroll_by(f32::NEG_INFINITY, 600.);
	assert_eq!(session.scroll, 0.);
	assert_eq!(session.pending_scroll, None);
	session.scroll_by(f32::INFINITY, 600.);
	session.scroll_by(0., 600.);
	assert_eq!(session.pending_scroll, Some(f32::INFINITY));
	session.layout_pending = false;
	session.resolve_scroll(600.);
	assert_eq!(session.scroll, 1700.);
}

#[test]
fn heading_anchors_queue_until_their_heading_is_laid_out() {
	let mut engine = crate::layout::LayoutEngine::new();
	let options = LayoutOptions::default();
	let document = Arc::new(document::parse(
		"# Intro\n\nParagraph.\n\n# Details\n\nMore.\n",
	));
	let layout = engine.layout(&document, &options);
	let details = layout.anchor_y("details").unwrap();
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: layout.clone(),
			content_version: 1,
			complete: true,
		},
		300.,
	);
	session.pending_anchor = Some("details".into());
	assert_eq!(session.resolve_anchor(300.), Some(Ok(())));
	assert_eq!(session.pending_anchor, None);
	let max = (layout.height - 300.).max(0.);
	assert_eq!(session.scroll, details.clamp(0., max));
	// A heading the finished document lacks is reported once.
	session.pending_anchor = Some("missing".into());
	assert_eq!(session.resolve_anchor(300.), Some(Err("missing".into())));
	assert_eq!(session.pending_anchor, None);
	// An unfinished prefix keeps the anchor queued.
	let mut prefix = layout.clone();
	prefix.blocks.truncate(1);
	prefix.height = layout.blocks[1].y;
	session.accept(
		crate::worker::ReaderSnapshot {
			document,
			layout: prefix,
			content_version: 2,
			complete: false,
		},
		300.,
	);
	session.pending_anchor = Some("details".into());
	assert_eq!(session.resolve_anchor(300.), None);
	assert_eq!(session.pending_anchor.as_deref(), Some("details"));
	// A deliberate scroll abandons the queued anchor.
	session.scroll_by(40., 300.);
	assert_eq!(session.pending_anchor, None);
}

#[test]
fn partial_reload_waits_for_anchor_and_keeps_the_old_snapshot() {
	let mut engine = crate::layout::LayoutEngine::new();
	let options = LayoutOptions::default();
	let document = Arc::new(document::parse("Paragraph.\n\n".repeat(100)));
	let full = engine.layout(&document, &options);
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: full.clone(),
			content_version: 1,
			complete: true,
		},
		600.,
	);
	session.scroll = 1800.;
	let mut partial = full.clone();
	partial.blocks.truncate(5);
	partial.height = full.blocks[5].y;
	let reader = crate::worker::ReaderSnapshot {
		document,
		layout: partial,
		content_version: 2,
		complete: false,
	};
	assert!(!session.can_display(&reader, 600.));
	assert_eq!(session.snapshot.blocks.len(), 100);
	assert_eq!(session.scroll, 1800.);
}

#[test]
fn completing_a_prefix_preserves_scroll_and_selection_and_finishes_counts() {
	let document = Arc::new(document::parse("Paragraph.\n\n".repeat(100)));
	let mut engine = crate::layout::LayoutEngine::new();
	let mut prefix = None;
	let layout = engine
		.layout_progressive(
			&document,
			&LayoutOptions::default(),
			&Default::default(),
			|p| {
				if p.blocks.len() == 10 {
					prefix = Some(p.clone());
				}
				true
			},
		)
		.unwrap();
	let mut session = ReaderSession::default();
	session.accept(
		crate::worker::ReaderSnapshot {
			document: document.clone(),
			layout: prefix.unwrap(),
			content_version: 1,
			complete: false,
		},
		100.,
	);
	session.scroll_by(100., 100.);
	let selection = session.snapshot.select_all(1).unwrap();
	let text = session.snapshot.extract_text(selection, 1);
	let reader = crate::worker::ReaderSnapshot {
		document,
		layout,
		content_version: 1,
		complete: true,
	};
	assert!(session.extends_prefix(&reader));
	let rebased = session
		.snapshot
		.rebase_selection(&reader.layout, selection, 1, 1)
		.unwrap();
	assert_eq!(reader.layout.extract_text(rebased, 1), text);
	assert_eq!(session.counts, TextCounts::default());
	session.accept(reader, 100.);
	assert_eq!(session.scroll, 100.);
	assert!(session.snapshot_complete);
	assert!(!session.layout_pending);
	assert!(session.counts.chars > text.len());
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
		complete: true,
	};
	assert!(session.accept(reader.clone(), 300.0));
	assert_eq!(session.counts, TextCounts { chars: 5, words: 1 });
	assert!(!session.accept(reader, 300.0));
	assert_eq!(session.counts, TextCounts { chars: 5, words: 1 });
	assert!(Arc::ptr_eq(session.document.as_ref().unwrap(), &document));
}
