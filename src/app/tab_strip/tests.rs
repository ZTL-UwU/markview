use super::*;
fn viewport() -> Rect {
	Rect {
		x: 10.0,
		y: 4.0,
		w: 300.0,
		h: 32.0,
	}
}
#[test]
fn widths_shrink_before_scrolling_and_never_below_minimum() {
	let small = TabLayout::new(viewport(), &[(100.0, 60.0); 2], 50.0);
	assert_eq!(small.rects[0].w, 100.0);
	assert_eq!(small.scroll, 0.0);
	let compressed = TabLayout::new(viewport(), &[(120.0, 60.0); 4], 0.0);
	assert_eq!(compressed.max_scroll, 0.0);
	assert_eq!(compressed.rects[0].w, 75.0);
	let overflow = TabLayout::new(viewport(), &[(120.0, 60.0); 8], 10000.0);
	assert_eq!(overflow.rects[0].w, 60.0);
	assert_eq!(overflow.scroll, overflow.max_scroll);
	assert_eq!(overflow.rects.last().unwrap().x + 60.0, 310.0);
}
#[test]
fn scrolling_clips_hits_and_can_reveal_every_tab() {
	let widths = [(120.0, 60.0); 20];
	let layout = TabLayout::new(viewport(), &widths, 25.0);
	assert_eq!(layout.hit(9.0, 10.0), None);
	assert_eq!(layout.hit(311.0, 10.0), None);
	assert_eq!(layout.hit(12.0, 10.0), Some(0));
	assert_eq!(layout.hit(12.0, 38.0), None);
	for i in 0..20 {
		let revealed = TabLayout::new(viewport(), &widths, layout.reveal(i));
		let rect = revealed.rects[i];
		assert!(rect.x >= 10.0 && rect.x + rect.w <= 310.0);
	}
	assert_eq!(TabLayout::new(viewport(), &[], 500.0).scroll, 0.0);
}

#[test]
fn drag_threshold_multiple_neighbors_reverse_and_stationary_pointer() {
	let layout = TabLayout::new(viewport(), &[(80.0, 60.0); 4], 0.0);
	let mut drag = TabDrag {
		index: 0,
		start: 20.0,
		grab: 10.0,
		last: 20.0,
		moving: false,
	};
	drag.update(&layout, 24.0);
	assert!(!drag.moving);
	assert_eq!(drag.index, 0);
	drag.update(&layout, 220.0);
	assert!(drag.moving);
	assert_eq!(drag.index, 2);
	drag.update(&layout, 220.0);
	assert_eq!(drag.index, 2);
	drag.update(&layout, 20.0);
	assert_eq!(drag.index, 0);
}

#[test]
fn edge_scroll_reorders_under_a_stationary_pointer_and_stops_at_bounds() {
	let widths = [(120.0, 60.0); 20];
	let mut layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut drag = TabDrag {
		index: 0,
		start: 20.0,
		grab: 10.0,
		last: 20.0,
		moving: true,
	};
	assert_eq!(layout.edge_scroll(10.0), 0.0);
	for _ in 0..200 {
		layout = TabLayout::new(
			viewport(),
			&widths,
			layout.scroll + layout.edge_scroll(310.0),
		);
		drag.update(&layout, 310.0);
	}
	assert_eq!(drag.index, 19);
	assert_eq!(layout.edge_scroll(310.0), 0.0);
	assert_eq!(layout.scroll, layout.max_scroll);
	for _ in 0..200 {
		layout = TabLayout::new(
			viewport(),
			&widths,
			layout.scroll + layout.edge_scroll(10.0),
		);
		drag.update(&layout, 10.0);
	}
	assert_eq!(drag.index, 0);
	assert_eq!(layout.scroll, 0.0);
	let mut strip = TabStrip {
		drag: Some(drag),
		scroll_at: Some(Instant::now()),
		..Default::default()
	};
	strip.cancel_drag();
	assert!(strip.drag.is_none() && strip.scroll_at.is_none());
}
