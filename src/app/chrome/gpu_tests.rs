use crate::state::Command;
use crate::{
	document,
	layout::LayoutEngine,
	render::{Renderer, Theme, View},
};
use anyhow::Result;
use std::collections::HashMap;

use super::*;
#[test]
#[ignore = "requires a GPU; writes artifacts/refactor-ui.png"]
fn settings_and_selection_frame() -> Result<()> {
	for (width, height, theme, panel_open, filename) in [
		(800.0, 600.0, Theme::Light, true, "refactor-ui.png"),
		(800.0, 600.0, Theme::Dark, true, "settings-dark.png"),
		(500.0, 300.0, Theme::Light, true, "settings-compact.png"),
		(800.0, 600.0, Theme::Light, false, "reader-chrome.png"),
		(
			500.0,
			300.0,
			Theme::Dark,
			false,
			"reader-chrome-compact.png",
		),
	] {
		let settings = ReaderSettings {
			theme,
			..Default::default()
		};
		let document = document::parse(
			"# Reading selections\n\nSelect **English**, 中文 and $x^2$ across lines.\n\n```rust\n\tlet answer = 42;\n```\n\n| A | B |\n|---|---|\n| one | two |\n",
		);
		let snapshot = LayoutEngine::new()
			.layout(&document, &settings.layout_options(width, false));
		let interaction = InteractionState {
			panel_open,
			focus: Some(if panel_open {
				Command::Larger
			} else {
				Command::Settings
			}),
			..Default::default()
		};
		let counts = markview_core::text::TextCounts::of(
			&snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		);
		let mut overlay = vec![
			Draw::Rect(
				Rect {
					x: 0.0,
					y: 0.0,
					w: width,
					h: TOP,
				},
				Paint::Background,
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: TOP - 1.0,
					w: width,
					h: 1.0,
				},
				Paint::Border,
			),
		];
		overlay.extend(draw_footer(
			&mut TextShaper::new(),
			counts,
			Some(counts),
			None,
			"",
			width,
			height,
		));
		overlay.extend(draw_controls(
			&mut TextShaper::new(),
			&settings,
			&interaction,
			width,
			height,
		));
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		let horizontal = HashMap::new();
		let view = View {
			hovered_link: None,
			held_overflow: None,
			hovered_overflow: None,
			width: (width * 1.25) as u32,
			height: (height * 1.25) as u32,
			scale: 1.25,
			left: 20.0,
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			scroll: 0.0,
			theme: settings.theme,
			horizontal: &horizontal,
			selection: snapshot.select_all(1),
			revision: 1,
		};
		let target = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&snapshot,
			&view,
			&overlay,
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts")
			.join(filename);
		std::fs::create_dir_all(output.parent().unwrap())?;
		renderer.save_png(&target, &output)?;
		if panel_open {
			let mut settings = settings.clone();
			settings.style = Some(vec!["paper".into(), "dark".into()]);
			let mut entries =
				crate::stylesheet::catalog(None, settings.style.as_deref());
			let paper = entries.iter_mut().find(|e| e.id == "paper").unwrap();
			paper.name = "纸与墨".into();
			paper.source = "/example/styles/paper.mvss.toml".into();
			paper.error = None;
			entries.push(crate::stylesheet::Entry {
				id: "invalid".into(),
				name: "Invalid stylesheet".into(),
				source: "/example/styles/invalid.mvss.toml".into(),
				error: Some("em.font: must not be empty".into()),
			});
			let overlay = draw_styles(
				&mut TextShaper::new(),
				&settings,
				&interaction,
				&entries,
				0,
				width,
				height,
			);
			let submission = renderer.render(
				&snapshot,
				&view,
				&overlay,
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(submission))?;
			renderer.save_png(
				&target,
				&output.with_file_name(format!("styles-{filename}")),
			)?;
		}
	}
	Ok(())
}
