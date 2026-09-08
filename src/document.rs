//! Semantic Markdown; no HTML rendering or remote resource loading.
use comrak::{
	Arena, Options,
	nodes::{AstNode, ListType, NodeValue, TableAlignment},
	parse_document,
};
use std::{
	collections::{HashMap, hash_map::DefaultHasher},
	hash::{Hash, Hasher},
	ops::Range,
	sync::Arc,
};

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct TextStyle {
	pub bold: bool,
	pub italic: bool,
	pub strike: bool,
	pub code: bool,
	pub superscript: bool,
	pub link: Option<String>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum InlineKind {
	Text(String),
	Math { latex: String, display: bool },
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Inline {
	pub kind: InlineKind,
	pub style: TextStyle,
	pub source: Range<usize>,
}

pub type RichText = Vec<Inline>;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CellAlign {
	Left,
	Center,
	Right,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ListItem {
	pub checked: Option<bool>,
	pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BlockKind {
	Paragraph(RichText),
	Heading {
		level: u8,
		text: RichText,
	},
	Code {
		language: String,
		text: String,
	},
	Quote {
		label: Option<String>,
		blocks: Vec<Block>,
	},
	List {
		start: Option<usize>,
		tight: bool,
		items: Vec<ListItem>,
	},
	Table {
		align: Vec<CellAlign>,
		rows: Vec<Vec<RichText>>,
	},
	Footnote {
		label: String,
		blocks: Vec<Block>,
	},
	Rule,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Block {
	pub id: u64,
	/// Semantic cache identity includes resolved references, excludes positions.
	pub content_key: u64,
	pub source: Range<usize>,
	pub kind: BlockKind,
}

#[derive(Clone, Debug)]
pub struct Document {
	pub source: Arc<str>,
	pub blocks: Vec<Block>,
}

pub fn fingerprint(value: &impl Hash) -> u64 {
	let mut h = DefaultHasher::new();
	value.hash(&mut h);
	h.finish()
}

struct Reader<'s> {
	source: &'s str,
	lines: Vec<usize>,
	footnotes: HashMap<String, u32>,
}

impl Reader<'_> {
	fn range(&self, node: &AstNode<'_>) -> Range<usize> {
		let p = node.data.borrow().sourcepos;
		let start = self
			.lines
			.get(p.start.line.saturating_sub(1))
			.copied()
			.unwrap_or(0)
			+ p.start.column.saturating_sub(1);
		let end = self
			.lines
			.get(p.end.line.saturating_sub(1))
			.copied()
			.unwrap_or(0)
			+ p.end.column;
		// Comrak columns are byte offsets unless sourcepos_chars is enabled.
		let mut start = start.min(self.source.len());
		let mut end = end.min(self.source.len()).max(start);
		while !self.source.is_char_boundary(start) {
			start -= 1;
		}
		while !self.source.is_char_boundary(end) {
			end += 1;
		}
		start..end
	}

	fn inlines<'a>(
		&self,
		node: &'a AstNode<'a>,
		style: &TextStyle,
		out: &mut RichText,
	) {
		for child in node.children() {
			let mut style = style.clone();
			let value = child.data.borrow();
			let kind = match &value.value {
				NodeValue::Text(t) => Some(InlineKind::Text(t.to_string())),
				NodeValue::SoftBreak => Some(InlineKind::Text(" ".into())),
				NodeValue::LineBreak => Some(InlineKind::Text("\n".into())),
				NodeValue::Code(c) => {
					style.code = true;
					Some(InlineKind::Text(c.literal.clone()))
				}
				NodeValue::HtmlInline(t) | NodeValue::Raw(t) => {
					style.code = true;
					Some(InlineKind::Text(t.clone()))
				}
				NodeValue::Math(m) => Some(InlineKind::Math {
					latex: m.literal.clone(),
					display: m.display_math,
				}),
				NodeValue::FootnoteReference(f) => {
					style.superscript = true;
					Some(InlineKind::Text(format!("[{}]", f.ix)))
				}
				NodeValue::Strong => {
					style.bold = true;
					None
				}
				NodeValue::Emph => {
					style.italic = true;
					None
				}
				NodeValue::Strikethrough => {
					style.strike = true;
					None
				}
				NodeValue::Link(l) => {
					style.link = Some(l.url.clone());
					None
				}
				NodeValue::Image(_) => {
					let mut alt = Vec::new();
					self.inlines(child, &TextStyle::default(), &mut alt);
					let alt = plain_text(&alt);
					style.italic = true;
					Some(InlineKind::Text(format!(
						"[Image: {}]",
						if alt.is_empty() {
							"no description"
						} else {
							&alt
						}
					)))
				}
				_ => None,
			};
			if let Some(kind) = kind {
				out.push(Inline {
					kind,
					style,
					source: self.range(child),
				});
			} else {
				self.inlines(child, &style, out);
			}
		}
	}

	fn rich<'a>(&self, node: &'a AstNode<'a>) -> RichText {
		let mut text = Vec::new();
		self.inlines(node, &TextStyle::default(), &mut text);
		text
	}

	fn blocks<'a>(&self, node: &'a AstNode<'a>, depth: usize) -> Vec<Block> {
		let mut blocks = Vec::new();
		for child in node.children() {
			let source = self.range(child);
			let data = child.data.borrow();
			let kind = if depth >= 64 {
				BlockKind::Code {
					language: "nested Markdown".into(),
					text: self.source[source.clone()].to_string(),
				}
			} else {
				match &data.value {
					NodeValue::Paragraph => {
						BlockKind::Paragraph(self.rich(child))
					}
					NodeValue::Heading(h) => BlockKind::Heading {
						level: h.level,
						text: self.rich(child),
					},
					NodeValue::CodeBlock(c) if c.info.trim() == "math" => {
						BlockKind::Paragraph(vec![Inline {
							kind: InlineKind::Math {
								latex: c.literal.clone(),
								display: true,
							},
							style: TextStyle::default(),
							source: source.clone(),
						}])
					}
					NodeValue::CodeBlock(c) => BlockKind::Code {
						language: c.info.clone(),
						text: c.literal.clone(),
					},
					NodeValue::HtmlBlock(h) => BlockKind::Code {
						language: "HTML source".into(),
						text: h.literal.clone(),
					},
					NodeValue::ThematicBreak => BlockKind::Rule,
					NodeValue::BlockQuote => BlockKind::Quote {
						label: None,
						blocks: self.blocks(child, depth + 1),
					},
					NodeValue::Alert(a) => BlockKind::Quote {
						label: Some(format!("{:?}", a.alert_type)),
						blocks: self.blocks(child, depth + 1),
					},
					NodeValue::List(l) => BlockKind::List {
						start: (l.list_type == ListType::Ordered)
							.then_some(l.start),
						tight: l.tight,
						items: child
							.children()
							.map(|item| {
								let checked = match &item.data.borrow().value {
									NodeValue::TaskItem(t) => {
										Some(t.symbol.is_some())
									}
									_ => None,
								};
								ListItem {
									checked,
									blocks: self.blocks(item, depth + 1),
								}
							})
							.collect(),
					},
					NodeValue::Table(t) => BlockKind::Table {
						align: t
							.alignments
							.iter()
							.map(|a| match a {
								TableAlignment::Center => CellAlign::Center,
								TableAlignment::Right => CellAlign::Right,
								_ => CellAlign::Left,
							})
							.collect(),
						rows: child
							.children()
							.map(|r| {
								r.children().map(|c| self.rich(c)).collect()
							})
							.collect(),
					},
					NodeValue::FootnoteDefinition(f) => BlockKind::Footnote {
						label: self
							.footnotes
							.get(&f.name)
							.map_or_else(|| f.name.clone(), u32::to_string),
						blocks: self.blocks(child, depth + 1),
					},
					_ => {
						blocks.extend(self.blocks(child, depth + 1));
						continue;
					}
				}
			};
			// Content identity deliberately excludes source offsets, which shift on append/insert.
			let id = fingerprint(&(
				std::mem::discriminant(&kind),
				&self.source[source.clone()],
			));
			let content_key = semantic_key(&kind);
			blocks.push(Block {
				id,
				content_key,
				source,
				kind,
			});
		}
		blocks
	}
}

