//! Fragment links: heading anchors inside the reader and across documents.
//!
//! A link may name a heading with a fragment. `#section` moves inside the
//! current document; `other.md#section` opens that document and then moves.
//! Because layout is progressive, the target heading may not exist yet, so the
//! fragment is queued on the session until its heading is laid out.
use super::App;
use std::time::{Duration, Instant};

/// A link's document part, without its fragment.
pub(super) fn link_target(link: &str) -> &str {
	link.split_once('#').map_or(link, |(target, _)| target)
}

/// A link's percent-decoded fragment, when it has a non-empty one.
pub(super) fn link_fragment(link: &str) -> Option<String> {
	let (_, fragment) = link.split_once('#')?;
	if fragment.is_empty() {
		return None;
	}
	Some(
		percent_encoding::percent_decode_str(fragment)
			.decode_utf8_lossy()
			.into_owned(),
	)
}

impl App {
	/// Queues a heading anchor and applies it as soon as it is laid out.
	pub(super) fn goto_anchor(&mut self, anchor: String) {
		self.readers.session.pending_anchor = Some(anchor);
		self.apply_anchor();
	}

	/// Applies a queued anchor, reporting a heading the finished layout lacks.
	pub(super) fn apply_anchor(&mut self) {
		let Some(result) = self.readers.session.resolve_anchor(self.viewport())
		else {
			return;
		};
		match result {
			Ok(()) => {
				self.error = false;
				self.status.clear();
				self.status_until = None;
				self.worker
					.prioritize(self.readers.session.coverage(self.viewport()));
				self.refresh_hover();
			}
			Err(anchor) => {
				self.error = true;
				self.status = format!("Heading not found: #{anchor}");
				self.status_until =
					Some(Instant::now() + Duration::from_secs(4));
			}
		}
		self.redraw();
	}
}

#[cfg(test)]
mod tests;
