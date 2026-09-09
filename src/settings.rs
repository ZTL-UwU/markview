//! Reader preferences, isolated from launch flags and document state.
use crate::{layout::LayoutOptions, render::Theme};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

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
		}
	}
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
	version: u32,
	/// Absent means "follow the system theme"; only a user choice is stored.
	#[serde(skip_serializing_if = "Option::is_none")]
	style: Option<Vec<String>>,
	#[serde(
		rename = "fontdef-override",
		default,
		skip_serializing_if = "Vec::is_empty"
	)]
	fontdef_overrides: Vec<FontDefOverride>,
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
			style: None,
			fontdef_overrides: Vec::new(),
			font_size: settings.font_size,
			width: settings.width,
			justify: settings.justify,
			hyphenate: settings.hyphenate,
		}
	}
}

impl Config {
	fn reader_settings(&self) -> ReaderSettings {
		let style = self.style.clone().or_else(|| {
			self.theme.map(|theme| {
				vec![
					if theme == Theme::Dark {
						"dark"
					} else {
						"light"
					}
					.into(),
				]
			})
		});
		let theme = style
			.as_ref()
			.map(|ids| {
				if ids.first().is_some_and(|id| id == "dark") {
					Theme::Dark
				} else {
					Theme::Light
				}
			})
			.unwrap_or_default();
		ReaderSettings {
			theme,
			style,
			fontdef_overrides: self.fontdef_overrides.clone(),
			font_size: self.font_size,
			width: self.width,
			justify: self.justify,
			hyphenate: self.hyphenate,
			..Default::default()
		}
	}
}

