//! Reader preferences, isolated from launch flags and document state.
use crate::{layout::LayoutOptions, render::Theme};
use anyhow::{Result, bail};
use markview_core::style::CjkType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod store;
pub use store::SettingsStore;

#[derive(Clone, Debug, PartialEq)]
pub struct ReaderSettings {
	pub theme: Theme,
	pub style: Option<Vec<String>>,
	pub fontdef_overrides: Vec<FontDefOverride>,
	pub stylesheet: std::sync::Arc<markview_core::style::Stylesheet>,
	pub font_size: f32,
	pub width: f32,
	pub justify: bool,
	pub hyphenate: bool,
	pub cjk_type: CjkType,
	pub codeblock_theme_override: Option<String>,
}
impl Default for ReaderSettings {
	fn default() -> Self {
		Self {
			theme: Theme::default(),
			style: None,
			fontdef_overrides: Vec::new(),
			stylesheet: markview_core::style::Stylesheet::bundled(false),
			font_size: 18.0,
			width: 760.0,
			justify: true,
			hyphenate: true,
			cjk_type: default_cjk_type(),
			codeblock_theme_override: None,
		}
	}
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontDefOverride {
	pub id: String,
	#[serde(rename = "override")]
	pub replacement: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
	Theme,
	FontSize,
	Width,
	Justify,
	Hyphenate,
	CjkType,
}
impl ReaderSettings {
	pub fn layout_options(
		&self,
		viewport_width: f32,
		greedy: bool,
	) -> LayoutOptions {
		LayoutOptions {
			width: self.width.min(viewport_width - 40.0).max(80.0),
			font_size: self.font_size,
			justify: self.justify,
			hyphenate: self.hyphenate,
			greedy,
			stylesheet: self.stylesheet.clone(),
			codeblock_theme_override: self.codeblock_theme_override.clone(),
		}
	}
	pub fn validate(&self) -> Result<()> {
		if let Some(ids) = &self.style {
			for id in ids {
				crate::stylesheet::validate_id(id)?;
			}
		}
		if !self.font_size.is_finite()
			|| !(10.0..=40.0).contains(&self.font_size)
			|| !self.width.is_finite()
			|| !(240.0..=1600.0).contains(&self.width)
		{
			bail!("Reader settings are out of range");
		}
		Ok(())
	}
	pub fn copy_field(&mut self, other: &Self, field: Setting) {
		match field {
			Setting::Theme => {
				self.theme = other.theme;
				self.style = other.style.clone();
			}
			Setting::FontSize => self.font_size = other.font_size,
			Setting::Width => self.width = other.width,
			Setting::Justify => self.justify = other.justify,
			Setting::Hyphenate => self.hyphenate = other.hyphenate,
			Setting::CjkType => self.cjk_type = other.cjk_type,
		}
	}
}
fn default_cjk_type() -> CjkType {
	let Some(locale) = sys_locale::get_locale() else {
		return CjkType::Sc;
	};
	let locale = locale.to_ascii_lowercase().replace('_', "-");
	if locale.starts_with("ja-") || locale == "ja" {
		CjkType::Jp
	} else if locale.starts_with("zh-")
		&& ["tw", "hk", "mo", "hant"]
			.iter()
			.any(|part| locale.split('-').any(|item| item == *part))
	{
		CjkType::Tc
	} else {
		CjkType::Sc
	}
}
pub fn config_path() -> Option<PathBuf> {
	#[cfg(target_os = "windows")]
	let base = std::env::var_os("APPDATA").map(PathBuf::from);
	#[cfg(target_os = "macos")]
	let base = std::env::var_os("HOME")
		.map(|p| PathBuf::from(p).join("Library/Application Support"));
	#[cfg(not(any(target_os = "windows", target_os = "macos")))]
	let base = std::env::var_os("XDG_CONFIG_HOME")
		.map(PathBuf::from)
		.filter(|p| p.is_absolute())
		.or_else(|| {
			std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config"))
		});
	base.map(|p| p.join("markview/settings.toml"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stylesheet_tests;
