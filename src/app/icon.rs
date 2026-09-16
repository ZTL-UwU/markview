//! The icon a running window advertises to the desktop environment.
//!
//! Every platform embeds the same committed byte assets, so the reader never
//! depends on a system theme to find its own icon.

#[cfg(target_os = "windows")]
#[allow(dead_code)]
const WINDOWS_ICON: &[u8] = include_bytes!("../../assets/icons/markview.ico");

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
const LINUX_ICON: &[u8] = include_bytes!("../../assets/icons/markview-128.png");

#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn decode() -> Option<(u32, u32, Vec<u8>)> {
	let dir = ico::IconDir::read(std::io::Cursor::new(WINDOWS_ICON)).ok()?;
	let entry = dir.entries().iter().max_by_key(|e| e.width())?;
	let image = entry.decode().ok()?;
	Some((image.width(), image.height(), image.rgba_data().to_vec()))
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn decode() -> Option<(u32, u32, Vec<u8>)> {
	let image = image::load_from_memory(LINUX_ICON).ok()?.into_rgba8();
	let (width, height) = image.dimensions();
	Some((width, height, image.into_raw()))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The embedded asset must decode without a window server, because every
	/// archive ships it.
	#[test]
	fn embedded_icon_decodes_to_a_square_rgba_image() {
		let (width, height, data) = decode().expect("embedded icon decodes");
		assert_eq!(width, height);
		assert!(width >= 32);
		assert_eq!(data.len(), (width * height * 4) as usize);
	}
}