pub struct SettingsStore {
	path: Option<PathBuf>,
	saved: ReaderSettings,
	invalid: Option<Vec<u8>>,
	dirty: bool,
	pending: Vec<Setting>,
	source: Option<Vec<u8>>,
}
impl SettingsStore {
	pub fn load(path: Option<PathBuf>) -> (Self, Option<String>) {
		let mut store = Self {
			path,
			saved: ReaderSettings::default(),
			invalid: None,
			dirty: false,
			pending: Vec::new(),
			source: None,
		};
		let mut warning = None;
		if let Some(path) = &store.path {
			match fs::read(path) {
				Ok(bytes) => {
					store.source = Some(bytes.clone());
					let result = (|| -> Result<Config> {
						let config: Config = toml_edit::de::from_str(
							std::str::from_utf8(&bytes)?,
						)?;
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
							store.saved = config.reader_settings();
							if let Err(error) = store.saved.validate() {
								warning = Some(format!(
									"Settings: {error}; using defaults"
								));
								store.saved = ReaderSettings::default();

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
	pub fn path(&self) -> Option<&std::path::Path> {
		self.path.as_deref()
	}
	pub fn has_pending_changes(&self) -> bool {
		self.dirty
	}
	/// Invalid or temporarily missing files never replace the last good values.
	pub fn reload(&mut self) -> Result<bool> {
		let Some(path) = &self.path else {
			return Ok(false);
		};
		let bytes = fs::read(path)
			.context("Cannot read settings; keeping current values")?;
		if self.source.as_ref() == Some(&bytes) {
			return Ok(false);
		}
		let config: Config =
			toml_edit::de::from_str(std::str::from_utf8(&bytes)?)
				.context("Invalid settings; keeping current values")?;
		if config.version != 1 {
			bail!(
				"Unsupported settings version {}; keeping current values",
				config.version
			);
		}
		let saved = config.reader_settings();
		saved
			.validate()
			.context("Invalid settings; keeping current values")?;
		let mut next = Self {
			path: self.path.clone(),
			saved,

			invalid: None,
			dirty: self.dirty,
			pending: self.pending.clone(),
			source: Some(bytes),
		};
		for field in &self.pending {
			next.saved.copy_field(&self.saved, *field);
		}
		next.pending = self.pending.clone();
		next.dirty = self.dirty;
		*self = next;
		Ok(true)
	}
	pub fn ensure_file(&mut self) -> Result<()> {
		let path = self
			.path
			.as_ref()
			.context("No user configuration directory available")?;
		if !path.exists() {
			let legacy = path.with_extension("json");
			if self.source.is_none() && !self.dirty && legacy.exists() {
				let config: Config =
					serde_json::from_slice(&fs::read(&legacy)?)?;
				if config.version != 1 {
					bail!("Unsupported legacy settings version");
				}
				let saved = config.reader_settings();
				saved.validate()?;
				self.saved = saved;
			}
			self.dirty = true;
			self.flush()?;
		}
		Ok(())
	}
	pub fn settings(&self) -> ReaderSettings {
		self.saved.clone()
	}
	/// The saved theme, or `None` while the reader still follows the system.
	pub fn theme_preference(&self) -> Option<Theme> {
		self.saved.style.as_ref().map(|ids| {
			if ids.first().is_some_and(|id| id == "dark") {
				Theme::Dark
			} else {
				Theme::Light
			}
		})
	}
	pub fn follow_system(&mut self) {
		self.saved.style = None;
		if !self.pending.contains(&Setting::Theme) {
			self.pending.push(Setting::Theme);
		}
		self.dirty = true;
	}
	pub fn changed(
		&mut self,
		effective: &ReaderSettings,
		field: Option<Setting>,
	) {
		match field {
			Some(field) => {
				if !self.pending.contains(&field) {
					self.pending.push(field);
				}
				self.saved.copy_field(effective, field);
				if field == Setting::Theme {
					self.saved.style = effective.style.clone().or_else(|| {
						Some(vec![
							if effective.theme == Theme::Dark {
								"dark"
							} else {
								"light"
							}
							.into(),
						])
					});
				}
			}
			None => {
				self.pending = vec![
					Setting::Theme,
					Setting::FontSize,
					Setting::Width,
					Setting::Justify,
					Setting::Hyphenate,
				];
				self.saved = effective.clone();
				self.saved.style = None;
			}
		}
		self.dirty = true;
	}
	pub fn flush(&mut self) -> Result<()> {
		if !self.dirty {
			return Ok(());
		}
		// Merge external edits before saving a pending UI change.
		if self.path.as_ref().is_some_and(|p| p.exists()) {
			self.reload()?;
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
				.suffix(".toml")
				.tempfile_in(parent)?;
			backup.write_all(bytes)?;
			backup.as_file().sync_all()?;
			backup.keep()?;
			self.invalid = None;
		}
		self.saved.validate()?;
		let config = Config {
			version: 1,
			theme: None,
			style: self.saved.style.clone(),
			fontdef_overrides: self.saved.fontdef_overrides.clone(),
			font_size: self.saved.font_size,
			width: self.saved.width,
			justify: self.saved.justify,
			hyphenate: self.saved.hyphenate,
		};
		let values = toml_edit::ser::to_document(&config)?;
		let mut document = self
			.source
			.as_ref()
			.and_then(|b| std::str::from_utf8(b).ok())
			.and_then(|s| s.parse::<toml_edit::DocumentMut>().ok())
			.unwrap_or_default();
		for (key, value) in values.iter() {
			let mut value = value.clone();
			if let (Some(old), Some(new)) = (
				document.get(key).and_then(|v| v.as_value()),
				value.as_value_mut(),
			) {
				*new.decor_mut() = old.decor().clone();
			}
			document[key] = value;
		}
		let mut comments = String::new();
		for key in [Some("theme"), config.style.is_none().then_some("style")]
			.into_iter()
			.flatten()
		{
			if let Some(prefix) = document
				.key(key)
				.and_then(|k| k.leaf_decor().prefix())
				.and_then(|p| p.as_str())
				&& prefix.contains('#')
			{
				comments.push_str(prefix);
				if !prefix.ends_with('\n') {
					comments.push('\n');
				}
			}
			if let Some(suffix) = document
				.get(key)
				.and_then(|v| v.as_value())
				.and_then(|v| v.decor().suffix())
				.and_then(|p| p.as_str())
				&& suffix.contains('#')
			{
				comments.push_str(suffix.trim_start());
				if !suffix.ends_with('\n') {
					comments.push('\n');
				}
			}
			document.remove(key);
		}
		let bytes = format!("{comments}{document}").into_bytes();
		let mut temp = tempfile::NamedTempFile::new_in(parent)?;
		temp.write_all(&bytes)?;
		temp.as_file().sync_all()?;
		temp.persist(path)?;
		// Durability of the rename itself needs the directory entry flushed.
		if let Ok(dir) = fs::File::open(parent) {
			let _ = dir.sync_all();
		}
		self.dirty = false;
		self.pending.clear();
		self.source = Some(bytes);
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
	base.map(|p| p.join("markview/settings.toml"))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn external_edits_merge_pending_ui_fields_and_preserve_comments() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.toml");
		fs::write(&path, "# Reading\nfont_size = 20.0 # comfortable\nwidth = 800.0\n[extra]\nvalue = 42\n").unwrap();
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_none());
		let mut ui = store.settings();
		ui.font_size = 24.0;
		store.changed(&ui, Some(Setting::FontSize));
		fs::write(&path, "# Reading\nfont_size = 21.0 # comfortable\nwidth = 900.0\n[extra]\nvalue = 42\n").unwrap();
		store.flush().unwrap();
		let text = fs::read_to_string(&path).unwrap();
		assert!(text.contains("# Reading"));
		assert!(text.contains("# comfortable"));
		assert!(text.contains("value = 42"));
		let (loaded, warning) = SettingsStore::load(Some(path));
		assert!(warning.is_none());
		assert_eq!(loaded.settings().font_size, 24.0);
		assert_eq!(loaded.settings().width, 900.0);
		assert!(!store.reload().unwrap());
	}
	#[test]
	fn invalid_reload_and_deletion_retain_last_good_settings() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.toml");
		fs::write(&path, "font_size = 22\n").unwrap();
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_none());
		for invalid in [
			"font_size =",
			"font_size = 99",
			"width = nan",
			"theme = 'unknown'",
			"version = 2",
		] {
			fs::write(&path, invalid).unwrap();
			assert!(store.reload().is_err());
			assert_eq!(store.settings().font_size, 22.0);
		}
		let mut ui = store.settings();
		ui.justify = false;
		store.changed(&ui, Some(Setting::Justify));
		assert!(store.flush().is_err());
		assert_eq!(fs::read_to_string(&path).unwrap(), "version = 2");
		fs::remove_file(&path).unwrap();
		assert!(store.reload().is_err());
		fs::write(&path, "font_size = 26\n").unwrap();
		assert!(store.reload().unwrap());
		assert_eq!(store.settings().font_size, 26.0);
		assert!(!store.settings().justify);
	}
	#[test]
	fn legacy_json_is_migrated_without_modifying_original() {
		let dir = tempfile::tempdir().unwrap();
		let legacy = dir.path().join("settings.json");
		let original = r#"{"version":1,"font_size":23,"theme":"dark"}"#;
		fs::write(&legacy, original).unwrap();
		let path = dir.path().join("settings.toml");
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		store.ensure_file().unwrap();
		assert_eq!(fs::read_to_string(legacy).unwrap(), original);
		assert_eq!(store.settings().font_size, 23.0);
		assert_eq!(store.theme_preference(), Some(Theme::Dark));
		assert!(SettingsStore::load(Some(path)).1.is_none());
		store.follow_system();
		store.flush().unwrap();
		assert_eq!(store.theme_preference(), None);
	}
	#[test]
	fn toml_atomic_save_is_watched_and_applied() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("settings.toml");
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		store.ensure_file().unwrap();
		let (tx, rx) = std::sync::mpsc::channel();
		let _watch = crate::watch::FileWatch::new(path.clone(), move || {
			let _ = tx.send(());
		});
		let replacement = dir.path().join("save.tmp");
		fs::write(&replacement, "font_size = 28\njustify = false\n").unwrap();
		fs::rename(replacement, path).unwrap();
		rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
		assert!(store.reload().unwrap());
		assert_eq!(store.settings().font_size, 28.0);
		assert!(!store.settings().justify);
	}
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

#[cfg(test)]
mod stylesheet_tests {
	use super::*;
	#[test]
	fn stylesheet_lists_migrate_merge_and_preserve_personal_preferences() {
		let tmp = tempfile::tempdir().unwrap();
		let path = tmp.path().join("settings.toml");
		fs::write(
			&path,
			"# Preferences\ntheme='dark'\nfont_size=23\nwidth=900\n",
		)
		.unwrap();
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_none());
		assert_eq!(store.settings().style, Some(vec!["dark".into()]));
		let mut ui = store.settings();
		ui.style = Some(vec!["paper".into(), "dark".into()]);
		store.changed(&ui, Some(Setting::Theme));
		fs::write(&path,"# Preferences\ntheme='light'\nstyle=['external']\nfont_size=25\nwidth=960\n").unwrap();
		store.flush().unwrap();
		let source = fs::read_to_string(&path).unwrap();
		assert!(source.contains("# Preferences"));
		assert!(!source.contains("theme"));
		let (loaded, warning) = SettingsStore::load(Some(path));
		assert!(warning.is_none());
		assert_eq!(loaded.settings().style, ui.style);
		assert_eq!(loaded.settings().font_size, 25.);
		assert_eq!(loaded.settings().width, 960.);
	}
	#[test]
	fn empty_style_overrides_legacy_theme_and_survives_save() {
		let tmp = tempfile::tempdir().unwrap();
		let path = tmp.path().join("settings.toml");
		fs::write(&path, "theme='dark'\nstyle=[]").unwrap();
		let (mut store, _) = SettingsStore::load(Some(path.clone()));
		assert_eq!(store.settings().style, Some(vec![]));
		assert_eq!(store.settings().theme, Theme::Light);
		let mut ui = store.settings();
		ui.font_size = 22.;
		store.changed(&ui, Some(Setting::FontSize));
		store.flush().unwrap();
		assert_eq!(
			SettingsStore::load(Some(path)).0.settings().style,
			Some(vec![])
		);
	}
}
