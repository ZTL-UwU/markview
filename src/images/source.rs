//! Source resolution and bounded reads; independent of decoding and scheduling.
use anyhow::{Context, Result, bail};
use base64::Engine;
use std::{
	fs,
	io::Read,
	path::{Path, PathBuf},
	time::SystemTime,
};
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Source {
	File(PathBuf),
	Http(String),
	Data(String),
}

pub(super) fn source(
	src: &str,
	document: &Path,
	offline: bool,
) -> Result<Source> {
	if src.is_empty() {
		bail!("Missing image source");
	}
	if Path::new(src).is_absolute() {
		return Ok(Source::File(PathBuf::from(src)));
	}
	if let Ok(url) = url::Url::parse(src) {
		return match url.scheme() {
			"http" | "https" if !offline => Ok(Source::Http(url.to_string())),
			"http" | "https" => {
				anyhow::bail!("Network images disabled (--offline)")
			}
			"file" => Ok(Source::File(
				url.to_file_path()
					.map_err(|_| anyhow::anyhow!("Invalid local file URL"))?,
			)),
			"data" => Ok(Source::Data(src.to_owned())),
			_ => anyhow::bail!("Unsupported image URL scheme"),
		};
	}
	let decoded = percent_encoding::percent_decode_str(src)
		.decode_utf8()
		.context("Invalid path encoding")?;
	let path = document
		.parent()
		.unwrap_or(Path::new("."))
		.join(decoded.as_ref());
	Ok(Source::File(fs::canonicalize(&path).unwrap_or(path)))
}

fn bounded(mut reader: impl Read) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	reader
		.by_ref()
		.take((MAX_BYTES + 1) as u64)
		.read_to_end(&mut bytes)?;
	if bytes.len() > MAX_BYTES {
		bail!("Image exceeds 32 MiB");
	}
	Ok(bytes)
}

pub(super) fn fetch(
	source: &Source,
	client: &reqwest::blocking::Client,
) -> Result<Vec<u8>> {
	match source {
		Source::File(path) => {
			let file = fs::File::open(path).context("Cannot open image")?;
			if !file.metadata()?.is_file() {
				bail!("Image is not a regular file");
			}
			bounded(file)
		}
		Source::Http(url) => {
			bounded(client.get(url).send()?.error_for_status()?)
		}
		Source::Data(uri) => {
			let (header, data) =
				uri.split_once(',').context("Invalid data URI")?;
			if !header.to_ascii_lowercase().starts_with("data:image/") {
				bail!("Data URI must contain an image");
			}
			if data.len() > MAX_BYTES * 3 {
				bail!("Image exceeds 32 MiB");
			}
			let data =
				percent_encoding::percent_decode_str(data).collect::<Vec<_>>();
			let bytes = if header.to_ascii_lowercase().ends_with(";base64") {
				base64::engine::general_purpose::STANDARD.decode(data)?
			} else {
				data
			};
			if bytes.len() > MAX_BYTES {
				bail!("Image exceeds 32 MiB");
			}
			Ok(bytes)
		}
	}
}

pub(super) fn stamp(source: &Source) -> Option<(u64, Option<SystemTime>)> {
	if let Source::File(path) = source {
		fs::metadata(path)
			.ok()
			.map(|m| (m.len(), m.modified().ok()))
	} else {
		None
	}
}
