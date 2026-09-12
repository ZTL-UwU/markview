use super::*;
#[test]
fn scrolled_links_cannot_be_activated_outside_their_clip() {
	let document = crate::document::parse(
		"| Link |\n|---|\n| [abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz](https://example.com) |\n",
	);
	let snapshot = crate::layout::LayoutEngine::new().layout(
		&document,
		&crate::layout::LayoutOptions {
			width: 100.0,
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0];
	let overflow = &block.layout.overflow[0];
	let link = &block.layout.links[0];
	let horizontal = HashMap::from([((0, 0), 80.0)]);
	assert_eq!(
		snapshot.link_at(
			overflow.rect.x - 1.0,
			block.y + link.rect.y + 1.0,
			&horizontal
		),
		None
	);
	assert_eq!(
		snapshot.link_at(
			overflow.rect.x + 5.0,
			block.y + link.rect.y + 1.0,
			&horizontal
		),
		Some("https://example.com")
	);
}
#[test]
fn viewport_translation_and_clipping_share_logical_coordinates() {
	let viewport = Viewport {
		width: 800.0,
		height: 600.0,
		left: 20.0,
		top: 68.0,
		bottom: 38.0,
		scroll: 100.0,
	};
	let rect = viewport.window_rect(Rect {
		x: 30.0,
		y: 150.0,
		w: 60.0,
		h: 25.0,
	});
	assert_eq!(viewport.document_point(rect.x, rect.y), (30.0, 150.0));
	assert!(viewport.clip().contains(rect.x, rect.y));
	assert!(!viewport.clip().contains(rect.x, 30.0));
}
#[test]
fn scrollbar_thumb_spans_the_track_and_maps_to_the_scroll_range() {
	let metrics = ScrollbarMetrics::DOCUMENT;
	let track = Rect {
		x: 786.0,
		y: 40.0,
		w: metrics.band(),
		h: 732.0,
	};
	// A document that fits the viewport shows no scrollbar at all.
	assert!(Scrollbar::vertical(track, 0.0, 100.0, 100.0, metrics).is_none());
	let top = Scrollbar::vertical(track, 0.0, 2000.0, 500.0, metrics).unwrap();
	assert_eq!(top.thumb.y, 40.0);
	assert_eq!(top.thumb.h, track.h * 0.25);
	let travel = track.h - top.thumb.h;
	let bottom =
		Scrollbar::vertical(track, 1500.0, 2000.0, 500.0, metrics).unwrap();
	assert!((bottom.thumb.y - (40.0 + travel)).abs() < 0.01);
	// Grabbing 10 px inside the thumb and dragging by the whole travel
	// moves the document by exactly the scroll maximum.
	let grab = top.grab(800.0, 50.0);
	assert_eq!(grab, 10.0);
	assert!((top.scroll_for(800.0, 50.0 + travel, grab) - 1500.0).abs() < 0.01);
	assert_eq!(top.scroll_for(800.0, -460.0, grab), 0.0);
	// A press on the empty track puts the thumb's start under the pointer.
	assert_eq!(top.scroll_for(800.0, 40.0, 0.0), 0.0);
	assert_eq!(top.scroll_for(800.0, 40.0 + travel, 0.0), 1500.0);
}
#[test]
fn scrollbar_grab_zone_is_the_band_and_the_thumb_thickens_on_hover() {
	let metrics = ScrollbarMetrics::DOCUMENT;
	let band = Rect {
		x: 786.0,
		y: 40.0,
		w: metrics.band(),
		h: 700.0,
	};
	let vertical =
		Scrollbar::vertical(band, 0.0, 2000.0, 500.0, metrics).unwrap();
	assert!(vertical.hit(786.0, 80.0));
	assert!(vertical.hit(799.0, 80.0));
	assert!(!vertical.hit(785.0, 80.0));
	assert!(!vertical.hit(790.0, 35.0));
	assert!(!vertical.hit(790.0, 745.0));
	// At rest the bar is centred and thin; the hovered thumb fills the
	// band while the track keeps its rest thickness.
	let (track, thumb) = vertical.bars(false);
	assert_eq!(track.w, metrics.thickness);
	assert_eq!(track.x, band.x + (band.w - metrics.thickness) * 0.5);
	assert_eq!(thumb.w, metrics.thickness);
	let (expanded_track, expanded_thumb) = vertical.bars(true);
	assert_eq!(expanded_track.x, track.x);
	assert_eq!(expanded_track.w, metrics.thickness);
	assert_eq!(expanded_thumb.y, vertical.thumb.y);
	assert_eq!(expanded_thumb.x, band.x);
	assert_eq!(expanded_thumb.w, metrics.thickness_hover);
	// Anywhere across the band at the thumb's own extent grabs the thumb,
	// so a press that looks like it landed on the bar never jumps.
	assert!(vertical.on_thumb(band.x, band.y + 1.0));
	assert!(vertical.on_thumb(band.x + band.w, vertical.thumb.y));
	assert!(
		vertical.on_thumb(band.x + band.w, vertical.thumb.y + vertical.thumb.h)
	);
	assert!(
		!vertical.on_thumb(band.x, vertical.thumb.y + vertical.thumb.h + 1.0)
	);
	assert!(!vertical.on_thumb(band.x - 1.0, vertical.thumb.y));
}
#[test]
fn horizontal_scrollbar_uses_the_block_gutter_and_keeps_its_thickness() {
	let metrics = ScrollbarMetrics::OVERFLOW;
	let band = Rect {
		x: 40.0,
		y: 300.0,
		w: 200.0,
		h: SCROLLBAR_GUTTER,
	};
	let bar = Scrollbar::horizontal(band, 0.0, 400.0, 200.0, metrics).unwrap();
	assert_eq!(bar.thumb.w, 100.0);
	assert_eq!(bar.thumb.x, 40.0);
	// The band is the reserved gutter, or the bar itself without one.
	assert_eq!(metrics.overflow_band(SCROLLBAR_GUTTER), SCROLLBAR_GUTTER);
	assert_eq!(metrics.overflow_band(30.0), 30.0);
	assert_eq!(metrics.overflow_band(0.0), metrics.band());
	let (track, thumb) = bar.bars(false);
	assert_eq!(track.h, metrics.thickness);
	assert_eq!(track.y, band.y + (band.h - metrics.thickness) * 0.5);
	assert_eq!(thumb.h, metrics.thickness);
	// The wide block's bar does not thicken: both states paint the same.
	let (expanded_track, expanded_thumb) = bar.bars(true);
	assert_eq!(expanded_track.h, metrics.thickness);
	assert_eq!(expanded_thumb.h, metrics.thickness_hover);
	assert_eq!(expanded_thumb.y, thumb.y);
	assert!(bar.hit(100.0, 300.0));
	assert!(bar.hit(100.0, 307.0));
	assert!(!bar.hit(100.0, 309.0));
	assert!(!bar.hit(20.0, 300.0));
	assert!(bar.on_thumb(100.0, 300.0));
	assert!(!bar.on_thumb(150.0, 300.0));
	// A press on the empty track jumps the thumb under the pointer.
	assert_eq!(bar.scroll_for(240.0, 300.0, 0.0), 200.0);
}
