//! Logical reading text and its final layout geometry. No clipboard or input APIs.
use crate::layout::{LayoutSnapshot, Rect};
use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed};
use std::{collections::HashMap, ops::Range, sync::OnceLock};
use unicode_segmentation::UnicodeSegmentation;

/// Word boundaries for pointer gestures and text counts. ICU supplies the
/// UAX #29 rules plus the Chinese and Japanese dictionaries, so 中文文字 breaks
/// into 中文 / 文字 instead of one segment per character. The segmenter is
/// immutable and cheap to copy, so one process-wide instance serves every thread.
fn word_segmenter() -> WordSegmenterBorrowed<'static> {
	static SEGMENTER: OnceLock<WordSegmenterBorrowed<'static>> =
		OnceLock::new();
	*SEGMENTER.get_or_init(|| WordSegmenter::new_auto(Default::default()))
}

/// Whether a segment reads as a word: it carries letters or digits. ICU's own
/// `is_word_like` reports false for a segment that ends in a combining mark
/// after a base letter, such as `Cafe\u{301}`, so classify by content instead.
fn is_word(segment: &str) -> bool {
	segment.chars().any(char::is_alphanumeric)
}

/// The word-like range a click lands on. A click on punctuation or an emoji
/// selects that cluster; a click on whitespace selects the nearest word, with
/// the word before the gap winning a tie.
fn word_range(text: &str, clicked: Range<usize>) -> Option<Range<usize>> {
	let mut start = 0;
	let mut containing = None;
	let mut containing_word = None;
	let mut preceding = None;
	let mut following = None;
	for end in word_segmenter().segment_str(text) {
		let range = start..end;
		let word = is_word(&text[range.clone()]);
		if range.start <= clicked.start && clicked.end <= range.end {
			if word {
				containing_word = Some(range);
				break;
			}
			containing = Some(range);
		} else if word {
			if range.end <= clicked.start {
				preceding = Some(range);
			} else if range.start >= clicked.end {
				following = Some(range);
				break;
			}
		}
		start = end;
	}
	if let Some(range) = containing_word {
		return Some(range);
	}
	if let Some(range) = &containing
		&& !text[range.clone()].contains(char::is_whitespace)
	{
		return containing;
	}
	match (preceding, following) {
		(Some(before), Some(after)) => {
			Some(if clicked.start - before.end <= after.start - clicked.end {
				before
			} else {
				after
			})
		}
		(Some(before), None) => Some(before),
		(None, Some(after)) => Some(after),
		(None, None) => containing,
	}
}

