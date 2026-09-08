use ratex_types::{DisplayList, MathStyle};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct MathBox {
	pub width: f32,
	pub ascent: f32,
	pub descent: f32,
	pub size: f32,
	pub display: DisplayList,
}

#[derive(Default)]
pub struct MathEngine {
	cache: HashMap<(String, bool, u32), Result<Arc<MathBox>, String>>,
}

impl MathEngine {
	pub fn layout(
		&mut self,
		latex: &str,
		display: bool,
		size: f32,
	) -> Result<Arc<MathBox>, String> {
		let key = (latex.to_string(), display, size.to_bits());
		if let Some(value) = self.cache.get(&key) {
			return value.clone();
		}
		// Bound caches and hostile/accidentally enormous AI-generated formulas.
		if self.cache.len() >= 256 {
			self.cache.clear();
		}
		let result = if latex.len() > 16_384 {
			Err("Formula exceeds 16 KiB".into())
		} else {
			std::panic::catch_unwind(|| {
				let ast =
					ratex_parser::parse(latex).map_err(|e| e.to_string())?;
				let opts = ratex_layout::LayoutOptions {
					style: if display {
						MathStyle::Display
					} else {
						MathStyle::Text
					},
					..Default::default()
				};
				let layout = ratex_layout::layout(&ast, &opts);
				let commands = ratex_layout::to_display_list(&layout);
				let width = commands.width as f32 * size;
				let ascent = commands.height as f32 * size;
				let descent = commands.depth as f32 * size;
				if ![width, ascent, descent]
					.iter()
					.all(|x| x.is_finite() && *x >= 0.0 && *x < 1e6)
				{
					return Err("Formula has invalid dimensions".into());
				}
				Ok(Arc::new(MathBox {
					width,
					ascent,
					descent,
					size,
					display: commands,
				}))
			})
			.unwrap_or_else(|_| Err("Formula could not be laid out".into()))
		};
		self.cache.insert(key, result.clone());
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn baseline_and_errors() {
		let mut e = MathEngine::default();
		let m = e.layout(r"\frac{x_1}{\sqrt{y}}", false, 18.0).unwrap();
		assert!(m.width > 0.0 && m.ascent > 0.0 && m.descent > 0.0);
		assert!(!m.display.items.is_empty());
		assert!(e.layout(r"\frac{", false, 18.0).is_err());
		assert!(Arc::ptr_eq(
			&m,
			&e.layout(r"\frac{x_1}{\sqrt{y}}", false, 18.0).unwrap()
		));
	}
}
