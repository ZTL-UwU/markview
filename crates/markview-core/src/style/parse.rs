//! Markview Stylesheet v1: strict parsing, field-wise cascading and semantic text styles.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::{CjkType, FontDefinition, Metadata, Role, Rule, Stylesheet};
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
	} else if role == MathError {
		matches!(
			key,
			"show"
				| "color" | "font"
				| "weight" | "size"
				| "decoration"
				| "background"
				| "line_height"
		)
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
			"theme" => role == CodeBlock,
			_ => false,
		}
	};
	if !allowed {
		bail!("{}.{}: unsupported field", role.name(), key);
	}
	Ok(())
}
