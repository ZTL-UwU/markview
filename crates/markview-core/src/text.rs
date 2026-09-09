//! Logical reading text and its final layout geometry. No clipboard or input APIs.
use crate::layout::{LayoutSnapshot, Rect};
use std::{collections::HashMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

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
		cluster.range.start = self.boundaries[self
			.boundaries
			.partition_point(|&i| i <= cluster.range.start)
			.saturating_sub(1)];
		cluster.range.end = self.boundaries[self
			.boundaries
			.partition_point(|&i| i < cluster.range.end)
			.min(self.boundaries.len() - 1)];
		self.clusters.push(cluster);
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
						rects.push(rect);
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

#[cfg(test)]
mod tests {
	use super::*;
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
			"x^2 \\notacommand{x} internationalization representation"
		);
		assert!(node.clusters.iter().any(|c| c.range == (0..3)));
		for cluster in &node.clusters {
			assert!(cluster.range.end <= node.text.len());
		}
	}
}
