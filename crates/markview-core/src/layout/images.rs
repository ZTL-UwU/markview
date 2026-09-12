use super::{BlockContext, fitted_range};
use crate::{
	scene::{BlockLayout, Draw, Paint, Rect},
	style::{ColorField, Role},
	text::TextCluster,
};
impl BlockContext<'_> {
	/// The image box scaled into the paragraph measure, like `max-width: 100%`.
	pub(super) fn image_size(
		&self,
		image: &crate::image::ImageSpec,
		available: f32,
		size: f32,
	) -> (f32, f32) {
		let inset = self.image_insets(size, available);
		let (w, h) = image.size(
			self.images.entries.get(&image.src),
			(available - inset[1] - inset[3]).max(1.),
		);
		(w + inset[1] + inset[3], h + inset[0] + inset[2])
	}

	pub(super) fn image_placeholder(
		&self,
		image: &crate::image::ImageSpec,
	) -> Option<String> {
		let message = match self.images.entries.get(&image.src) {
			Some(i) if i.size.is_some() && i.error.is_none() => return None,
			Some(i) if i.error.is_some() => i.error.as_deref().unwrap(),
			_ => "Loading image…",
		};
		Some(if image.alt.is_empty() {
			message.to_owned()
		} else {
			format!("{} · {message}", image.alt)
		})
	}

	pub(super) fn image_insets(&self, size: f32, available: f32) -> [f32; 4] {
		let rule = self.shaper.stylesheet.rule(Role::Image);
		let base = size / self.shaper.appearance.size;
		let border = rule.border_width.unwrap_or(0.);
		let mut inset = rule
			.padding
			.as_ref()
			.map(|p| p.sides().map(|v| v * base + border))
			.unwrap_or([border; 4]);
		let scale =
			((available - 1.).max(0.) / (inset[1] + inset[3]).max(1.)).min(1.);
		inset[1] *= scale;
		inset[3] *= scale;
		inset
	}

	pub(super) fn draw_image(
		&mut self,
		image: &crate::image::ImageSpec,
		rect: Rect,
		size: f32,
		available: f32,
		out: &mut BlockLayout,
	) -> Vec<TextCluster> {
		let mut text_clusters = Vec::new();
		let info = self.images.entries.get(&image.src);
		let inset = self.image_insets(size, available);
		let content = Rect {
			x: rect.x + inset[3],
			y: rect.y + inset[0],
			w: (rect.w - inset[1] - inset[3]).max(1.),
			h: (rect.h - inset[0] - inset[2]).max(1.),
		};
		out.draws.push(Draw::Image {
			src: image.src.clone(),
			version: info.map_or(0, |i| i.version),
			rect: content,
			title: image.title.clone(),
		});
		out.draws.push(Draw::Box {
			rect,
			role: Role::Image,
			radius: 0.,
			border: self
				.shaper
				.stylesheet
				.rule(Role::Image)
				.border_width
				.unwrap_or(0.)
				.min(inset[1])
				.min(inset[3]),
			left_only: false,
		});
		if let Some(text) = self.image_placeholder(image) {
			let rect = content;
			out.draws.push(Draw::Rect(
				rect,
				Paint::Styled(Role::ImagePlaceholder, ColorField::Background),
			));
			let old = self.shaper.appearance.clone();
			let base = size / old.size;
			self.shaper.appearance =
				self.shaper.stylesheet.text(&old, Role::ImagePlaceholder);
			let label_size = base * self.shaper.appearance.size;
			let label = self.shaper.fit(&text, base, (rect.w - 12.).max(0.));
			if rect.h >= label_size + 12. && rect.w > 12. {
				let baseline = rect.y + 6. + label_size;
				let mut cursor = rect.x + 6.;
				let command = out.draws.len();
				for c in self.shaper.shape(&label, &[], label_size, true) {
					text_clusters.push(TextCluster {
						range: fitted_range(&text, &label, c.range),
						rect: Rect {
							x: cursor,
							y: baseline - c.ascent,
							w: c.width.max(1.),
							h: c.ascent + c.descent,
						},
						rtl: c.rtl,
						command,
					});
					cursor += c.width;
				}
				out.draws.extend(self.shaper.label(
					&label,
					base,
					rect.x + 6.,
					rect.y + 6. + label_size,
					Paint::Styled(Role::ImagePlaceholder, ColorField::Color),
				));
			}
			self.shaper.appearance = old;
		}
		out.width = out.width.max(rect.x + rect.w);
		text_clusters
	}
}
