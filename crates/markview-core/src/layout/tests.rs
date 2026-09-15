use super::*;
use crate::document;
use crate::document::{Inline, InlineKind, TextStyle};

#[test]
fn progressive_prefixes_share_final_geometry_and_can_be_cancelled() {
	let doc = document::parse(
		"A paragraph with **bold**, 中文 and $x^2$.\n\n".repeat(40),
	);
	let options = LayoutOptions::default();
	let mut engine = LayoutEngine::new();
	let mut prefix = None;
	let final_layout = engine
		.layout_progressive(&doc, &options, &Default::default(), |p| {
			if p.blocks.len() == 3 {
				prefix = Some(p.clone());
			}
			true
		})
		.unwrap();
	let prefix = prefix.unwrap();
	for (a, b) in prefix.blocks.iter().zip(&final_layout.blocks) {
		assert_eq!(a.y, b.y);
		assert!(Arc::ptr_eq(&a.layout, &b.layout));
	}
	let full = LayoutEngine::new().layout(&doc, &options);
	assert_eq!(full.height, final_layout.height);
	assert!(full.same_reading_text(&final_layout));
	assert_eq!(full.blocks.len(), final_layout.blocks.len());
	for (a, b) in full.blocks.iter().zip(&final_layout.blocks) {
		assert_eq!(
			(a.y, a.layout.height, a.layout.draws.len()),
			(b.y, b.layout.height, b.layout.draws.len())
		);
	}
	let mut visited = 0;
	assert!(
		engine
			.layout_progressive(&doc, &options, &Default::default(), |p| {
				visited = p.blocks.len();
				visited < 3
			})
			.is_none()
	);
	assert_eq!(visited, 3);
	assert!(engine.layout(&doc, &options).same_reading_text(&full));
}
#[test]
fn links_are_hit_testable_and_survive_reuse() {
	let d = document::parse(
		"See [the manual](https://example.com/manual) and [mail](mailto:a@b.example).\n",
	);
	let mut engine = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 400.0,
		..Default::default()
	};
	let snapshot = engine.layout(&d, &opts);
	let block = &snapshot.blocks[0];
	assert_eq!(block.layout.links.len(), 2);
	assert_eq!(block.layout.links[0].url, "https://example.com/manual");
	assert_eq!(block.layout.links[1].url, "mailto:a@b.example");
	let hit = block.layout.links[0].rect;
	let none = HashMap::new();
	assert_eq!(
		snapshot.link_at(hit.x + 1.0, block.y + hit.y + 1.0, &none),
		Some("https://example.com/manual")
	);
	assert_eq!(
		snapshot.link_at(hit.x - 6.0, block.y + hit.y + 1.0, &none),
		None
	);
	assert_eq!(snapshot.link_at(hit.x + 1.0, block.y - 1.0, &none), None);
	let again = engine.layout(&d, &opts);
	assert_eq!(again.reused, 1);
	assert_eq!(again.blocks[0].layout.links.len(), 2);
}
#[test]
fn wrapped_links_produce_one_rect_per_line() {
	let d = document::parse(
		"[an intentionally long linked phrase that wraps](https://example.com)\n",
	);
	let mut engine = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 120.0,
		..Default::default()
	};
	let snapshot = engine.layout(&d, &opts);
	let links = &snapshot.blocks[0].layout.links;
	assert!(links.len() > 1, "expected a wrapped link, got {links:?}");
	assert!(links.iter().all(|l| l.url == "https://example.com"));
	assert!(links.windows(2).all(|w| w[0].rect.y < w[1].rect.y));
}
#[test]
fn long_labels_are_trimmed_to_fit() {
	let mut engine = LayoutEngine::new();
	let short = "https://example.com";
	assert_eq!(engine.fit(short, 11.0, 500.0), short);
	let long = "https://example.com/a/very/long/path/that/keeps/going?with=query&more=1";
	let fitted = engine.fit(long, 11.0, 160.0);
	assert!(engine.text_width(&fitted, 11.0) <= 160.0);
	let (head, tail) = fitted.split_once('…').expect("ellipsis");
	assert!(long.starts_with(head) && long.ends_with(tail));
	assert!(!head.is_empty() && !tail.is_empty());
	assert!(fitted.chars().count() < long.chars().count());
}
#[test]
fn mixed_layout_is_finite_and_reused() {
	let mut engine = LayoutEngine::new();
	let d = document::parse(
		"中文标点（不应落在错误的位置），以及 **English typography** 与 $\\frac{x_1}{y}$ 混排。\n\nSecond paragraph.\n",
	);
	let opts = LayoutOptions {
		width: 280.0,
		..Default::default()
	};
	let a = engine.layout(&d, &opts);
	assert!(a.height.is_finite() && a.height > 50.0);
	assert_eq!(a.math_errors, 0);
	assert!(
		a.blocks[0]
			.layout
			.draws
			.iter()
			.any(|d| matches!(d, Draw::Math { .. }))
	);
	let b = engine.layout(&d, &opts);
	assert_eq!(b.reused, 2);
	let c = engine.layout(
		&d,
		&LayoutOptions {
			width: 400.0,
			..opts
		},
	);
	assert_eq!(c.reused, 0);
}
#[test]
fn math_errors_are_visible_and_copyable_when_enabled() {
	let doc = document::parse("$$S_2^\\*$$");
	let mut engine = LayoutEngine::new();
	let shown = engine.layout(&doc, &LayoutOptions::default());
	let selected = shown.select_all(1).unwrap();
	assert_eq!(shown.math_errors, 1);
	assert!(
		shown
			.extract_text(selected, 1)
			.contains("Undefined control sequence: \\*")
	);

	let mut stylesheet = (*crate::style::Stylesheet::bundled(false)).clone();
	stylesheet.merge(
		&crate::style::Stylesheet::parse(
			"format_version=1\nversion=1\n[math.error]\nshow=false",
		)
		.unwrap(),
	);
	let hidden = engine.layout(
		&doc,
		&LayoutOptions {
			stylesheet: Arc::new(stylesheet),
			..Default::default()
		},
	);
	assert_eq!(hidden.math_errors, 1);
	assert!(
		!hidden
			.extract_text(hidden.select_all(1).unwrap(), 1)
			.contains("Undefined control sequence")
	);
}
#[test]
fn cjk_boundaries_and_hyphenation() {
	let mut e = LayoutEngine::new();
	let mut out = BlockLayout::default();
	let rich = vec![Inline {
		kind: InlineKind::Text("（中文），排版。 extraordinary".into()),
		style: TextStyle::default(),
		source: 0..0,
	}];
	let images = Default::default();
	let mut context = BlockContext {
		shaper: &mut e.shaper,
		math: &mut e.math,
		images: &images,
		highlight_cache: e.highlights.results(),
	};
	let p = context.prepare(&rich, 18.0, &mut out);
	let units = context.units(&p, 18.0, false, true, 760.0);
	for u in &units {
		if u.after.is_some() && u.source.end < p.text.len() {
			assert!(
				!"），。"
					.contains(p.text[u.source.end..].chars().next().unwrap())
			);
			assert_ne!(&p.text[u.source.clone()], "（");
		}
	}
	assert!(
		units
			.iter()
			.any(|u| u.after.is_some_and(|b| b.hyphen_width > 0.0))
	);
}
#[test]
fn content_cache_survives_offsets_but_not_changed_references() {
	let mut e = LayoutEngine::new();
	let opts = LayoutOptions::default();
	let a = document::parse("A [link][id].\n\n[id]: https://one.example\n");
	e.layout(&a, &opts);
	let b = document::parse(
		"Inserted paragraph.\n\nA [link][id].\n\n[id]: https://one.example\n",
	);
	assert_eq!(e.layout(&b, &opts).reused, 1);
	let c = document::parse(
		"Inserted paragraph.\n\nA [link][id].\n\n[id]: https://two.example\n",
	);
	assert_eq!(e.layout(&c, &opts).reused, 1); // Only the inserted paragraph.
}
#[test]
fn anchor_follows_content_and_only_follows_bottom_when_requested() {
	fn snapshot(ids: &[u64]) -> LayoutSnapshot {
		LayoutSnapshot {
			height: ids.len() as f32 * 120.0,
			blocks: ids
				.iter()
				.enumerate()
				.map(|(i, &id)| PlacedBlock {
					id,
					source: 0..0,
					y: i as f32 * 120.0,
					layout: Arc::new(BlockLayout {
						height: 120.0,
						..Default::default()
					}),
				})
				.collect(),
			..Default::default()
		}
	}
	let old = snapshot(&[1, 2, 3, 4, 5, 6]);
	let new = snapshot(&[0, 1, 2, 3, 4, 5, 6]);
	assert_eq!(anchored_scroll(&old, &new, 310.0, 200.0, true), 430.0);
	let appended = snapshot(&[1, 2, 3, 4, 5, 6, 7]);
	assert_eq!(anchored_scroll(&old, &appended, 520.0, 200.0, true), 640.0);
	assert_eq!(anchored_scroll(&old, &appended, 520.0, 200.0, false), 520.0);
}
#[test]
fn wide_blocks_are_scrollable_and_formulas_grow_line_height() {
	let mut e = LayoutEngine::new();
	let opts = LayoutOptions {
		width: 260.0,
		..Default::default()
	};
	let d = document::parse(
		"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n\n| Long column | Another column |\n|--|--|\n| aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb |\n",
	);
	let layout = e.layout(&d, &opts);
	assert!(layout.blocks.iter().all(|b| !b.layout.overflow.is_empty()));
	let text = e.layout(&document::parse("Plain text."), &opts);
	let math = e.layout(
		&document::parse(
			"Before $\\dfrac{\\dfrac{a}{b}}{\\dfrac{c}{d}}$ after.",
		),
		&opts,
	);
	assert!(math.height > text.height);
	assert_eq!(math.math_errors, 0);
}
#[test]
fn overflowing_blocks_reserve_the_configured_scrollbar_gutter() {
	let mut e = LayoutEngine::new();
	let d = document::parse(
		"```\n01234567890123456789012345678901234567890123456789012345678901234567890\n```\n",
	);
	let opts = LayoutOptions {
		width: 260.0,
		..Default::default()
	};
	let base = e.layout(&d, &opts);
	let bundled = crate::style::Stylesheet::bundled(false).scrollbar_gutter();
	assert_eq!(base.blocks[0].layout.overflow[0].gutter, bundled);
	// A wider gutter both reserves more space and grows the block.
	let mut sheet = (*crate::style::Stylesheet::bundled(false)).clone();
	sheet.merge(
		&crate::style::Stylesheet::parse(
			"format_version=1\nversion=1\n[scrollbar]\ngutter=30.0",
		)
		.unwrap(),
	);
	let taller = e.layout(
		&d,
		&LayoutOptions {
			width: 260.0,
			stylesheet: Arc::new(sheet),
			..Default::default()
		},
	);
	assert_eq!(taller.blocks[0].layout.overflow[0].gutter, 30.0);
	let delta = taller.blocks[0].layout.height - base.blocks[0].layout.height;
	assert!((delta - (30.0 - bundled)).abs() < 0.01, "{delta}");
}
#[test]
fn benchmark_corpus_needs_no_emergency_greedy_fallback() {
	let mut e = LayoutEngine::new();
	for source in [
		include_str!("../../../../tests/fixtures/ordinary-10k.md"),
		include_str!("../../../../tests/fixtures/math-10k.md"),
	] {
		assert_eq!(source.len(), 10240);
		let d = document::parse(source);
		for width in [350.0, 760.0] {
			let s = e.layout(
				&d,
				&LayoutOptions {
					width,
					..Default::default()
				},
			);
			assert_eq!(s.math_errors, 0);
			assert_eq!(s.degraded, 0);
			assert!(s.blocks.iter().all(|b| b.layout.overflow.is_empty()));
		}
	}
}