pub fn parse(source: impl Into<Arc<str>>) -> Document {
	let source = source.into();
	let mut options = Options::default();
	options.extension.table = true;
	options.extension.strikethrough = true;
	options.extension.tasklist = true;
	options.extension.autolink = true;
	options.extension.footnotes = true;
	options.extension.alerts = true;
	options.extension.math_dollars = true;
	options.extension.math_code = true;
	let arena = Arena::new();
	let root = parse_document(&arena, &source, &options);
	let mut lines = vec![0];
	lines.extend(source.match_indices('\n').map(|(i, _)| i + 1));
	let reader = Reader {
		source: &source,
		lines,
		footnotes: root
			.descendants()
			.filter_map(|n| match &n.data.borrow().value {
				NodeValue::FootnoteReference(f) => Some((f.name.clone(), f.ix)),
				_ => None,
			})
			.collect(),
	};
	let blocks = reader.blocks(root, 0);
	Document { source, blocks }
}

pub fn plain_text(text: &RichText) -> String {
	text.iter()
		.map(|s| match &s.kind {
			InlineKind::Text(t) => t.as_str(),
			InlineKind::Math { latex, .. } => latex.as_str(),
		})
		.collect()
}

fn semantic_key(kind: &BlockKind) -> u64 {
	let mut hash = DefaultHasher::new();
	std::mem::discriminant(kind).hash(&mut hash);
	let rich = |t: &RichText| {
		fingerprint(&t.iter().map(|i| (&i.kind, &i.style)).collect::<Vec<_>>())
	};
	let children =
		|b: &[Block]| b.iter().map(|b| b.content_key).collect::<Vec<_>>();
	match kind {
		BlockKind::Paragraph(t) => rich(t).hash(&mut hash),
		BlockKind::Heading { level, text } => {
			(level, rich(text)).hash(&mut hash)
		}
		BlockKind::Code { language, text } => (language, text).hash(&mut hash),
		BlockKind::Quote { label: _, blocks }
		| BlockKind::Footnote { label: _, blocks } => {
			// The variants' labels have different types, so hash them separately.
			if let BlockKind::Quote { label, .. } = kind {
				label.hash(&mut hash);
			}
			if let BlockKind::Footnote { label, .. } = kind {
				label.hash(&mut hash);
			}
			children(blocks).hash(&mut hash);
		}
		BlockKind::List {
			start,
			tight,
			items,
		} => {
			(start, tight).hash(&mut hash);
			for item in items {
				(item.checked, children(&item.blocks)).hash(&mut hash);
			}
		}
		BlockKind::Table { align, rows } => {
			align.hash(&mut hash);
			for row in rows {
				for cell in row {
					rich(cell).hash(&mut hash);
				}
			}
		}
		BlockKind::Rule => {}
	}
	hash.finish()
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn gfm_and_raw_html() {
		let doc = parse(
			"# 中文\n\n- [x] done\n- [ ] todo\n\n| A | B |\n|:-|--:|\n| x | $x^2$ |\n\n~~gone~~ https://example.com <b>raw</b>\n",
		);
		assert_eq!(doc.blocks.len(), 4);
		let BlockKind::List { items, .. } = &doc.blocks[1].kind else {
			panic!()
		};
		assert_eq!(items[0].checked, Some(true));
		assert_eq!(items[1].checked, Some(false));
		let BlockKind::Table { align, .. } = &doc.blocks[2].kind else {
			panic!()
		};
		assert_eq!(align[1], CellAlign::Right);
		let BlockKind::Paragraph(p) = &doc.blocks[3].kind else {
			panic!()
		};
		assert!(p.iter().any(|s| s.style.strike));
		assert!(
			p.iter()
				.any(|s| s.style.link.as_deref() == Some("https://example.com"))
		);
		assert!(plain_text(p).contains("<b>raw</b>"));
	}
	#[test]
	fn identity_survives_insertion_and_ranges_are_utf8() {
		let a = parse("你好 **world**\n");
		let b = parse("New paragraph.\n\n你好 **world**\n");
		assert_eq!(a.blocks[0].id, b.blocks[1].id);
		assert_eq!(&b.source[b.blocks[1].source.clone()], "你好 **world**");
	}
	#[test]
	fn incomplete_fence_and_math_do_not_drop_text() {
		let d = parse("```rust\nlet x = 1;\n");
		assert!(
			matches!(&d.blocks[0].kind, BlockKind::Code { text, .. } if text.contains("let x"))
		);
		let d = parse("Cost \\$5, unfinished $x\n");
		assert!(
			matches!(&d.blocks[0].kind, BlockKind::Paragraph(p) if plain_text(p).contains("$x"))
		);
	}
	#[test]
	fn resolved_references_invalidate_semantics_and_footnotes_use_numbers() {
		let a = parse("A [link][id].\n\n[id]: https://one.example\n");
		let b = parse("A [link][id].\n\n[id]: https://two.example\n");
		assert_eq!(a.blocks[0].id, b.blocks[0].id);
		assert_ne!(a.blocks[0].content_key, b.blocks[0].content_key);
		let d = parse("See [^name].\n\n[^name]: The footnote.\n");
		assert!(
			matches!(&d.blocks[0].kind, BlockKind::Paragraph(p) if plain_text(p).contains("[1]"))
		);
		assert!(d.blocks.iter().any(
			|b| matches!(&b.kind, BlockKind::Footnote { label, .. } if label == "1")
		));
	}
}
