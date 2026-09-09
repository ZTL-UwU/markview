//! Bounded, read-only document loading.
use anyhow::{Context, Result, bail};
use std::{fs, io::Read, path::Path};
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub fn read_document(path: &Path) -> Result<String> {
	let mut file = fs::File::open(path)
		.with_context(|| format!("Cannot open {}", path.display()))?;
	let before = file.metadata()?;
	if !before.is_file() {
		bail!("Not a regular file: {}", path.display());
	}
	if before.len() > MAX_FILE_BYTES {
		bail!("MVP file size limit is 32 MiB");
	}
	let mut bytes = Vec::with_capacity(before.len() as usize);
	Read::by_ref(&mut file)
		.take(MAX_FILE_BYTES + 1)
		.read_to_end(&mut bytes)?;
	if bytes.len() as u64 > MAX_FILE_BYTES {
		bail!("MVP file size limit is 32 MiB");
	}
	let after = file.metadata()?;
	if before.len() != after.len()
		|| before.modified().ok() != after.modified().ok()
	{
		bail!("File is still being written; waiting for the next update");
	}
	let text = String::from_utf8(bytes)
		.context("File is not complete UTF-8; waiting for a valid update")?;
	Ok(text.trim_start_matches('\u{feff}').to_string())
}
