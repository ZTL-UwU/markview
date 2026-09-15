//! Cache filename measurements so pointer motion never shapes offscreen tabs.
use crate::{layout::TextShaper, state::ReaderTab};
use markview_core::style::{Condition, Stylesheet, TextAppearance};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(super) struct TabMetrics {
	sheet: Option<Arc<Stylesheet>>,
	order: Vec<PathBuf>,
	measured: HashMap<PathBuf, (f32, f32)>,
	pub widths: Vec<(f32, f32)>,
}
impl TabMetrics {
	pub fn sync(&mut self, ui: &mut TextShaper, tabs: &[ReaderTab]) {
		if self
			.sheet
			.as_ref()
			.is_none_or(|sheet| !Arc::ptr_eq(sheet, &ui.stylesheet))
		{
			self.order.clear();
			self.widths.clear();
			self.measured.clear();
			self.sheet = Some(ui.stylesheet.clone());
		}
		if self.order.len() == tabs.len()
			&& self
				.order
				.iter()
				.zip(tabs)
				.all(|(path, tab)| *path == tab.path)
		{
			return;
		}
		self.order = tabs.iter().map(|tab| tab.path.clone()).collect();
		let paths: std::collections::HashSet<_> =
			tabs.iter().map(|tab| &tab.path).collect();
		self.measured.retain(|path, _| paths.contains(path));
		let old = ui.appearance.clone();
		ui.appearance = tab_appearance(ui);
		self.widths.clear();
		for tab in tabs {
			let widths =
				*self.measured.entry(tab.path.clone()).or_insert_with(|| {
					let name = tab
						.path
						.file_name()
						.unwrap_or(tab.path.as_os_str())
						.to_string_lossy();
					let prefix: String = name.graphemes(true).take(2).collect();
					let minimum = ui
						.text_width("汉字", 12.0)
						.max(ui.text_width(&prefix, 12.0))
						+ 36.0;
					let natural = (ui.text_width(&name, 12.0) + 36.0)
						.clamp(92.0, 240.0)
						.max(minimum);
					(natural, minimum)
				});
			self.widths.push(widths);
		}
		ui.appearance = old;
	}
}
pub(super) fn tab_appearance(ui: &TextShaper) -> TextAppearance {
	ui.stylesheet.text(
		&ui.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Toolbar,
	)
}