/// Counts the same reading text that is copied, including whitespace. Words use
/// the same dictionary segmentation as double-click, so Chinese and Japanese
/// count by word instead of by character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCounts {
	pub chars: usize,
	pub words: usize,
}
impl TextCounts {
	pub fn of(text: &str) -> Self {
		let mut words = 0;
		let mut start = 0;
		for end in word_segmenter().segment_str(text) {
			words += usize::from(is_word(&text[start..end]));
			start = end;
		}
		Self {
			chars: text.graphemes(true).count(),
			words,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
	Before,
	After,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPosition {
	pub revision: u64,
	pub block: usize,
	pub node: usize,
	pub offset: usize,
	pub affinity: Affinity,
}
impl TextPosition {
	fn key(self) -> (usize, usize, usize) {
		(self.block, self.node, self.offset)
	}
	/// Reading order of two positions, ignoring revision and affinity. Repeated
	/// blocks and nodes order by their occurrence.
	pub fn cmp_reading(self, other: Self) -> std::cmp::Ordering {
		self.key().cmp(&other.key())
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
	pub anchor: TextPosition,
	pub focus: TextPosition,
}
impl TextSelection {
	pub fn ordered(self) -> (TextPosition, TextPosition) {
		if self.anchor.key() <= self.focus.key() {
			(self.anchor, self.focus)
		} else {
			(self.focus, self.anchor)
		}
	}
	pub fn is_empty(self) -> bool {
		self.anchor.key() == self.focus.key()
	}
}
#[derive(Clone, Debug)]
pub struct TextNode {
	pub text: String,
	pub separator: &'static str,
	pub clusters: Vec<TextCluster>,
	boundaries: Vec<usize>,
}
impl TextNode {
	pub fn new(text: String, separator: &'static str) -> Self {
		let mut boundaries: Vec<_> =
			text.grapheme_indices(true).map(|(i, _)| i).collect();
		boundaries.push(text.len());
		Self {
			text,
			separator,
			clusters: Vec::new(),
			boundaries,
		}
	}
	pub fn push(&mut self, mut cluster: TextCluster) {
		// A shaping cluster may begin inside a grapheme; never expose that boundary.
		cluster.range.start = self.grapheme_floor(cluster.range.start);
		cluster.range.end = self.grapheme_ceil(cluster.range.end);
		self.clusters.push(cluster);
	}
	/// Greatest grapheme boundary at or before `offset`.
	fn grapheme_floor(&self, offset: usize) -> usize {
		self.boundaries[self
			.boundaries
			.partition_point(|&i| i <= offset)
			.saturating_sub(1)]
	}
	/// Least grapheme boundary at or after `offset`.
	fn grapheme_ceil(&self, offset: usize) -> usize {
		self.boundaries[self
			.boundaries
			.partition_point(|&i| i < offset)
			.min(self.boundaries.len() - 1)]
	}
}
#[derive(Clone, Debug)]
pub struct TextCluster {
	pub range: Range<usize>,
	pub rect: Rect,
	pub rtl: bool,
	/// Draw index binds geometry to the same overflow viewport as painted text.
	pub command: usize,
}

impl LayoutSnapshot {
	pub fn same_reading_text(&self, other: &Self) -> bool {
		self.blocks.len() == other.blocks.len()
			&& self.blocks.iter().zip(&other.blocks).all(|(a, b)| {
				std::sync::Arc::ptr_eq(&a.layout, &b.layout)
					|| (a.layout.text.len() == b.layout.text.len()
						&& a.layout.text.iter().zip(&b.layout.text).all(
							|(a, b)| {
								a.text == b.text && a.separator == b.separator
							},
						))
			})
	}

	/// Preserve a selection across presentation text changes only when the
	/// selected text itself survives. Stable node slots keep nested cells aligned.
	pub fn rebase_selection(
		&self,
		next: &Self,
		selection: TextSelection,
		old_revision: u64,
		revision: u64,
	) -> Option<TextSelection> {
		if selection.anchor.revision != old_revision
			|| selection.focus.revision != old_revision
		{
			return None;
		}
		let (a, b) = selection.ordered();
		for bi in a.block..=b.block {
			let old = self.blocks.get(bi)?;
			let new = next.blocks.get(bi)?;
			if old.id != new.id
				|| old.layout.text.len() != new.layout.text.len()
			{
				return None;
			}
			for (ni, (old, new)) in
				old.layout.text.iter().zip(&new.layout.text).enumerate()
			{
				if (bi, ni) < (a.block, a.node)
					|| (bi, ni) > (b.block, b.node)
					|| old.text == new.text
				{
					continue;
				}
				let (prefix, end, _) = changed_span(&old.text, &new.text);
				let start = if (bi, ni) == (a.block, a.node) {
					a.offset
				} else {
					0
				};
				let stop = if (bi, ni) == (b.block, b.node) {
					b.offset
				} else {
					old.text.len()
				};
				if start < end && stop > prefix
					|| (prefix == end && start < prefix && stop > prefix)
				{
					return None;
				}
			}
		}
		let position = |mut p: TextPosition| -> Option<TextPosition> {
			let old = &self.blocks.get(p.block)?.layout.text.get(p.node)?.text;
			let new = &next.blocks.get(p.block)?.layout.text.get(p.node)?.text;
			if old != new {
				let (prefix, end, new_end) = changed_span(old, new);
				if prefix == end && p.offset == prefix {
					if p.key() == a.key() {
						p.offset = new_end;
					}
				} else if p.offset >= end {
					p.offset = p.offset - end + new_end;
				} else if p.offset > prefix {
					return None;
				}
			}
			if !new.is_char_boundary(p.offset) {
				return None;
			}
			p.revision = revision;
			Some(p)
		};
		Some(TextSelection {
			anchor: position(selection.anchor)?,
			focus: position(selection.focus)?,
		})
	}
	/// Select the word nearest a text hit-test position. Uses dictionary
	/// segmentation, so double-click selects a Chinese or Japanese word rather
	/// than a single character.
	pub fn select_word_at(
		&self,
		position: TextPosition,
	) -> Option<TextSelection> {
		let node = self
			.blocks
			.get(position.block)?
			.layout
			.text
			.get(position.node)?;
		let text = node.text.as_str();
		let offset = position.offset.min(text.len());
		// A hit reports the boundary before or after the grapheme under the
		// pointer; recover that grapheme so both halves of a character agree.
		let clicked = match position.affinity {
			Affinity::Before => {
				let tail = text.get(offset..)?;
				let end = tail
					.graphemes(true)
					.next()
					.map_or(offset, |g| offset + g.len());
				offset..end
			}
			Affinity::After => {
				let head = text.get(..offset)?;
				let start = head
					.graphemes(true)
					.next_back()
					.map_or(offset, |g| offset - g.len());
				start..offset
			}
		};
		let range = word_range(text, clicked)?;
		let start = node.grapheme_floor(range.start);
		let end = node.grapheme_ceil(range.end);
		if start >= end {
			return None;
		}
		Some(TextSelection {
			anchor: TextPosition {
				offset: start,
				affinity: Affinity::Before,
				..position
			},
			focus: TextPosition {
				offset: end,
				affinity: Affinity::After,
				..position
			},
		})
	}

	/// Select all text belonging to the laid-out block under a text hit.
	pub fn select_block_at(
		&self,
		position: TextPosition,
	) -> Option<TextSelection> {
		let block = self.blocks.get(position.block)?;
		let first = block
			.layout
			.text
			.iter()
			.enumerate()
			.find(|(_, node)| !node.text.is_empty())?;
		let last = block
			.layout
			.text
			.iter()
			.enumerate()
			.rev()
			.find(|(_, node)| !node.text.is_empty())?;
		Some(TextSelection {
			anchor: TextPosition {
				block: position.block,
				node: first.0,
				offset: 0,
				affinity: Affinity::Before,
				..position
			},
			focus: TextPosition {
				block: position.block,
				node: last.0,
				offset: last.1.text.len(),
				affinity: Affinity::After,
				..position
			},
		})
	}

	pub fn select_all(&self, revision: u64) -> Option<TextSelection> {
		let positions: Vec<_> = self
			.blocks
			.iter()
			.enumerate()
			.flat_map(|(bi, b)| {
				b.layout
					.text
					.iter()
					.enumerate()
					.filter(|(_, n)| !n.text.is_empty())
					.map(move |(ni, n)| (bi, ni, n.text.len()))
			})
			.collect();
		let &(b, n, _) = positions.first()?;
		let &(eb, en, len) = positions.last()?;
		Some(TextSelection {
			anchor: TextPosition {
				revision,
				block: b,
				node: n,
				offset: 0,
				affinity: Affinity::Before,
			},
			focus: TextPosition {
				revision,
				block: eb,
				node: en,
				offset: len,
				affinity: Affinity::After,
			},
		})
	}
	pub fn hit_test_text(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
		revision: u64,
	) -> Option<TextPosition> {
		let mut best = None;
		let mut distance = f32::INFINITY;
		// Blocks are ordered by y: only the one above the cursor's block can
		// still be nearer, and once a block is farther than the best score the
		// blocks below it can only be farther.
		let start = self
			.blocks
			.partition_point(|b| b.y + b.layout.height < y)
			.saturating_sub(1);
		for (bi, block) in self.blocks.iter().enumerate().skip(start) {
			let dy = (block.y - y)
				.max(0.0)
				.max(y - block.y - block.layout.height);
			if dy * dy * 10000.0 > distance {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				for cluster in &node.clusters {
					let Some(rect) = self.text_rect(bi, cluster, horizontal)
					else {
						continue;
					};
					let dy = (rect.y - y).max(0.0).max(y - rect.y - rect.h);
					let dx = (rect.x - x).max(0.0).max(x - rect.x - rect.w);
					let score = dy * dy * 10000.0 + dx * dx;
					if score < distance {
						distance = score;
						let (offset, _) = block.layout.command_view(
							cluster.command,
							bi,
							horizontal,
						);
						let after = (x
							>= cluster.rect.x - offset + cluster.rect.w * 0.5)
							!= cluster.rtl;
						best = Some(TextPosition {
							revision,
							block: bi,
							node: ni,
							offset: if after {
								cluster.range.end
							} else {
								cluster.range.start
							},
							affinity: if after {
								Affinity::After
							} else {
								Affinity::Before
							},
						});
					}
				}
			}
		}
		best
	}
	/// Returns whether a point is inside the laid-out bounds of a text cluster.
	/// Unlike `hit_test_text`, this does not snap through line spacing to the
	/// nearest cluster.
	pub fn contains_text(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> bool {
		let start = self
			.blocks
			.partition_point(|block| block.y + block.layout.height < y)
			.saturating_sub(1);
		self.blocks
			.iter()
			.enumerate()
			.skip(start)
			.any(|(bi, block)| {
				if block.y > y {
					return false;
				}
				block.layout.text.iter().any(|node| {
					node.clusters.iter().any(|cluster| {
						self.text_rect(bi, cluster, horizontal)
							.is_some_and(|rect| rect.contains(x, y))
					})
				})
			})
	}
	fn text_rect(
		&self,
		bi: usize,
		cluster: &TextCluster,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<Rect> {
		let block = &self.blocks[bi];
		let mut rect = cluster.rect;
		let (offset, clip) =
			block.layout.command_view(cluster.command, bi, horizontal);
		rect.x -= offset;
		if let Some(clip) = clip {
			rect = rect.intersect(clip)?;
		}
		rect.y += block.y;
		Some(rect)
	}
	pub fn selection_rects(
		&self,
		selection: TextSelection,
		horizontal: &HashMap<(usize, usize), f32>,
		revision: u64,
	) -> Vec<Rect> {
		self.selection_rects_in(
			selection,
			horizontal,
			revision,
			f32::NEG_INFINITY..f32::INFINITY,
		)
	}
	/// Capacity of unique retained reading text and hit-test allocations; excludes allocator overhead.
	pub fn text_index_bytes(&self) -> usize {
		let mut seen = std::collections::HashSet::new();
		self.blocks
			.iter()
			.filter(|b| seen.insert(std::sync::Arc::as_ptr(&b.layout)))
			.map(|b| {
				b.layout.text.capacity() * std::mem::size_of::<TextNode>()
					+ b.layout
						.text
						.iter()
						.map(|n| {
							n.text.capacity()
								+ n.boundaries.capacity()
									* std::mem::size_of::<usize>()
								+ n.clusters.capacity()
									* std::mem::size_of::<TextCluster>()
						})
						.sum::<usize>()
			})
			.sum()
	}
	pub fn selection_rects_in(
		&self,
		selection: TextSelection,
		horizontal: &HashMap<(usize, usize), f32>,
		revision: u64,
		visible_y: Range<f32>,
	) -> Vec<Rect> {
		if selection.is_empty()
			|| selection.anchor.revision != revision
			|| selection.focus.revision != revision
		{
			return Vec::new();
		}
		let (a, b) = selection.ordered();
		let mut rects = Vec::new();
		let start = self
			.blocks
			.partition_point(|b| b.y + b.layout.height < visible_y.start);
		for (bi, block) in self.blocks.iter().enumerate().skip(start) {
			if block.y > visible_y.end {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				for cluster in &node.clusters {
					if (bi, ni, cluster.range.end) > a.key()
						&& (bi, ni, cluster.range.start) < b.key()
						&& let Some(rect) =
							self.text_rect(bi, cluster, horizontal)
					{
						if matches!(
							block.layout.draws.get(cluster.command),
							Some(crate::scene::Draw::Image { .. })
						) {
							rects.extend([
								Rect {
									x: rect.x - 2.,
									y: rect.y - 2.,
									w: rect.w + 4.,
									h: 2.,
								},
								Rect {
									x: rect.x - 2.,
									y: rect.y + rect.h,
									w: rect.w + 4.,
									h: 2.,
								},
								Rect {
									x: rect.x - 2.,
									y: rect.y,
									w: 2.,
									h: rect.h,
								},
								Rect {
									x: rect.x + rect.w,
									y: rect.y,
									w: 2.,
									h: rect.h,
								},
							]);
						} else {
							rects.push(rect);
						}
					}
				}
			}
		}
		rects
	}
	pub fn extract_text(
		&self,
		selection: TextSelection,
		revision: u64,
	) -> String {
		if selection.anchor.revision != revision
			|| selection.focus.revision != revision
		{
			return String::new();
		}
		let (a, b) = selection.ordered();
		let mut result = String::new();
		for (bi, block) in self.blocks.iter().enumerate() {
			for (ni, node) in block.layout.text.iter().enumerate() {
				if node.text.is_empty() {
					continue;
				}
				if (bi, ni) < (a.block, a.node) || (bi, ni) > (b.block, b.node)
				{
					continue;
				}
				let start = if (bi, ni) == (a.block, a.node) {
					a.offset
				} else {
					0
				};
				let end = if (bi, ni) == (b.block, b.node) {
					b.offset
				} else {
					node.text.len()
				};
				if let Some(part) = node.text.get(start..end) {
					if (bi, ni) != (a.block, a.node) {
						result.push_str(node.separator);
					}
					result.push_str(part);
				}
			}
		}
		result.replace('\u{ad}', "")
	}
}

/// The differing interval, on grapheme boundaries, after trimming a shared
/// prefix and suffix. Also maps elided placeholder text back to its full value.
pub(crate) fn changed_span(old: &str, new: &str) -> (usize, usize, usize) {
	let prefix = old
		.graphemes(true)
		.zip(new.graphemes(true))
		.take_while(|(a, b)| a == b)
		.map(|(a, _)| a.len())
		.sum::<usize>();
	let suffix = old[prefix..]
		.graphemes(true)
		.rev()
		.zip(new[prefix..].graphemes(true).rev())
		.take_while(|(a, b)| a == b)
		.map(|(a, _)| a.len())
		.sum::<usize>();
	(prefix, old.len() - suffix, new.len() - suffix)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn counts_use_graphemes_and_dictionary_words() {
		assert_eq!(TextCounts::of(""), TextCounts::default());
		assert_eq!(
			TextCounts::of("Hello world!"),
			TextCounts {
				chars: 12,
				words: 2
			}
		);
		assert_eq!(
			TextCounts::of("e\u{301} 👩‍💻 中文"),
			TextCounts { chars: 6, words: 2 }
		);
		assert_eq!(TextCounts::of(" \n\t"), TextCounts { chars: 3, words: 0 });
		// Dictionary segmentation counts 中文文字 as two words, not four.
		assert_eq!(
			TextCounts::of("中文文字"),
			TextCounts { chars: 4, words: 2 }
		);
	}
	use crate::{
		document,
		layout::{LayoutEngine, LayoutOptions},
	};
	fn layout(source: &str, width: f32) -> LayoutSnapshot {
		LayoutEngine::new().layout(
			&document::parse(source),
			&LayoutOptions {
				width,
				..Default::default()
			},
		)
	}
	#[test]
	fn copies_reading_text_code_tables_and_atomic_math() {
		let snapshot = layout(
			"# Title\n\nA **bold** [link](https://example.com) $x^2$.\n\n```rust\n\tlet x = 1;\n\n```\n\n| A | B |\n|---|---|\n| 中 | 文 |\n\n- one\n- [x] two",
			300.0,
		);
		let text = snapshot.extract_text(snapshot.select_all(9).unwrap(), 9);
		assert_eq!(
			text,
			"Title\n\nA bold link x^2.\n\n\tlet x = 1;\n\n\n\nA\tB\n中\t文\n• one\n[x] two"
		);
		assert!(
			snapshot
				.extract_text(snapshot.select_all(9).unwrap(), 10)
				.is_empty()
		);
	}
	#[test]
	fn double_and_triple_click_ranges_follow_reading_text() {
		let snapshot = layout("First word here.\n\nSecond paragraph.", 300.0);
		let word = snapshot
			.select_word_at(TextPosition {
				revision: 1,
				block: 0,
				node: 0,
				offset: 8,
				affinity: Affinity::Before,
			})
			.unwrap();
		assert_eq!(snapshot.extract_text(word, 1), "word");
		let block = snapshot
			.select_block_at(TextPosition {
				revision: 1,
				block: 1,
				node: 0,
				offset: 3,
				affinity: Affinity::Before,
			})
			.unwrap();
		assert_eq!(snapshot.extract_text(block, 1), "Second paragraph.");
	}
	/// Extracts the double-click selection at `offset` in the first block.
	fn word_at(source: &str, offset: usize, affinity: Affinity) -> String {
		let snapshot = layout(source, 400.0);
		let selection = snapshot
			.select_word_at(TextPosition {
				revision: 1,
				block: 0,
				node: 0,
				offset,
				affinity,
			})
			.expect("a word selection");
		snapshot.extract_text(selection, 1)
	}
	#[test]
	fn double_click_uses_dictionary_segmentation_for_cjk() {
		// 中文文字: dictionary words are 中文 and 文字, not four characters.
		assert_eq!(word_at("中文文字", 3, Affinity::Before), "中文");
		assert_eq!(word_at("中文文字", 6, Affinity::Before), "文字");
		assert_eq!(word_at("中文文字", 9, Affinity::After), "文字");
		// The half of the character under the pointer picks the boundary side.
		assert_eq!(word_at("中文文字", 6, Affinity::After), "中文");
		// Japanese mixes dictionary words with single-character particles.
		assert_eq!(word_at("国際化と日本語", 6, Affinity::Before), "化");
		assert_eq!(word_at("国際化と日本語", 12, Affinity::Before), "日本語");
	}
	#[test]
	fn double_click_prefers_adjacent_word_over_whitespace() {
		// 中文 测试: a hit inside the gap takes the word before it.
		assert_eq!(word_at("中文 测试", 6, Affinity::Before), "中文");
		assert_eq!(word_at("中文 测试", 7, Affinity::Before), "测试");
		assert_eq!(word_at("中文 测试", 7, Affinity::After), "中文");
		// Punctuation and emoji are their own selectable cluster.
		assert_eq!(word_at("Hello, world!", 5, Affinity::After), "Hello");
		assert_eq!(word_at("Hello, world!", 5, Affinity::Before), ",");
		assert_eq!(word_at("Hello 👩‍💻 world", 6, Affinity::Before), "👩‍💻");
	}
	#[test]
	fn double_click_never_splits_a_grapheme() {
		let snapshot = layout("Cafe\u{301} shop", 400.0);
		let selection = snapshot
			.select_word_at(TextPosition {
				revision: 1,
				block: 0,
				node: 0,
				offset: 4,
				affinity: Affinity::Before,
			})
			.unwrap();
		assert_eq!(snapshot.extract_text(selection, 1), "Cafe\u{301}");
	}
	#[test]
	fn pointer_hit_inside_a_cjk_word_selects_that_word() {
		let snapshot = layout("中文文字", 400.0);
		let block = &snapshot.blocks[0];
		let horizontal = HashMap::new();
		// Click the right half of the first glyph of 文字.
		let cluster = block.layout.text[0]
			.clusters
			.iter()
			.find(|c| c.range.start >= 6)
			.unwrap();
		let position = snapshot
			.hit_test_text(
				cluster.rect.x + cluster.rect.w * 0.75,
				block.y + cluster.rect.y + cluster.rect.h * 0.5,
				&horizontal,
				1,
			)
			.unwrap();
		let selection = snapshot.select_word_at(position).unwrap();
		assert_eq!(snapshot.extract_text(selection, 1), "文字");
		// The highlight covers exactly the two glyphs of the word.
		let word_clusters: Vec<_> = block.layout.text[0]
			.clusters
			.iter()
			.filter(|c| c.range.start >= 6 && c.range.end <= 12)
			.collect();
		let rects = snapshot.selection_rects(selection, &horizontal, 1);
		assert_eq!(rects.len(), word_clusters.len());
		for (rect, cluster) in rects.iter().zip(word_clusters) {
			assert!((rect.x - cluster.rect.x).abs() < 0.01);
			assert!((rect.w - cluster.rect.w).abs() < 0.01);
		}
	}
	#[test]
	fn selection_survives_reflow_and_repeated_blocks_are_distinct() {
		let doc = document::parse(
			"A repeated paragraph with internationalization and 中文文字.\n\nA repeated paragraph with internationalization and 中文文字.",
		);
		let mut engine = LayoutEngine::new();
		let wide = engine.layout(&doc, &LayoutOptions::default());
		let selection = TextSelection {
			anchor: TextPosition {
				revision: 1,
				block: 0,
				node: 0,
				offset: 2,
				affinity: Affinity::Before,
			},
			focus: TextPosition {
				revision: 1,
				block: 1,
				node: 0,
				offset: 10,
				affinity: Affinity::After,
			},
		};
		let narrow = engine.layout(
			&doc,
			&LayoutOptions {
				width: 120.0,
				font_size: 23.0,
				..Default::default()
			},
		);
		assert_eq!(
			wide.extract_text(selection, 1),
			narrow.extract_text(selection, 1)
		);
		assert_eq!(
			wide.extract_text(wide.select_all(1).unwrap(), 1),
			narrow.extract_text(narrow.select_all(1).unwrap(), 1)
		);
		assert!(
			!narrow
				.selection_rects(selection, &HashMap::new(), 1)
				.is_empty()
		);
	}
	#[test]
	fn hit_testing_respects_graphemes_tabs_and_overflow_clip() {
		let snapshot = layout(
			"Cafe\u{301} 👨‍👩‍👧 中文 office\n\n```\n\t012345678901234567890123456789012345678901234567890\n```",
			130.0,
		);
		let node = &snapshot.blocks[0].layout.text[0];
		let boundaries: Vec<_> = node
			.text
			.grapheme_indices(true)
			.map(|(i, _)| i)
			.chain([node.text.len()])
			.collect();
		for cluster in &node.clusters {
			assert!(boundaries.contains(&cluster.range.start));
			assert!(boundaries.contains(&cluster.range.end));
		}
		let tab = &snapshot.blocks[1].layout.text[0].clusters[0];
		assert_eq!(tab.range, 0..1);
		let mut horizontal = HashMap::new();
		horizontal.insert((1, 0), 80.0);
		let all = snapshot.select_all(1).unwrap();
		let overflow = &snapshot.blocks[1].layout.overflow[0];
		for rect in snapshot
			.selection_rects(all, &horizontal, 1)
			.iter()
			.filter(|r| r.y >= snapshot.blocks[1].y)
		{
			assert!(
				rect.x >= overflow.rect.x
					&& rect.x + rect.w
						<= overflow.rect.x + overflow.rect.w + 0.01
			);
		}
		let c = &node.clusters[0];
		let hit = snapshot
			.hit_test_text(
				c.rect.x + 0.1,
				c.rect.y + c.rect.h / 2.0,
				&horizontal,
				1,
			)
			.unwrap();
		assert_eq!(hit.offset, 0);
	}
	#[test]
	fn hit_testing_prunes_far_blocks_but_keeps_the_nearest_cluster() {
		let source: String = (0..400)
			.map(|i| format!("Paragraph {i} with some words.\n\n"))
			.collect();
		let snapshot = layout(&source, 400.0);
		assert!(snapshot.blocks.len() > 300);
		let none = HashMap::new();
		let first = &snapshot.blocks[0];
		let cluster = &first.layout.text[0].clusters[0];
		let hit = snapshot
			.hit_test_text(
				cluster.rect.x + 0.5,
				first.y + cluster.rect.y + cluster.rect.h * 0.5,
				&none,
				1,
			)
			.unwrap();
		assert_eq!(hit.block, 0);
		let index = snapshot.blocks.len() - 1;
		let last = &snapshot.blocks[index];
		let cluster = &last.layout.text[0].clusters[0];
		let hit = snapshot
			.hit_test_text(
				cluster.rect.x + 0.5,
				last.y + cluster.rect.y + cluster.rect.h * 0.5,
				&none,
				1,
			)
			.unwrap();
		assert_eq!(hit.block, index);
		assert_eq!(hit.offset, 0);
	}
	#[test]
	fn formula_mapping_does_not_depend_on_success_and_hyphens_are_visual_only()
	{
		let snapshot = layout(
			"$x^2$ $\\notacommand{x}$ internationalization representation",
			90.0,
		);
		let node = &snapshot.blocks[0].layout.text[0];
		assert_eq!(
			snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
			"x^2 \\notacommand{x} [Math error: ParseError at position 0: Undefined control sequence: \\notacommand] internationalization representation"
		);
		assert!(node.clusters.iter().any(|c| c.range == (0..3)));
		for cluster in &node.clusters {
			assert!(cluster.range.end <= node.text.len());
		}
	}
}
