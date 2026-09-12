//! Markview Stylesheet v1: strict parsing, field-wise cascading and semantic text styles.
use serde::{Deserialize, Serialize};

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
	MathError,
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
		(Self::MathError, "math.error"),
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
	pub(super) fn block(self) -> bool {
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
	pub show: Option<bool>,
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
	pub theme: Option<String>,
}
impl Rule {
	pub fn overlay(&mut self, higher: &Self) {
		macro_rules! merge { ($($f:ident),*) => { $(if higher.$f.is_some(){self.$f=higher.$f.clone();})* }; }
		merge!(
			show,
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
			focus_color,
			theme
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
