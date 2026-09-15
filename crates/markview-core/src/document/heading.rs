//! Anchors for headings, matching the slugs GitHub gives a rendered heading.
//!
//! A link such as `#getting-started` or `other.md#getting-started` names a
//! heading by its anchor: the heading text lowercased, punctuation and symbols
//! dropped, ASCII spaces replaced by hyphens, and a repeated heading suffixed
//! `-1`, `-2`, and so on in document order.

use std::collections::HashSet;

/// Per-document anchor uniqueness, assigned in reading order.
#[derive(Default)]
pub(crate) struct Anchors {
	used: HashSet<String>,
}

impl Anchors {
	/// The anchor for one heading.
	pub(crate) fn unique(&mut self, text: &str) -> String {
		let base = heading_slug(text);
		let mut anchor = base.clone();
		let mut suffix = 1;
		while self.used.contains(&anchor) {
			anchor = format!("{base}-{suffix}");
			suffix += 1;
		}
		self.used.insert(anchor.clone());
		anchor
	}
}

/// The slug of one heading. Letters and digits survive in any script; `-` and
/// `_` are kept; an ASCII space becomes a hyphen; everything else is dropped.
pub fn heading_slug(text: &str) -> String {
	let mut slug = String::with_capacity(text.len());
	for c in text.to_lowercase().chars() {
		match c {
			' ' => slug.push('-'),
			'-' | '_' => slug.push(c),
			_ if c.is_alphanumeric() => slug.push(c),
			_ => {}
		}
	}
	slug
}
