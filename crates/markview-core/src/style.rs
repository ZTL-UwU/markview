//! Stylesheet cascading and resolved semantic appearance.
mod parse;
mod types;
use crate::{
	document::TextStyle,
	scene::{Paint, SCROLLBAR_GUTTER, ScrollbarMetrics},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
	collections::BTreeMap,
	sync::{Arc, OnceLock},
};
pub use types::{
	CaptionSource, CjkType, Color, ColorField, Decoration, Font, FontDefType,
	FontDefinition, Padding, Role, Rule, TextAlign, Variant,
};

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
	pub name: Option<String>,
	pub description: Option<String>,
	pub author: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stylesheet {
	/// Version of the theme represented by this stylesheet.
	pub version: u64,
	pub fontdefs: BTreeMap<String, FontDefinition>,
	fontdef_variants: BTreeMap<(String, Option<FontDefType>), FontDefinition>,
	cjk_type: CjkType,
	pub meta: Metadata,
	pub rules: BTreeMap<Role, Rule>,
}
impl Stylesheet {
	pub fn rule(&self, role: Role) -> &Rule {
		static EMPTY: OnceLock<Rule> = OnceLock::new();
		self.rules
			.get(&role)
			.unwrap_or_else(|| EMPTY.get_or_init(Rule::default))
	}
	/// Thicknesses of the reader's vertical scrollbar.
	pub fn scrollbar_metrics(&self) -> ScrollbarMetrics {
		let rule = self.rule(Role::Scrollbar);
		ScrollbarMetrics {
			thickness: rule
				.thickness
				.unwrap_or(ScrollbarMetrics::DOCUMENT.thickness),
			thickness_hover: rule
				.thickness_hover
				.unwrap_or(ScrollbarMetrics::DOCUMENT.thickness_hover),
		}
	}
	/// Thicknesses of a wide block's horizontal scrollbar. Setting both fields
	/// to the same value disables the thickening on hover.
	pub fn overflow_scrollbar_metrics(&self) -> ScrollbarMetrics {
		let rule = self.rule(Role::Scrollbar);
		ScrollbarMetrics {
			thickness: rule
				.overflow_thickness
				.unwrap_or(ScrollbarMetrics::OVERFLOW.thickness),
			thickness_hover: rule
				.overflow_thickness_hover
				.unwrap_or(ScrollbarMetrics::OVERFLOW.thickness_hover),
		}
	}
	/// Space an overflowing block reserves below its content for its
	/// horizontal scrollbar.
	pub fn scrollbar_gutter(&self) -> f32 {
		self.rule(Role::Scrollbar)
			.gutter
			.unwrap_or(SCROLLBAR_GUTTER)
	}
	pub fn merge(&mut self, higher: &Self) {
		for (key, def) in &higher.fontdef_variants {
			self.fontdef_variants.insert(key.clone(), def.clone());
		}
		self.resolve_fontdefs();
		for (r, v) in &higher.rules {
			self.rules.entry(*r).or_default().overlay(v);
		}
	}
	pub(super) fn resolve_fontdefs(&mut self) {
		let mut resolved = BTreeMap::new();
		for ((id, ty), def) in &self.fontdef_variants {
			if ty.is_none() {
				resolved.insert(id.clone(), def.clone());
			}
		}
		if self.cjk_type != CjkType::None {
			let selected = match self.cjk_type {
				CjkType::Sc => FontDefType::Sc,
				CjkType::Tc => FontDefType::Tc,
				CjkType::Jp => FontDefType::Jp,
				CjkType::None => unreachable!(),
			};
			for ((id, ty), def) in &self.fontdef_variants {
				if *ty == Some(selected) {
					resolved.insert(id.clone(), def.clone());
				}
			}
		}
		self.fontdefs = resolved;
	}
	pub fn set_cjk_type(&mut self, cjk_type: CjkType) {
		self.cjk_type = cjk_type;
		self.resolve_fontdefs();
	}
	pub fn has_fontdef_variant(&self, id: &str) -> bool {
		self.fontdef_variants
			.keys()
			.any(|(candidate, _)| candidate == id)
	}
	pub fn apply_font_overrides(
		&mut self,
		overrides: &[(String, String)],
	) -> Result<()> {
		for (id, name) in overrides {
			let def = self.fontdefs.get_mut(id).with_context(|| {
				format!("fontdef override: unknown id {id:?}")
			})?;
			if name.trim().is_empty() {
				bail!("fontdef override {id:?}: empty font name");
			}
			def.lookfor = vec![name.clone()];
		}
		Ok(())
	}
	/// Raw bundled declarations, without implicitly merging light into dark.
	pub fn bundled_rules(dark: bool) -> Arc<Self> {
		if !dark {
			return Self::bundled(false);
		}
		static DARK: OnceLock<Arc<Stylesheet>> = OnceLock::new();
		DARK.get_or_init(|| {
			Arc::new(
				Self::parse(include_str!("../styles/dark.mvss.toml"))
					.expect("bundled dark stylesheet"),
			)
		})
		.clone()
	}
	pub fn bundled(dark: bool) -> Arc<Self> {
		static LIGHT: OnceLock<Arc<Stylesheet>> = OnceLock::new();
		static DARK: OnceLock<Arc<Stylesheet>> = OnceLock::new();
		if dark {
			DARK.get_or_init(|| {
				let mut s = (*Self::bundled(false)).clone();
				s.merge(&Self::bundled_rules(true));
				Arc::new(s)
			})
			.clone()
		} else {
			LIGHT
				.get_or_init(|| {
					Arc::new(
						Self::parse(include_str!("../styles/light.mvss.toml"))
							.expect("bundled light stylesheet"),
					)
				})
				.clone()
		}
	}
	pub fn paint(&self, paint: Paint) -> [f32; 4] {
		use ColorField as C;
		use Role as R;
		if let Paint::Cascade(mut chain, field) = paint {
			while chain != 0 {
				let index = (chain & 63) as usize;
				chain >>= 6;
				if let Some((role, _)) =
					index.checked_sub(1).and_then(|i| Role::ALL.get(i))
					&& let Some(color) =
						self.rule(*role).color(field).or_else(|| {
							(*role == Role::LinkHover)
								.then(|| self.rule(Role::Link).color(field))
								.flatten()
						}) {
					return color.rgba();
				}
			}
			return if field == C::Color {
				self.color(R::Body, C::Color)
			} else {
				Color(0).rgba()
			};
		}
		let (r, c) = match paint {
			Paint::Cascade(..) => unreachable!(),
			Paint::Color(color) => return color.rgba(),
			Paint::Styled(r, c) => (r, c),
			Paint::Text => (R::Body, C::Color),
			Paint::Background => (R::Body, C::Background),
			Paint::Muted => (R::Ui, C::Muted),
			Paint::Accent => (R::Ui, C::Accent),
			Paint::Border => (R::Ui, C::BorderColor),
			Paint::Panel => (R::Button, C::Background),
			Paint::Glass => (R::Panel, C::Background),
			Paint::Scrim => (R::Ui, C::Scrim),
			Paint::Shadow => (R::Ui, C::Shadow),
			Paint::Error => (R::Ui, C::Error),
		};
		self.color(r, c)
	}
	pub fn color(&self, role: Role, field: ColorField) -> [f32; 4] {
		self.rule(role)
			.color(field)
			.or_else(|| {
				if role == Role::LinkHover {
					self.rule(Role::Link).color(field)
				} else {
					None
				}
			})
			.or_else(|| {
				if role == Role::TableHeader {
					self.rule(Role::TableCell).color(field)
				} else {
					None
				}
			})
			.or_else(|| {
				if matches!(role, Role::TableHeader | Role::TableCell)
					&& field == ColorField::BorderColor
				{
					self.rule(Role::Table).border_color
				} else {
					None
				}
			})
			.or_else(|| {
				if role.ui() {
					self.rule(Role::Ui).color(field)
				} else {
					None
				}
			})
			.unwrap_or_else(|| match field {
				ColorField::Color => {
					self.rule(Role::Body).color.unwrap_or(Color(0x262b30ff))
				}
				ColorField::Background if role == Role::Body => {
					Color(0xfafaf8ff)
				}
				_ => Color(0),
			})
			.rgba()
	}
	/// Colors are resolved by the renderer; only geometry-affecting declarations invalidate layout.
	pub fn layout_key(&self) -> u64 {
		let mut s = String::new();
		for &(role, _) in Role::ALL {
			let r = self.rule(role);
			s.push_str(&format!("{:?}{:?}{:?}", r.source, r.align, r.show));
			s.push_str(&format!(
				"{role:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
				r.font,
				r.weight,
				r.size,
				r.decoration,
				r.line_height,
				r.space_before,
				r.space_after,
				r.padding,
				r.border_width,
				r.radius,
				r.gutter
			));
		}
		crate::document::fingerprint(&s)
	}
	pub fn text(&self, parent: &TextAppearance, role: Role) -> TextAppearance {
		let mut out = parent.clone();
		let r = self.rule(role);
		if role == Role::StrongEm {
			out = self.text(parent, Role::Em);
			out.weight =
				self.rule(Role::Strong).weight.unwrap_or(parent.weight);
		}
		out.paint = out.paint.cascade(role, ColorField::Color);
		out.background = Some(
			out.background
				.unwrap_or(Paint::Cascade(0, ColorField::Background))
				.cascade(role, ColorField::Background),
		);
		if let Some(v) = &r.font {
			out.font = v.clone();
		}
		if let Some(v) = r.weight {
			out.weight = v;
		}
		if let Some(v) = r.size {
			out.size = v;
		}
		if let Some(v) = r.line_height {
			out.line_height = v;
		}
		if let Some(v) = &r.decoration {
			out.decoration = v.clone();
		}
		out
	}
	pub fn inline(
		&self,
		parent: &TextAppearance,
		s: &TextStyle,
	) -> TextAppearance {
		let mut out = parent.clone();
		out.size = 1.;
		out.background = None;
		let emphasis = match (s.bold, s.italic) {
			(true, true) => Some(Role::StrongEm),
			(true, false) => Some(Role::Strong),
			(false, true) => Some(Role::Em),
			_ => None,
		};
		for r in [
			emphasis,
			s.link.as_ref().map(|_| Role::Link),
			s.strike.then_some(Role::Del),
			s.superscript.then_some(Role::Sup),
			s.code.then_some(Role::Code),
			s.math_error.then_some(Role::MathError),
		]
		.into_iter()
		.flatten()
		{
			out = self.text(&out, r);
		}
		if let Some(color) = s.color {
			out.paint = Paint::Color(color);
		}
		out
	}
}
#[derive(Clone, Debug)]
pub struct TextAppearance {
	pub font: Vec<Font>,
	pub weight: u16,
	pub size: f32,
	pub line_height: f32,
	pub paint: Paint,
	pub background: Option<Paint>,
	pub decoration: Vec<Decoration>,
}
impl Default for TextAppearance {
	fn default() -> Self {
		Self {
			font: vec![Font {
				family: "serif".into(),
				variant: Variant::Normal,
				weight: None,
			}],
			weight: 400,
			size: 1.,
			line_height: 1.65,
			paint: Paint::Styled(Role::Body, ColorField::Color),
			background: None,
			decoration: vec![],
		}
	}
}
#[cfg(test)]
mod tests;
