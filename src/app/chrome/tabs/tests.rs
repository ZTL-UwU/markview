use super::*;
use crate::app::tab_metrics::TabMetrics;

#[test]
fn compact_labels_keep_two_whole_graphemes_and_fit_the_measured_space() {
	let mut ui = TextShaper::new();
	ui.appearance = crate::app::tab_metrics::tab_appearance(&ui);
	for name in ["中文文档.md", "e\u{301}日笔记.md", "👩‍💻🙂notes.md", "a.md"]
	{
		let prefix: String = name.graphemes(true).take(2).collect();
		let width = ui.text_width(&prefix, 12.0);
		let fitted = fit_label(&mut ui, name, width);
		assert_eq!(fitted, prefix);
		assert!(ui.text_width(&fitted, 12.0) <= width + 0.001);
	}
}

#[test]
fn measured_tabs_fit_minimum_window_and_styles_invalidate_widths() {
	let mut ui = TextShaper::new();
	let mut metrics = TabMetrics::default();
	let tabs: Vec<_> = (0..30)
		.map(|i| ReaderTab::new(format!("中文文档{i}.md").into()))
		.collect();
	metrics.sync(&mut ui, &tabs);
	let old = metrics.widths.clone();
	let strip = TabStrip::default();
	let mut bar = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &metrics.widths,
		tabs: &tabs,
		active_tab: 0,
		cursor: (0.0, 0.0),
		width: 500.0,
	};
	let layout = bar.layout();
	assert!(layout.max_scroll > 0.0);
	for (rect, (_, minimum)) in layout.rects.iter().zip(&metrics.widths) {
		assert!((rect.w - minimum).abs() < 0.001);
	}
	let sheet = markview_core::style::Stylesheet::parse(
		"format_version=2\nversion=1\n[[rule]]\nwhen=['ui','toolbar']\nsize = 1.5\n",
	)
	.unwrap();
	ui.set_stylesheet(std::sync::Arc::new(sheet));
	metrics.sync(&mut ui, &tabs);
	assert!(metrics.widths[0].1 > old[0].1);
	metrics.sync(&mut ui, &[]);
	assert!(metrics.widths.is_empty());
}

#[test]
fn active_tab_connects_to_the_page_and_close_follows_hover() {
	let mut ui = TextShaper::new();
	let tabs = vec![
		ReaderTab::new("one.md".into()),
		ReaderTab::new("two.md".into()),
	];
	let mut metrics = TabMetrics::default();
	metrics.sync(&mut ui, &tabs);
	let strip = TabStrip::default();
	let mut idle = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &metrics.widths,
		tabs: &tabs,
		active_tab: 0,
		cursor: (0.0, 0.0),
		width: 800.0,
	};
	let layout = idle.layout();
	assert!((layout.viewport.y - crate::app::TITLE).abs() < 0.01);
	assert!((layout.viewport.h - crate::app::TAB).abs() < 0.01);
	assert!(
		(layout.rects[1].x - (layout.rects[0].x + layout.rects[0].w)).abs()
			< 0.01
	);
	let idle_close = close_glyph_count(&idle.draw_tabs(), &layout.rects);
	assert!(idle_close[0] > 0);
	assert_eq!(idle_close[1], 0);
	let mut hovered = TabBar {
		ui: &mut ui,
		strip: &strip,
		widths: &metrics.widths,
		tabs: &tabs,
		active_tab: 0,
		cursor: (layout.rects[1].x + 4.0, crate::app::TITLE + 4.0),
		width: 800.0,
	};
	let hovered_close = close_glyph_count(&hovered.draw_tabs(), &layout.rects);
	assert!(hovered_close[0] > 0);
	assert!(hovered_close[1] > 0);
}

fn close_glyph_count(draws: &[Draw], rects: &[Rect]) -> Vec<usize> {
	let mut counts = vec![0; rects.len()];
	fn walk(draws: &[Draw], rects: &[Rect], counts: &mut [usize]) {
		for draw in draws {
			match draw {
				Draw::Clipped { draws, .. } => walk(draws, rects, counts),
				Draw::Glyph(g) => {
					if let Some(i) = rects.iter().position(|r| {
						g.x >= r.x + r.w - 24.0 && g.x <= r.x + r.w
					}) {
						counts[i] += 1;
					}
				}
				_ => {}
			}
		}
	}
	walk(draws, rects, &mut counts);
	counts
}
