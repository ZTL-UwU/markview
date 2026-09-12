use super::*;
fn sheet(source: &str) -> Arc<crate::style::Stylesheet> {
	let mut s = (*crate::style::Stylesheet::bundled(false)).clone();
	s.merge(&crate::style::Stylesheet::parse(source).unwrap());
	Arc::new(s)
}
#[test]
fn live_colors_follow_semantics_without_reflow() {
	let doc = crate::document::parse(
		"> *English 中文*\n\n# Heading\n\n***Both*** [*link*](https://example.com)",
	);
	let mut engine = LayoutEngine::new();
	let first = engine.layout(&doc, &LayoutOptions::default());
	let stylesheet = sheet(
		"format_version=1\nversion=1\n[blockquote]\ncolor='#123456'\n[em]\ncolor='#abcdef'\n[strong_em]\ncolor='#654321'\n[link]\ncolor='#102030'",
	);
	let second = engine.layout(
		&doc,
		&LayoutOptions {
			stylesheet: stylesheet.clone(),
			..Default::default()
		},
	);
	assert_eq!(second.reused, doc.blocks.len());
	assert_eq!(first.height, second.height);
	let glyph = second.blocks[0]
		.layout
		.draws
		.iter()
		.find_map(|d| {
			if let Draw::Glyph(g) = d {
				Some(g)
			} else {
				None
			}
		})
		.unwrap();
	assert_eq!(
		stylesheet.paint(glyph.paint),
		crate::style::Color(0xabcdefff).rgba()
	);
	let last = &second.blocks.last().unwrap().layout;
	let colors: Vec<_> = last
		.draws
		.iter()
		.filter_map(|d| {
			if let Draw::Glyph(g) = d {
				Some(stylesheet.paint(g.paint))
			} else {
				None
			}
		})
		.collect();
	assert!(colors.contains(&crate::style::Color(0x654321ff).rgba()));
	assert!(colors.contains(&crate::style::Color(0x102030ff).rgba()));
}
#[test]
fn geometry_changes_invalidate_cache_and_keep_reading_text() {
	let doc = crate::document::parse(
		"# Heading\n\nText\n\n- item\n\n| A | B |\n|---|---|\n| C | D |",
	);
	let mut engine = LayoutEngine::new();
	let first = engine.layout(&doc, &LayoutOptions::default());
	let options = LayoutOptions {
		stylesheet: sheet(
			"format_version=1\nversion=1\n[body]\npadding=1.0\n[h1]\nsize=2.5\n[p]\nline_height=2.0\n[list_item]\npadding=0.5\n[table.cell]\npadding=1.0",
		),
		..Default::default()
	};
	let second = engine.layout(&doc, &options);
	assert_eq!(second.reused, 0);
	assert!(second.height > first.height);
	assert_eq!(
		first.extract_text(first.select_all(1).unwrap(), 1),
		second.extract_text(second.select_all(1).unwrap(), 1)
	);
	assert!(
		second.blocks[0]
			.layout
			.draws
			.iter()
			.filter_map(|d| if let Draw::Glyph(g) = d {
				Some(g.size)
			} else {
				None
			})
			.all(|size| size == 45.)
	);
	assert!(second.blocks[2].layout.draws.iter().any(|d| matches!(
		d,
		Draw::Box {
			role: Role::ListItem,
			..
		}
	)));
}
