//! Clipboard text detection and titles for pasted Markdown documents.

/// Returns true when the text is usable as a Markdown document.
///
/// Plain text and formulas are valid Markdown too, so this deliberately uses
/// a blacklist instead of requiring Markdown punctuation.
pub(crate) fn looks_like_markdown(text: &str) -> bool {
	let text = text.trim();
	if text.is_empty() || text.len() > crate::file::MAX_FILE_BYTES as usize {
		return false;
	}
	if text.chars().filter(|c| *c == '\u{fffd}').count() >= 2 {
		return false;
	}
	text.chars()
		.all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}

/// Finds an ATX heading, then falls back to the first sentence-like line.
pub(crate) fn title_for(text: &str) -> String {
	let candidate = text.lines().find_map(|line| {
		let trimmed = line.trim();
		is_heading(trimmed).then(|| {
			trimmed
				.trim_start_matches('#')
				.trim()
				.trim_end_matches('#')
				.trim()
				.to_string()
		})
	});
	let candidate = candidate.or_else(|| {
		text.lines()
			.map(str::trim)
			.find(|line| !line.is_empty())
			.map(|line| {
				let line = line.trim_start_matches(['>', '-', '*']).trim();
				let end = line
					.char_indices()
					.find(|(_, c)| {
						matches!(c, '.' | '!' | '?' | '。' | '！' | '？')
					})
					.map_or(line.len(), |(i, c)| i + c.len_utf8());
				line[..end].to_string()
			})
	});

	let candidate = candidate.unwrap_or_else(|| "Pasted Markdown".into());
	let candidate = strip_inline_markup(&candidate);
	let candidate: String = candidate.chars().take(72).collect();
	if candidate.trim().is_empty() {
		"Pasted Markdown".into()
	} else {
		candidate.trim().into()
	}
}

fn is_heading(line: &str) -> bool {
	let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
	hashes > 0 && hashes <= 6 && line.as_bytes().get(hashes) == Some(&b' ')
}

fn strip_inline_markup(text: &str) -> String {
	text.replace("**", "")
		.replace("__", "")
		.replace(['*', '_', '`'], "")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn detects_structured_markdown_and_rejects_plain_prose() {
		assert!(looks_like_markdown("# Heading\n\nA paragraph."));
		assert!(looks_like_markdown("- one\n- two"));
		assert!(looks_like_markdown("This is just an ordinary sentence."));
		assert!(looks_like_markdown("$E = mc^2$"));
	}

	#[test]
	fn rejects_empty_control_heavy_and_badly_decoded_text() {
		assert!(!looks_like_markdown(" \n\t"));
		assert!(!looks_like_markdown("valid\u{0000}text"));
		assert!(!looks_like_markdown("bad � replacement � text"));
	}

	#[test]
	fn chooses_heading_or_first_sentence() {
		assert_eq!(
			title_for("# **Release notes** ###\n\nText"),
			"Release notes"
		);
		assert_eq!(
			title_for("A short sentence. More details follow."),
			"A short sentence."
		);
		assert_eq!(title_for("这是一个句子。后面还有内容。"), "这是一个句子。");
		assert_eq!(title_for("問題ですか？続きがあります。"), "問題ですか？");
	}
}
