use super::controls::toolbar_right_edge;
use crate::layout::{Draw, Paint, Rect, TextShaper};
use crate::state::ReaderTab;
use markview_core::style::{ColorField as C, Role};
pub(in crate::app) struct TabBar<'a> {
	pub(super) ui: &'a mut TextShaper,
	pub(super) tabs: &'a [ReaderTab],
	pub(super) active_tab: usize,
	pub(super) cursor: (f32, f32),
	pub(super) width: f32,
}
impl TabBar<'_> {
	pub(in crate::app) fn tab_rects(&mut self) -> Vec<(Rect, usize)> {
		let width = self.width;
		let right = toolbar_right_edge(self.ui, width);
		let mut x = 10.0;
		let mut out = Vec::new();
		for (index, tab) in self.tabs.iter().enumerate() {
			let name = tab
				.path
				.file_name()
				.unwrap_or(tab.path.as_os_str())
				.to_string_lossy();
			let label_width = self.ui.text_width(&name, 12.0);
			let w = (label_width + 34.0).clamp(92.0, 240.0);
			if x + w > right - 4.0 {
				break;
			}
			out.push((
				Rect {
					x,
					y: 4.0,
					w,
					h: 32.0,
				},
				index,
			));
			x += w + 2.0;
		}
		out
	}

	pub(super) fn draw_tabs(&mut self) -> Vec<Draw> {
		let mut out = Vec::new();
		for (rect, index) in self.tab_rects() {
			let active = index == self.active_tab;
			out.push(Draw::Box {
				rect,
				role: Role::Toolbar,
				radius: 0.0,
				border: 1.0,
				left_only: false,
			});
			let fill = if active {
				C::ActiveBackground
			} else if rect.contains(self.cursor.0, self.cursor.1) {
				C::HoverBackground
			} else {
				C::Background
			};
			out.push(Draw::Rect(rect, Paint::Styled(Role::Toolbar, fill)));
			let name = self.tabs[index]
				.path
				.file_name()
				.unwrap_or(self.tabs[index].path.as_os_str())
				.to_string_lossy();
			let name = self.ui.fit(&name, 12.0, rect.w - 26.0);
			out.extend(self.ui.label(
				&name,
				12.0,
				rect.x + 12.0,
				rect.y + 21.0,
				Paint::Styled(
					Role::Toolbar,
					if active { C::Color } else { C::Muted },
				),
			));
			out.extend(self.ui.label(
				"×",
				16.0,
				rect.x + rect.w - 19.0,
				rect.y + 21.0,
				Paint::Styled(Role::Toolbar, C::Muted),
			));
		}
		out
	}
}
