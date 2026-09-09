//! Reader preferences, isolated from launch flags and document state.
use crate::{layout::LayoutOptions, render::Theme};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderSettings {
	pub theme: Theme,
	pub font_size: f32,
	pub width: f32,
	pub justify: bool,
	pub hyphenate: bool,
}
impl Default for ReaderSettings {
	fn default() -> Self {
		Self {
			theme: Theme::default(),
			font_size: 18.0,
			width: 760.0,
			justify: true,
			hyphenate: true,
		}
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
	Theme,
	FontSize,
	Width,
	Justify,
	Hyphenate,
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
		}
	}
	pub fn validate(&self) -> Result<()> {
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
			Setting::Theme => self.theme = other.theme,
			Setting::FontSize => self.font_size = other.font_size,
			Setting::Width => self.width = other.width,
			Setting::Justify => self.justify = other.justify,
			Setting::Hyphenate => self.hyphenate = other.hyphenate,
		}
	}
}
#[derive(Serialize, Deserialize)]
struct Config {
	version: u32,
	#[serde(flatten)]
	settings: ReaderSettings,
}

pub struct SettingsStore {
	path: Option<PathBuf>,
	saved: ReaderSettings,
	invalid: Option<Vec<u8>>,
	dirty: bool,
}
impl SettingsStore {
	pub fn load(path: Option<PathBuf>) -> (Self, Option<String>) {
		let mut store = Self {
			path,
			saved: ReaderSettings::default(),
			invalid: None,
			dirty: false,
		};
		let mut warning = None;
		if let Some(path) = &store.path {
			match fs::read(path) {
				Ok(bytes) => {
					let result = (|| -> Result<ReaderSettings> {
						let config: Config = serde_json::from_slice(&bytes)?;
						if config.version != 1 {
							bail!(
								"Unsupported settings version {}",
								config.version
							);
						}
						config.settings.validate()?;
						Ok(config.settings)
					})();
					match result {
						Ok(settings) => store.saved = settings,
						Err(error) => {
							warning = Some(format!(
								"Settings: {error}; using defaults"
							));
							store.invalid = Some(bytes);
						}
					}
				}
				Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
				Err(error) => {
					warning = Some(format!("Cannot read settings: {error}"))
				}
			}
		}
		(store, warning)
	}
	pub fn settings(&self) -> ReaderSettings {
		self.saved.clone()
	}
	pub fn changed(
		&mut self,
		effective: &ReaderSettings,
		field: Option<Setting>,
	) {
		if let Some(field) = field {
			self.saved.copy_field(effective, field);
		} else {
			self.saved = effective.clone();
		}
		self.dirty = true;
	}
	pub fn flush(&mut self) -> Result<()> {
		if !self.dirty {
			return Ok(());
		}
		let path = self
			.path
			.as_ref()
			.context("No user configuration directory available")?;
		let parent = path.parent().context("Invalid configuration path")?;
		fs::create_dir_all(parent)?;
		if let Some(bytes) = &self.invalid {
			let mut backup = tempfile::Builder::new()
				.prefix("settings-invalid-")
				.suffix(".json")
				.tempfile_in(parent)?;
			backup.write_all(bytes)?;
			backup.as_file().sync_all()?;
			backup.keep()?;
			self.invalid = None;
		}
		self.saved.validate()?;
		let bytes = serde_json::to_vec_pretty(&Config {
			version: 1,
			settings: self.saved.clone(),
		})?;
		let mut temp = tempfile::NamedTempFile::new_in(parent)?;
		temp.write_all(&bytes)?;
		temp.as_file().sync_all()?;
		temp.persist(path)?;
		self.dirty = false;
		Ok(())
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
	base.map(|p| p.join("markview/settings.json"))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn overrides_do_not_leak_into_saved_fields() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.json");
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		let effective = ReaderSettings {
			font_size: 30.0,
			theme: Theme::Dark,
			..Default::default()
		};
		store.changed(&effective, Some(Setting::Theme));
		store.flush().unwrap();
		let (loaded, warning) = SettingsStore::load(Some(path));
		assert!(warning.is_none());
		assert_eq!(loaded.settings().font_size, 18.0);
		assert_eq!(loaded.settings().theme, Theme::Dark);
	}
	#[test]
	fn corrupt_configuration_is_preserved_and_defaults_recover() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.json");
		fs::write(&path, b"broken json").unwrap();
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_some());
		assert_eq!(fs::read(&path).unwrap(), b"broken json");
		store.changed(&ReaderSettings::default(), None);
		store.flush().unwrap();
		assert!(
			fs::read_dir(dir.path()).unwrap().any(|p| fs::read(
				p.unwrap().path()
			)
			.unwrap()
				== b"broken json")
		);
		assert!(SettingsStore::load(Some(path)).1.is_none());
	}
}
