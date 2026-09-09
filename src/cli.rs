//! Launch parsing; deterministic diagnostic modes do not load user settings.
use crate::{layout::LayoutOptions, render::Theme, settings::Setting};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
#[derive(Default, PartialEq, Eq)]
pub(crate) enum Mode {
	#[default]
	Window,
	Render,
	Bench,
	Smoke,
}
pub(crate) struct LaunchOptions {
	pub(crate) mode: Mode,
	pub(crate) path: Option<PathBuf>,
	pub(crate) output: Option<PathBuf>,
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) scale: f32,
	pub(crate) scroll: f32,
	pub(crate) theme: Option<Theme>,
	pub(crate) iterations: usize,
	pub(crate) options: LayoutOptions,
	pub(crate) overrides: Vec<Setting>,
}
impl Default for LaunchOptions {
	fn default() -> Self {
		Self {
			mode: Mode::Window,
			path: None,
			output: None,
			width: 1200,
			height: 800,
			scale: 1.0,
			scroll: 0.0,
			theme: None,
			iterations: 100,
			options: LayoutOptions::default(),
			overrides: Vec::new(),
		}
	}
}

pub(crate) fn arguments() -> Result<Option<LaunchOptions>> {
	parse_arguments(std::env::args_os().skip(1))
}
fn parse_arguments(
	args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Option<LaunchOptions>> {
	let mut out = LaunchOptions::default();
	let mut args = args.into_iter();
	while let Some(arg) = args.next() {
		let text = arg.to_string_lossy();
		match text.as_ref() {
			"--dark" | "--light" => out.overrides.push(Setting::Theme),
			"--font-size" => out.overrides.push(Setting::FontSize),
			"--column" => out.overrides.push(Setting::Width),
			"--left" => out.overrides.push(Setting::Justify),
			"--no-hyphens" => out.overrides.push(Setting::Hyphenate),
			_ => {}
		}
		match text.as_ref() {
			"-h" | "--help" => {
				println!(
					"Markview — native Markdown reading\n\nmarkview [FILE]\nmarkview --render FILE --output preview.png [--dark] [--scale 2]\nmarkview --bench FILE [--iterations 100] [--output metrics.json]\nmarkview --smoke-test FILE [--output window.png]\n\nOptions: --width N --height N --column N --font-size N --scroll N\n         --scale N --dark --light --left --no-hyphens --greedy\n\nKeyboard: Ctrl+O open · Ctrl+T theme · Ctrl+ +/- font size\n          Ctrl+[ / ] column width · Ctrl+L alignment · Ctrl+H hyphenation\n          arrows / PageUp / PageDown / Home / End scroll\n          Shift+wheel scroll wide blocks · Tab/Enter toolbar\n          click a link to open http, https or mailto in the system browser\n          drag / Shift+click select · Ctrl+A all · Ctrl+C copy · Ctrl+, settings\n\n--render and --bench use the real GPU pipeline offscreen.\n--greedy is a typography comparison mode."
				);
				return Ok(None);
			}
			"--render" => out.mode = Mode::Render,
			"--bench" => out.mode = Mode::Bench,
			"--smoke-test" => out.mode = Mode::Smoke,
			"--output" | "-o" => {
				out.output = Some(
					args.next().context("--output requires a path")?.into(),
				)
			}
			"--dark" => out.theme = Some(Theme::Dark),
			"--light" => out.theme = Some(Theme::Light),
			"--left" => out.options.justify = false,
			"--no-hyphens" => out.options.hyphenate = false,
			"--greedy" => out.options.greedy = true,
			"--width" | "--height" | "--column" | "--font-size" | "--scale"
			| "--scroll" | "--iterations" => {
				let value = args
					.next()
					.with_context(|| format!("{text} requires a number"))?;
				let number: f32 = value
					.to_string_lossy()
					.parse()
					.context("Invalid number")?;
				if !number.is_finite() || number < 0.0 {
					bail!("Invalid value for {text}");
				}
				match text.as_ref() {
					"--width" => out.width = (number as u32).clamp(320, 8192),
					"--height" => out.height = (number as u32).clamp(240, 8192),
					"--column" => {
						out.options.width = number.clamp(240.0, 1600.0)
					}
					"--font-size" => {
						out.options.font_size = number.clamp(10.0, 40.0)
					}
					"--scale" => out.scale = number.clamp(0.5, 4.0),
					"--scroll" => out.scroll = number,
					"--iterations" => {
						out.iterations = (number as usize).clamp(1, 10_000)
					}
					_ => {}
				}
			}
			"--" => {
				out.path = args.next().map(PathBuf::from);
				if args.next().is_some() {
					bail!("Open one document at a time");
				}
				break;
			}
			_ if text.starts_with('-') => {
				bail!("Unknown option {text}; use --help")
			}
			_ => {
				if out.path.is_some() {
					bail!("Open one document at a time");
				}
				out.path = Some(arg.into());
			}
		}
	}
	if out.mode != Mode::Window && out.path.is_none() {
		bail!("This mode requires a Markdown file");
	}
	if out.mode == Mode::Render && out.output.is_none() {
		bail!("--render requires --output preview.png");
	}
	Ok(Some(out))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn explicit_settings_and_headless_mode_are_distinct() {
		let args = parse_arguments(
			[
				"--render",
				"sample.md",
				"--output",
				"sample.png",
				"--dark",
				"--font-size",
				"23",
			]
			.map(Into::into),
		)
		.unwrap()
		.unwrap();
		assert!(args.mode == Mode::Render);
		assert_eq!(args.overrides, vec![Setting::Theme, Setting::FontSize]);
		assert_eq!(args.options.font_size, 23.0);
		let args = parse_arguments(["sample.md"].map(Into::into))
			.unwrap()
			.unwrap();
		assert!(args.overrides.is_empty());
		assert!(
			parse_arguments(["--font-size", "NaN"].map(Into::into)).is_err()
		);
	}
}
