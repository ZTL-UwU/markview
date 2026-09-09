//! Reader preferences, isolated from launch flags and document state.
use crate::{layout::LayoutOptions, render::Theme};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
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
#[serde(default)]
struct Config {
	version: u32,
	/// Absent means "follow the system theme"; only a user choice is stored.
	#[serde(skip_serializing_if = "Option::is_none")]
	theme: Option<Theme>,
	font_size: f32,
	width: f32,
	justify: bool,
	hyphenate: bool,
}
impl Default for Config {
	fn default() -> Self {
		let settings = ReaderSettings::default();
		Self {
			version: 1,
			theme: None,
			font_size: settings.font_size,
			width: settings.width,
			justify: settings.justify,
			hyphenate: settings.hyphenate,
		}
	}
}

pub struct SettingsStore {
	path: Option<PathBuf>,
	saved: ReaderSettings,
	theme: Option<Theme>,
	invalid: Option<Vec<u8>>,
	dirty: bool,
}
impl SettingsStore {
	pub fn load(path: Option<PathBuf>) -> (Self, Option<String>) {
		let mut store = Self {
			path,
			saved: ReaderSettings::default(),
			theme: None,
			invalid: None,
			dirty: false,
		};
		let mut warning = None;
		if let Some(path) = &store.path {
			match fs::read(path) {
				Ok(bytes) => {
					let result = (|| -> Result<Config> {
						let config: Config = serde_json::from_slice(&bytes)?;
						if config.version != 1 {
							bail!(
								"Unsupported settings version {}",
								config.version
							);
						}
						Ok(config)
					})();
					match result {
						Ok(config) => {
							store.theme = config.theme;
							store.saved = ReaderSettings {
								theme: config.theme.unwrap_or_default(),
								font_size: config.font_size,
								width: config.width,
								justify: config.justify,
								hyphenate: config.hyphenate,
							};
							if let Err(error) = store.saved.validate() {
								warning = Some(format!(
									"Settings: {error}; using defaults"
								));
								store.saved = ReaderSettings::default();
								store.theme = None;
								store.invalid = Some(bytes);
							}
						}
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
	/// The saved theme, or `None` while the reader still follows the system.
	pub fn theme_preference(&self) -> Option<Theme> {
		self.theme
	}
	pub fn changed(
		&mut self,
		effective: &ReaderSettings,
		field: Option<Setting>,
	) {
		match field {
			Some(field) => {
				self.saved.copy_field(effective, field);
				if field == Setting::Theme {
					self.theme = Some(effective.theme);
				}
			}
			None => {
				self.saved = effective.clone();
				self.theme = None;
			}
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
			theme: self.theme,
			font_size: self.saved.font_size,
			width: self.saved.width,
			justify: self.saved.justify,
			hyphenate: self.saved.hyphenate,
		})?;
		let mut temp = tempfile::NamedTempFile::new_in(parent)?;
		temp.write_all(&bytes)?;
		temp.as_file().sync_all()?;
		temp.persist(path)?;
		// Durability of the rename itself needs the directory entry flushed.
		if let Ok(dir) = fs::File::open(parent) {
			let _ = dir.sync_all();
		}
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
	fn theme_preference_is_optional_and_only_a_choice_pins_it() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.json");
		let (store, _) = SettingsStore::load(Some(path.clone()));
		assert_eq!(store.theme_preference(), None);
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		store.changed(
			&ReaderSettings {
				theme: Theme::Dark,
				..Default::default()
			},
			Some(Setting::Theme),
		);
		store.flush().unwrap();
		let (loaded, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_none());
		assert_eq!(loaded.theme_preference(), Some(Theme::Dark));
		assert_eq!(loaded.settings().theme, Theme::Dark);
		// Another field must not turn the system theme into a pinned choice.
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		store.changed(
			&ReaderSettings {
				font_size: 24.0,
				theme: Theme::Dark,
				..Default::default()
			},
			Some(Setting::FontSize),
		);
		store.flush().unwrap();
		let (loaded, _) = SettingsStore::load(Some(path.clone()));
		assert_eq!(loaded.theme_preference(), Some(Theme::Dark));
		assert_eq!(loaded.settings().font_size, 24.0);
		// Reset returns to following the system theme.
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		store.changed(&ReaderSettings::default(), None);
		store.flush().unwrap();
		let (loaded, _) = SettingsStore::load(Some(path));
		assert_eq!(loaded.theme_preference(), None);
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
