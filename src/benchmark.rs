//! Explicitly scoped timing: full document layout and completed offscreen GPU work.
use crate::{
	document,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot, Theme},
	render::{Renderer, View},
	watch::read_document,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{collections::HashMap, fs, path::Path, time::Instant};

#[derive(Serialize, Clone)]
pub struct Timing {
	pub read_ms: f64,
	pub parse_ms: f64,
	pub layout_ms: f64,
	pub gpu_prepare_and_complete_ms: f64,
	pub total_ms: f64,
}
#[derive(Serialize)]
pub struct Distribution {
	pub count: usize,
	pub p50_ms: f64,
	pub p95_ms: f64,
	pub max_ms: f64,
}
fn distribution(samples: &[Timing]) -> Distribution {
	let mut times: Vec<f64> = samples.iter().map(|t| t.total_ms).collect();
	times.sort_by(f64::total_cmp);
	Distribution {
		count: times.len(),
		p50_ms: times[(times.len() - 1) / 2],
		p95_ms: times
			[((times.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)],
		max_ms: *times.last().unwrap(),
	}
}

#[derive(Serialize, Default)]
pub struct Memory {
	pub linux_rss_bytes: Option<u64>,
	pub linux_peak_rss_bytes: Option<u64>,
}
pub fn memory() -> Memory {
	let text = fs::read_to_string("/proc/self/status").unwrap_or_default();
	let value = |key: &str| {
		text.lines()
			.find(|l| l.starts_with(key))
			.and_then(|l| l.split_whitespace().nth(1))
			.and_then(|v| v.parse::<u64>().ok())
			.map(|v| v * 1024)
	};
	Memory {
		linux_rss_bytes: value("VmRSS:"),
		linux_peak_rss_bytes: value("VmHWM:"),
	}
}

#[derive(Serialize)]
struct Report {
	scope: &'static str,
	adapter: String,
	file: String,
	bytes: usize,
	content_hash: String,
	physical_size: [u32; 2],
	scale: f32,
	column_width: f32,
	font_size: f32,
	initialization_ms: f64,
	first_open: Timing,
	full_layout_reopens: Distribution,
	cached_refreshes: Distribution,
	full_layout_samples: Vec<Timing>,
	cached_samples: Vec<Timing>,
	memory_after_scroll: Memory,
	tracked_gpu_bytes_excluding_driver: u64,
	degraded_paragraphs: usize,
	formula_errors: usize,
}

#[expect(clippy::too_many_arguments, reason = "CLI benchmark parameters")]
pub fn run(
	path: &Path,
	output: Option<&Path>,
	width: u32,
	height: u32,
	scale: f32,
	theme: Theme,
	iterations: usize,
	options: LayoutOptions,
) -> Result<()> {
	let init = Instant::now();
	let mut renderer = pollster::block_on(Renderer::new(None))?;
	let mut engine = LayoutEngine::new();
	let _ =
		engine.label("Markview", 14.0, 0.0, 0.0, crate::layout::Paint::Text);
	let texture = renderer.offscreen(width, height);
	let target = texture.create_view(&Default::default());
	let initialization_ms = init.elapsed().as_secs_f64() * 1000.0;
	let horizontal = HashMap::new();
	let mut view = View {
		width,
		height,
		scale,
		scroll: 0.0,
		left: ((width as f32 / scale - options.width) * 0.5).max(16.0),
		top: 24.0,
		bottom: 24.0,
		theme,
		horizontal: &horizontal,
	};
	let mut latest = LayoutSnapshot::default();
	let mut sample = |reuse: bool| -> Result<Timing> {
		if !reuse {
			engine.clear_document_cache();
		}
		let start = Instant::now();
		let text = read_document(path)?;
		let read_ms = start.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		let doc = document::parse(text);
		let parse_ms = t.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		latest = engine.layout(&doc, &options);
		let layout_ms = t.elapsed().as_secs_f64() * 1000.0;
		let t = Instant::now();
		let submission = renderer.render(&latest, &view, &[], &target)?;
		renderer.wait(Some(submission))?;
		Ok(Timing {
			read_ms,
			parse_ms,
			layout_ms,
			gpu_prepare_and_complete_ms: t.elapsed().as_secs_f64() * 1000.0,
			total_ms: start.elapsed().as_secs_f64() * 1000.0,
		})
	};
	let first_open = sample(false)?;
	let full_layout_samples = (0..iterations)
		.map(|_| sample(false))
		.collect::<Result<Vec<_>>>()?;
	let cached_samples = (0..iterations)
		.map(|_| sample(true))
		.collect::<Result<Vec<_>>>()?;
	let mut scroll = 0.0;
	while scroll < latest.height {
		view.scroll = scroll;
		let submission = renderer.render(&latest, &view, &[], &target)?;
		renderer.wait(Some(submission))?;
		scroll += (height as f32 / scale - 48.0).max(1.0);
	}
	let text = read_document(path)?;
	let report = Report {
		scope: "Release-mode target. Offscreen full layout + completed first-viewport GPU rendering; window/compositor presentation excluded. First open has cold document/glyph/math caches. Reopens clear block layouts but retain text-engine, math and glyph caches. OS file cache is not flushed.",
		adapter: renderer.adapter_name.clone(),
		file: path.display().to_string(),
		bytes: text.len(),
		content_hash: format!("{:016x}", document::fingerprint(&text)),
		physical_size: [width, height],
		scale,
		column_width: options.width,
		font_size: options.font_size,
		initialization_ms,
		first_open,
		full_layout_reopens: distribution(&full_layout_samples),
		cached_refreshes: distribution(&cached_samples),
		full_layout_samples,
		cached_samples,
		memory_after_scroll: memory(),
		tracked_gpu_bytes_excluding_driver: renderer.gpu_bytes()
			+ (width as u64 * height as u64 * 4),
		degraded_paragraphs: latest.degraded,
		formula_errors: latest.math_errors,
	};
	let json = serde_json::to_string_pretty(&report)?;
	if let Some(path) = output {
		if let Some(parent) =
			path.parent().filter(|p| !p.as_os_str().is_empty())
		{
			fs::create_dir_all(parent)?;
		}
		fs::write(path, &json)
			.with_context(|| format!("Write {}", path.display()))?;
	}
	println!("{json}");
	Ok(())
}
