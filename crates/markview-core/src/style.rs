//! Markview Stylesheet v1: strict parsing, field-wise cascading and semantic text styles.
use crate::{
	document::TextStyle,
	scene::{Paint, SCROLLBAR_GUTTER, ScrollbarMetrics},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
	#[default]
	Body,
	P,
	H1,
	H2,
	H3,
	H4,
	H5,
	H6,
	Blockquote,
	List,
	ListItem,
	Footnote,
	Em,
	Strong,
	StrongEm,
	Link,
	LinkHover,
	Code,
	Del,
	Sup,
	CodeBlock,
	CodeLabel,
	Table,
	TableHeader,
	TableCell,
	ListMarker,
	TaskMarker,
	Hr,
	Math,
	Selection,
	Scrollbar,
	Ui,
	Toolbar,
	Statusbar,
	Panel,
	Button,
	Image,
	ImageCaption,
	ImagePlaceholder,
}
impl Role {
	pub const ALL: &'static [(Self, &'static str)] = &[
		(Self::Body, "body"),
		(Self::P, "p"),
		(Self::H1, "h1"),
		(Self::H2, "h2"),
		(Self::H3, "h3"),
		(Self::H4, "h4"),
		(Self::H5, "h5"),
		(Self::H6, "h6"),
		(Self::Blockquote, "blockquote"),
		(Self::List, "list"),
		(Self::ListItem, "list_item"),
		(Self::Footnote, "footnote"),
		(Self::Em, "em"),
		(Self::Strong, "strong"),
		(Self::StrongEm, "strong_em"),
		(Self::Link, "link"),
		(Self::LinkHover, "link.hover"),
		(Self::Code, "code"),
		(Self::Del, "del"),
		(Self::Sup, "sup"),
		(Self::CodeBlock, "code_block"),
		(Self::CodeLabel, "code_block.label"),
		(Self::Table, "table"),
		(Self::TableHeader, "table.header"),
		(Self::TableCell, "table.cell"),
		(Self::ListMarker, "list.marker"),
		(Self::TaskMarker, "task_marker"),
		(Self::Hr, "hr"),
		(Self::Math, "math"),
		(Self::Selection, "selection"),
		(Self::Scrollbar, "scrollbar"),
		(Self::Ui, "ui"),
		(Self::Toolbar, "ui.toolbar"),
		(Self::Statusbar, "ui.statusbar"),
		(Self::Panel, "ui.panel"),
		(Self::Button, "ui.button"),
		(Self::Image, "img"),
		(Self::ImageCaption, "img.caption"),
		(Self::ImagePlaceholder, "img.placeholder"),
	];
	pub fn name(self) -> &'static str {
		Self::ALL.iter().find(|(r, _)| *r == self).unwrap().1
	}
	pub fn parse(name: &str) -> Option<Self> {
		Self::ALL.iter().find(|(_, n)| *n == name).map(|(r, _)| *r)
	}
	pub fn ui(self) -> bool {
		matches!(
			self,
			Self::Ui
				| Self::Toolbar
				| Self::Statusbar
				| Self::Panel
				| Self::Button
		)
	}
	fn block(self) -> bool {
		matches!(
			self,
			Self::Body
				| Self::P | Self::H1
				| Self::H2 | Self::H3
				| Self::H4 | Self::H5
				| Self::H6 | Self::Blockquote
				| Self::List | Self::ListItem
				| Self::Footnote
				| Self::CodeBlock
				| Self::CodeLabel
				| Self::Table
				| Self::TableHeader
				| Self::TableCell
		)
	}
	pub fn heading(level: u8) -> Self {
		[Self::H1, Self::H2, Self::H3, Self::H4, Self::H5, Self::H6]
			[level.clamp(1, 6) as usize - 1]
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorField {
	Color,
	Background,
	BorderColor,
	Muted,
	Accent,
	Error,
	Shadow,
	Scrim,
	Track,
	Thumb,
	ThumbHover,
	HoverBackground,
	ActiveBackground,
	DisabledColor,
	FocusColor,
}
impl ColorField {
	pub fn name(self) -> &'static str {
		match self {
			Self::Color => "color",
			Self::Background => "background",
			Self::BorderColor => "border_color",
			Self::Muted => "muted",
			Self::Accent => "accent",
			Self::Error => "error",
			Self::Shadow => "shadow",
			Self::Scrim => "scrim",
			Self::Track => "track",
			Self::Thumb => "thumb",
			Self::ThumbHover => "thumb_hover",
			Self::HoverBackground => "hover_background",
			Self::ActiveBackground => "active_background",
			Self::DisabledColor => "disabled_color",
			Self::FocusColor => "focus_color",
		}
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub u32);
impl Color {
	pub fn rgba(self) -> [f32; 4] {
		[
			((self.0 >> 24) & 255) as f32 / 255.,
			((self.0 >> 16) & 255) as f32 / 255.,
			((self.0 >> 8) & 255) as f32 / 255.,
			(self.0 & 255) as f32 / 255.,
		]
	}
}
impl<'de> Deserialize<'de> for Color {
	fn deserialize<D: serde::Deserializer<'de>>(
		d: D,
	) -> std::result::Result<Self, D::Error> {
		let s = String::deserialize(d)?;
		let h = s.strip_prefix('#').ok_or_else(|| {
			serde::de::Error::custom("expected #RRGGBB or #RRGGBBAA")
		})?;
		if !matches!(h.len(), 6 | 8)
			|| !h.bytes().all(|c| c.is_ascii_hexdigit())
		{
			return Err(serde::de::Error::custom(
				"expected #RRGGBB or #RRGGBBAA",
			));
		}
		let v = u32::from_str_radix(h, 16).map_err(serde::de::Error::custom)?;
		Ok(Self(if h.len() == 6 { v << 8 | 255 } else { v }))
	}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
	#[default]
	Normal,
	Italic,
	Oblique,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Font {
	pub family: String,
	#[serde(default)]
	pub variant: Variant,
	pub weight: Option<u16>,
}
#[derive(
	Clone,
	Copy,
	Debug,
	Default,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
	Hash,
	Serialize,
	Deserialize,
)]
pub enum CjkType {
	#[serde(rename = "SC")]
	Sc,
	#[serde(rename = "TC")]
	Tc,
	#[serde(rename = "JP")]
	Jp,
	#[default]
	#[serde(rename = "none")]
	None,
}
#[derive(
	Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize,
)]
pub enum FontDefType {
	#[serde(rename = "SC")]
	Sc,
	#[serde(rename = "TC")]
	Tc,
	#[serde(rename = "JP")]
	Jp,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontDefinition {
	pub id: String,
	#[serde(default)]
	pub r#type: Option<FontDefType>,
	pub lookfor: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Decoration {
	#[serde(rename = "underline")]
	Underline,
	#[serde(rename = "line-through")]
	Strike,
}
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Padding {
	All(f32),
	Sides([f32; 4]),
}
impl Padding {
	pub fn sides(&self) -> [f32; 4] {
		match self {
			Self::All(v) => [*v; 4],
			Self::Sides(v) => *v,
		}
	}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionSource {
	#[default]
	None,
	Title,
	Alt,
	TitleOrAlt,
}
impl CaptionSource {
	pub fn text(self, image: &crate::image::ImageSpec) -> Option<&str> {
		let text = match self {
			Self::None => return None,
			Self::Title => &image.title,
			Self::Alt => &image.alt,
			Self::TitleOrAlt if !image.title.trim().is_empty() => &image.title,
			Self::TitleOrAlt => &image.alt,
		};
		(!text.trim().is_empty()).then_some(text.trim())
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
	Left,
	Center,
	Right,
}
impl From<TextAlign> for crate::document::CellAlign {
	fn from(value: TextAlign) -> Self {
		match value {
			TextAlign::Left => Self::Left,
			TextAlign::Center => Self::Center,
			TextAlign::Right => Self::Right,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
	pub source: Option<CaptionSource>,
	pub align: Option<TextAlign>,
	pub color: Option<Color>,
	pub background: Option<Color>,
	pub border_color: Option<Color>,
	pub font: Option<Vec<Font>>,
	pub weight: Option<u16>,
	pub size: Option<f32>,
	pub decoration: Option<Vec<Decoration>>,
	pub line_height: Option<f32>,
	pub space_before: Option<f32>,
	pub space_after: Option<f32>,
	pub padding: Option<Padding>,
	pub border_width: Option<f32>,
	pub radius: Option<f32>,
	pub muted: Option<Color>,
	pub accent: Option<Color>,
	pub error: Option<Color>,
	pub shadow: Option<Color>,
	pub scrim: Option<Color>,
	pub track: Option<Color>,
	pub thumb: Option<Color>,
	pub thumb_hover: Option<Color>,
	pub thickness: Option<f32>,
	pub thickness_hover: Option<f32>,
	pub overflow_thickness: Option<f32>,
	pub overflow_thickness_hover: Option<f32>,
	pub gutter: Option<f32>,
	pub hover_background: Option<Color>,
	pub active_background: Option<Color>,
	pub disabled_color: Option<Color>,
	pub focus_color: Option<Color>,
}
impl Rule {
	pub fn overlay(&mut self, higher: &Self) {
		macro_rules! merge { ($($f:ident),*) => { $(if higher.$f.is_some(){self.$f=higher.$f.clone();})* }; }
		merge!(
			source,
			align,
			color,
			background,
			border_color,
			font,
			weight,
			size,
			decoration,
			line_height,
			space_before,
			space_after,
			padding,
			border_width,
			radius,
			muted,
			accent,
			error,
			shadow,
			scrim,
			track,
			thumb,
			thumb_hover,
			thickness,
			thickness_hover,
			overflow_thickness,
			overflow_thickness_hover,
			gutter,
			hover_background,
			active_background,
			disabled_color,
			focus_color
		);
	}
	pub fn color(&self, field: ColorField) -> Option<Color> {
		match field {
			ColorField::Color => self.color,
			ColorField::Background => self.background,
			ColorField::BorderColor => self.border_color,
			ColorField::Muted => self.muted,
			ColorField::Accent => self.accent,
			ColorField::Error => self.error,
			ColorField::Shadow => self.shadow,
			ColorField::Scrim => self.scrim,
			ColorField::Track => self.track,
			ColorField::Thumb => self.thumb,
			ColorField::ThumbHover => self.thumb_hover,
			ColorField::HoverBackground => self.hover_background,
			ColorField::ActiveBackground => self.active_background,
			ColorField::DisabledColor => self.disabled_color,
			ColorField::FocusColor => self.focus_color,
		}
	}
}
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
	pub fn parse(source: &str) -> Result<Self> {
		let mut doc = source.parse::<toml_edit::DocumentMut>()?;
		let format_version =
			doc.remove("format_version").and_then(|v| v.as_integer());
		if format_version != Some(1) {
			bail!("format_version: expected stylesheet format version = 1");
		}
		let version = doc
			.remove("version")
			.and_then(|v| v.as_integer())
			.context("version: expected a nonnegative integer")?;
		let version = u64::try_from(version)
			.context("version: expected a nonnegative integer")?;
		let fontdefs = if let Some(item) = doc.remove("fontdef") {
			let mut d = toml_edit::DocumentMut::new();
			d["fontdef"] = item;
			#[derive(Deserialize)]
			struct D {
				fontdef: Vec<FontDefinition>,
			}
			let defs = toml_edit::de::from_str::<D>(&d.to_string())
				.context("fontdef")?
				.fontdef;
			let mut out = BTreeMap::new();
			for def in defs {
				if def.id.trim().is_empty()
					|| def.id.chars().any(char::is_control)
					|| def.lookfor.is_empty()
					|| def.lookfor.iter().any(|name| name.trim().is_empty())
				{
					bail!("fontdef {:?}: invalid id or lookfor", def.id);
				}
				let key = (def.id.clone(), def.r#type);
				if out.insert(key.clone(), def).is_some() {
					bail!(
						"fontdef {:?} type {:?}: duplicate definition",
						key.0,
						key.1
					);
				}
			}
			out
		} else {
			BTreeMap::new()
		};
		let meta = if let Some(item) = doc.remove("meta") {
			let mut d = toml_edit::DocumentMut::new();
			d["meta"] = item;
			#[derive(Deserialize)]
			struct M {
				meta: Metadata,
			}
			toml_edit::de::from_str::<M>(&d.to_string())
				.context("meta")?
				.meta
		} else {
			Metadata::default()
		};
		let mut out = Self {
			version,
			fontdefs: BTreeMap::new(),
			fontdef_variants: fontdefs,
			cjk_type: CjkType::None,
			meta,
			..Self::default()
		};
		out.resolve_fontdefs();
		fn visit(
			out: &mut Stylesheet,
			name: &str,
			table: &dyn toml_edit::TableLike,
		) -> Result<()> {
			let role = Role::parse(name)
				.with_context(|| format!("Unknown element [{name}]"))?;
			let mut fields = toml_edit::DocumentMut::new();
			for (key, value) in table.iter() {
				if let Some(child) = value.as_table_like() {
					visit(out, &format!("{name}.{key}"), child)?;
				} else {
					validate_field(role, key)?;
					fields[key] = value.clone();
				}
			}
			let rule: Rule = toml_edit::de::from_str(&fields.to_string())
				.with_context(|| format!("[{name}]"))?;
			for (field, value, positive) in [
				("size", rule.size, true),
				("line_height", rule.line_height, true),
				("space_before", rule.space_before, false),
				("space_after", rule.space_after, false),
				("border_width", rule.border_width, false),
				("radius", rule.radius, false),
				("thickness", rule.thickness, true),
				("thickness_hover", rule.thickness_hover, true),
				("overflow_thickness", rule.overflow_thickness, true),
				(
					"overflow_thickness_hover",
					rule.overflow_thickness_hover,
					true,
				),
				("gutter", rule.gutter, false),
			] {
				if value.is_some_and(|v| {
					!v.is_finite() || if positive { v <= 0. } else { v < 0. }
				}) {
					bail!(
						"{name}.{field}: expected finite {}number",
						if positive {
							"positive "
						} else {
							"nonnegative "
						}
					);
				}
			}
			if rule.padding.as_ref().is_some_and(|p| {
				p.sides().iter().any(|v| !v.is_finite() || *v < 0.)
			}) {
				bail!("{name}.padding: expected finite nonnegative values");
			}
			if rule.weight.is_some_and(|w| !(1..=1000).contains(&w)) {
				bail!("{name}.weight: expected 1..1000");
			}
			if let Some(fonts) = &rule.font {
				if fonts.is_empty() {
					bail!("{name}.font: must not be empty");
				}
				for (i, font) in fonts.iter().enumerate() {
					if font.family.trim().is_empty()
						|| font.weight.is_some_and(|w| !(1..=1000).contains(&w))
					{
						bail!("{name}.font[{i}]: invalid family or weight");
					}
				}
			}
			if role == Role::Body
				&& rule.background.is_some_and(|c| c.0 & 255 != 255)
			{
				bail!("body.background: must be opaque");
			}
			out.rules.insert(role, rule);
			Ok(())
		}
		for (name, item) in doc.iter() {
			visit(
				&mut out,
				name,
				item.as_table_like()
					.with_context(|| format!("{name}: expected a table"))?,
			)?;
		}
		Ok(out)
	}
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
	fn resolve_fontdefs(&mut self) {
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
			s.push_str(&format!("{:?}{:?}", r.source, r.align));
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
		]
		.into_iter()
		.flatten()
		{
			out = self.text(&out, r);
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
fn validate_field(role: Role, key: &str) -> Result<()> {
	use Role::*;
	let allowed = if role == Selection {
		key == "background"
	} else if role == Image {
		matches!(
			key,
			"background"
				| "border_color"
				| "border_width"
				| "padding" | "align"
		)
	} else if role == ImageCaption {
		matches!(
			key,
			"source"
				| "align" | "color"
				| "font" | "weight"
				| "size" | "decoration"
				| "background"
				| "line_height"
				| "space_before"
				| "space_after"
		)
	} else if role == ImagePlaceholder {
		matches!(
			key,
			"color" | "font" | "weight" | "size" | "decoration" | "background"
		)
	} else if role == Scrollbar {
		matches!(
			key,
			"track"
				| "thumb" | "thumb_hover"
				| "thickness"
				| "thickness_hover"
				| "overflow_thickness"
				| "overflow_thickness_hover"
				| "gutter"
		)
	} else if role == Hr {
		matches!(
			key,
			"color" | "border_width" | "space_before" | "space_after"
		)
	} else if role == Math {
		matches!(key, "color" | "size")
	} else {
		match key {
			"color" | "font" | "weight" | "decoration" => true,
			"size" => role != Body,
			"background" => true,
			"line_height" | "space_before" | "space_after" => role.block(),
			"padding" | "border_width" | "radius" => {
				role.block() && role != CodeLabel
			}
			"border_color" => {
				(role.block() && role != CodeLabel)
					|| role.ui() || role == TaskMarker
			}
			"muted" | "accent" | "error" => role.ui(),
			"shadow" | "scrim" => role == Ui,
			"hover_background" | "active_background" | "disabled_color"
			| "focus_color" => role == Button,
			_ => false,
		}
	};
	if !allowed {
		bail!("{}.{}: unsupported field", role.name(), key);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn strict_schema() {
		for bad in [
			"[body]\ncolor='#ffffff'",
			"format_version=2\nversion=1",
			"format_version=1",
			"format_version=1\nversion=1\n[em]\nfont=[]",
			"format_version=1\nversion=1\n[p]\nsize=nan",
			"format_version=1\nversion=1\n[body]\nbackground='#ffffff00'",
			"format_version=1\nversion=1\n[em]\ncolorz='#ffffff'",
			"format_version=1\nversion=1\n[ui]\npadding=2",
			"format_version=1\nversion=1\n[math]\nfont=[{family='serif'}]",
			"format_version=1\nversion=1\n[[fontdef]]\nid='cjk'\ntype='none'\nlookfor=['serif']",
		] {
			assert!(Stylesheet::parse(bad).is_err(), "{bad}");
		}
	}
	#[test]
	fn scrollbar_sizes_are_configurable_and_validated() {
		// A stylesheet without a scrollbar rule falls back to the built-in
		// defaults; the bundled theme is free to pick its own sizes.
		let bare = Stylesheet::parse(
			"format_version=1\nversion=1\n[p]\ncolor='#000000'",
		)
		.unwrap();
		assert_eq!(bare.scrollbar_metrics(), ScrollbarMetrics::DOCUMENT);
		assert_eq!(
			bare.overflow_scrollbar_metrics(),
			ScrollbarMetrics::OVERFLOW
		);
		assert_eq!(bare.scrollbar_gutter(), SCROLLBAR_GUTTER);
		let mut sheet = (*Stylesheet::bundled(false)).clone();
		sheet.merge(
			&Stylesheet::parse(
				"format_version=1\nversion=1\n[scrollbar]\nthickness=3.0\nthickness_hover=9.0\noverflow_thickness=4.0\noverflow_thickness_hover=4.0\ngutter=12.0",
			)
			.unwrap(),
		);
		assert_eq!(
			sheet.scrollbar_metrics(),
			ScrollbarMetrics {
				thickness: 3.0,
				thickness_hover: 9.0
			}
		);
		assert_eq!(
			sheet.overflow_scrollbar_metrics(),
			ScrollbarMetrics {
				thickness: 4.0,
				thickness_hover: 4.0
			}
		);
		assert_eq!(sheet.scrollbar_gutter(), 12.0);
		// A theme that only overrides colors keeps the bundled sizes.
		sheet.merge(
			&Stylesheet::parse(
				"format_version=1\nversion=1\n[scrollbar]\nthumb='#000000'",
			)
			.unwrap(),
		);
		assert_eq!(sheet.scrollbar_metrics().thickness, 3.0);
		assert_eq!(sheet.scrollbar_gutter(), 12.0);
		for bad in [
			"format_version=1\nversion=1\n[scrollbar]\nthickness=0.0",
			"format_version=1\nversion=1\n[scrollbar]\nthickness_hover=-1.0",
			"format_version=1\nversion=1\n[scrollbar]\noverflow_thickness=0.0",
			"format_version=1\nversion=1\n[scrollbar]\ngutter=-1.0",
			"format_version=1\nversion=1\n[scrollbar]\ngutter=nan",
			"format_version=1\nversion=1\n[p]\nthickness=4.0",
		] {
			assert!(Stylesheet::parse(bad).is_err(), "{bad}");
		}
	}
	#[test]
	fn fontdefs_are_selected_by_cjk_type() {
		let source = "format_version=1\nversion=1\n[[fontdef]]\nid='cjk'\ntype='SC'\nlookfor=['SC']\n[[fontdef]]\nid='cjk'\ntype='TC'\nlookfor=['TC']";
		let mut sheet = Stylesheet::parse(source).unwrap();
		assert!(!sheet.fontdefs.contains_key("cjk"));
		sheet.set_cjk_type(CjkType::Sc);
		assert_eq!(sheet.fontdefs["cjk"].lookfor, ["SC"]);
		sheet.set_cjk_type(CjkType::Tc);
		assert_eq!(sheet.fontdefs["cjk"].lookfor, ["TC"]);
		sheet.set_cjk_type(CjkType::Jp);
		assert!(!sheet.fontdefs.contains_key("cjk"));
	}
	#[test]
	fn cascade_arrays_and_font_defaults() {
		let mut low=Stylesheet::parse("format_version=1\nversion=1\n[em]\ncolor='#123456'\nfont=[{family='Noto Serif',variant='italic'},{family='落霞文楷'}]").unwrap();
		let high = Stylesheet::parse(
			"format_version=1\nversion=2\n[em]\ncolor='#abcdef'",
		)
		.unwrap();
		low.merge(&high);
		assert_eq!(
			low.rule(Role::Em).font.as_ref().unwrap()[1].variant,
			Variant::Normal
		);
		assert_eq!(
			low.color(Role::Em, ColorField::Color),
			Color(0xabcdefff).rgba()
		);
		low.merge(
			&Stylesheet::parse(
				"format_version=1\nversion=3\n[em]\nfont=[{family='serif'}]",
			)
			.unwrap(),
		);
		assert_eq!(low.rule(Role::Em).font.as_ref().unwrap().len(), 1);
	}
	#[test]
	fn colors_do_not_change_layout_identity() {
		let mut s = (*Stylesheet::bundled(false)).clone();
		let k = s.layout_key();
		s.merge(
			&Stylesheet::parse(
				"format_version=1\nversion=2\n[em]\ncolor='#ffffff'",
			)
			.unwrap(),
		);
		assert_eq!(k, s.layout_key());
		s.merge(
			&Stylesheet::parse("format_version=1\nversion=2\n[em]\nsize=1.2")
				.unwrap(),
		);
		assert_ne!(k, s.layout_key());
		assert!(
			Stylesheet::bundled(true)
				.rule(Role::Body)
				.background
				.is_some()
		);
		let hover = Stylesheet::parse(
			"format_version=1\nversion=1\n[link]\ncolor='#123456'\n[link.hover]\ncolor='#abcdef'",
		)
		.unwrap();
		assert_eq!(
			hover.paint(Paint::Styled(Role::LinkHover, ColorField::Color)),
			Color(0xabcdefff).rgba()
		);
	}
}
