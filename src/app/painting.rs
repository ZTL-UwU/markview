use crate::cli::Mode;
use crate::state::ScrollbarAxis;
use crate::{
	benchmark,
	render::{Renderer, View},
};
use gpui::{Hitbox, Window};

use super::{App, BOTTOM, TOP};

impl App {
	fn document_view(&self) -> View<'_> {
		let (width, height, scale) = self.dimensions();
		View {
			selection: self.interaction.selection,
			revision: self.readers.session.accepted_revision,
			width: (width * scale).round() as u32,
			height: (height * scale).round() as u32,
			scale,
			scroll: self.readers.session.scroll,
			left: ((width - self.readers.session.snapshot.width) / 2.0)
				.max(20.0),
			top: TOP + 10.0,
			bottom: BOTTOM + 10.0,
			theme: self.preferences.values.theme,
			horizontal: &self.readers.session.horizontal,
			hovered_link: self.interaction.hover.as_deref(),
			hovered_overflow: self.interaction.hover_overflow,
			held_overflow: self.interaction.scrollbar.and_then(
				|drag| match drag.target {
					ScrollbarAxis::Overflow { block, overflow } => {
						Some((block, overflow))
					}
					ScrollbarAxis::Document => None,
				},
			),
		}
	}

	pub(super) fn paint_frame(
		&mut self,
		window: &mut Window,
		hitbox: &Hitbox,
		cx: &mut gpui::Context<Self>,
	) {
		if self.width < 1.0 || self.height < 1.0 {
			return;
		}
		self.painter.set_pointer(
			(!self.interaction.panel_open).then_some(self.interaction.cursor),
		);
		if let Some(specs) = window.gpu_specs()
			&& !specs.device_name.is_empty()
		{
			self.compositor = specs.device_name;
		}
		let overlay = self.overlay();
		let cursor = self.cursor_style();
		{
			let (width, height, scale) = self.dimensions();
			let view = View {
				selection: self.interaction.selection,
				revision: self.readers.session.accepted_revision,
				width: (width * scale).round() as u32,
				height: (height * scale).round() as u32,
				scale,
				scroll: self.readers.session.scroll,
				left: ((width - self.readers.session.snapshot.width) / 2.0)
					.max(20.0),
				top: TOP + 10.0,
				bottom: BOTTOM + 10.0,
				theme: self.preferences.values.theme,
				horizontal: &self.readers.session.horizontal,
				hovered_link: self.interaction.hover.as_deref(),
				hovered_overflow: self.interaction.hover_overflow,
				held_overflow: self.interaction.scrollbar.and_then(|drag| {
					match drag.target {
						ScrollbarAxis::Overflow { block, overflow } => {
							Some((block, overflow))
						}
						ScrollbarAxis::Document => None,
					}
				}),
			};
			self.painter.paint(
				window,
				&self.readers.session.snapshot,
				&view,
				&overlay,
				hitbox,
				cursor,
			);
		}
		if let Some(update) = self.first_frame.take() {
			eprintln!(
				"open→GPU complete: {:.2} ms (read {:.2}, parse {:.2}, layout {:.2}); reused {} blocks; {}",
				update.requested.elapsed().as_secs_f64() * 1000.0,
				update.read_ms,
				update.parse_ms,
				update.layout_ms,
				self.readers.session.snapshot.reused,
				self.compositor
			);
			if self.args.mode == Mode::Smoke {
				eprintln!(
					"process app entry→readable GPU frame: {:.2} ms; memory {}",
					self.started.elapsed().as_secs_f64() * 1000.0,
					serde_json::to_string(&benchmark::memory())
						.unwrap_or_else(|_| "{}".into())
				);
				if let Some(output) = self.args.output.clone()
					&& let Err(error) = self.dump_png(&output, &overlay)
				{
					self.fail(format!("Smoke PNG failed: {error:#}"), cx);
					return;
				}
			}
		}
		if self.args.mode == Mode::Smoke
			&& self.readers.session.snapshot_complete
			&& self.readers.session.displayed_version
				== self.readers.session.version
		{
			self.pending_quit = true;
			cx.defer(|cx| cx.quit());
		}
	}

	fn dump_png(
		&self,
		output: &std::path::Path,
		overlay: &[markview_core::scene::Draw],
	) -> anyhow::Result<()> {
		let view = self.document_view();
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		renderer.set_stylesheet(self.preferences.values.stylesheet.clone());
		if let Some(parent) =
			output.parent().filter(|p| !p.as_os_str().is_empty())
		{
			std::fs::create_dir_all(parent)?;
		}
		let texture = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&self.readers.session.snapshot,
			&view,
			overlay,
			&texture.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		renderer.save_png(&texture, output)?;
		Ok(())
	}
}
