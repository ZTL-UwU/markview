//! Semantic Markdown; raw HTML is limited to a small supported subset.
use crate::html;
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
	pub color: Option<crate::style::Color>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum InlineKind {
	Text(String),
	Image(crate::image::ImageSpec),
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
	/// Semantic identity of the reading text; equal ids mean equal positions.
	pub content_id: u64,
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
		// Raw HTML tags are siblings, so a supported tag opens a style scope
		// that the matching closing tag ends; unsupported markup stays source.
		let mut style = style.clone();
		let mut scopes: Vec<(String, TextStyle)> = Vec::new();
		for child in node.children() {
			let mut child_style = style.clone();
			let value = child.data.borrow();
			let kind = match &value.value {
				NodeValue::Text(t) => Some(InlineKind::Text(t.to_string())),
				NodeValue::SoftBreak => Some(InlineKind::Text(" ".into())),
				NodeValue::LineBreak => Some(InlineKind::Text("\n".into())),
				NodeValue::Code(c) => {
					child_style.code = true;
					Some(InlineKind::Text(c.literal.clone()))
				}
				NodeValue::Raw(t) => {
					child_style.code = true;
					Some(InlineKind::Text(t.clone()))
				}
				NodeValue::HtmlInline(t) => match html::inline(t) {
					html::Inline::Image(image) => {
						Some(InlineKind::Image(image))
					}
					html::Inline::Ignore => continue,
					html::Inline::Break => Some(InlineKind::Text("\n".into())),
					html::Inline::Open { name, patch } => {
						scopes.push((name, style.clone()));
						apply_patch(&patch, &mut style);
						continue;
					}
					html::Inline::Close { name } => {
						if let Some(i) =
							scopes.iter().rposition(|(open, _)| *open == name)
						{
							style = scopes[i].1.clone();
							scopes.truncate(i);
						}
						continue;
					}
					html::Inline::Literal => {
						child_style.code = true;
						Some(InlineKind::Text(t.clone()))
					}
				},
				NodeValue::Math(m) => Some(InlineKind::Math {
					latex: m.literal.clone(),
					display: m.display_math,
				}),
				NodeValue::FootnoteReference(f) => {
					child_style.superscript = true;
					Some(InlineKind::Text(format!("[{}]", f.ix)))
				}
				NodeValue::Strong => {
					child_style.bold = true;
					None
				}
				NodeValue::Emph => {
					child_style.italic = true;
					None
				}
				NodeValue::Strikethrough => {
					child_style.strike = true;
					None
				}
				NodeValue::Link(l) => {
					child_style.link = Some(l.url.clone());
					None
				}
				NodeValue::Image(link) => {
					let mut alt = Vec::new();
					self.inlines(child, &TextStyle::default(), &mut alt);
					let alt = plain_text(&alt);
					Some(InlineKind::Image(crate::image::ImageSpec {
						src: link.url.clone(),
						alt,
						title: link.title.clone(),
						width: None,
						height: None,
					}))
				}
				_ => None,
			};
			if let Some(kind) = kind {
				out.push(Inline {
					kind,
					style: child_style,
					source: self.range(child),
				});
			} else {
				self.inlines(child, &child_style, out);
			}
		}
	}

	fn rich<'a>(&self, node: &'a AstNode<'a>) -> RichText {
		let mut text = Vec::new();
		self.inlines(node, &TextStyle::default(), &mut text);
		merge_text(text)
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
					NodeValue::HtmlBlock(h) => match html::block(&h.literal) {
						html::Block::Unsupported => BlockKind::Code {
							language: "HTML source".into(),
							text: h.literal.clone(),
						},
						html::Block::Empty => continue,
						html::Block::Rule => BlockKind::Rule,
						html::Block::Heading { level, text } => {
							BlockKind::Heading {
								level,
								text: html_rich(text, &source),
							}
						}
						html::Block::Paragraph(text) => {
							BlockKind::Paragraph(html_rich(text, &source))
						}
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
	let mut hasher = DefaultHasher::new();
	for block in &blocks {
		block.content_key.hash(&mut hasher);
	}
	Document {
		source,
		blocks,
		content_id: hasher.finish(),
	}
}

pub fn plain_text(text: &RichText) -> String {
	text.iter()
		.map(|s| match &s.kind {
			InlineKind::Text(t) => t.as_str(),
			InlineKind::Image(image) => image.alt.as_str(),
			InlineKind::Math { latex, .. } => latex.as_str(),
		})
		.collect()
}

impl Block {
	pub fn images<'a>(&'a self, out: &mut Vec<&'a crate::image::ImageSpec>) {
		fn rich<'a>(
			text: &'a RichText,
			out: &mut Vec<&'a crate::image::ImageSpec>,
		) {
			for inline in text {
				if let InlineKind::Image(image) = &inline.kind {
					out.push(image);
				}
			}
		}
		match &self.kind {
			BlockKind::Paragraph(t) | BlockKind::Heading { text: t, .. } => {
				rich(t, out)
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => {
				for b in blocks {
					b.images(out);
				}
			}
			BlockKind::List { items, .. } => {
				for item in items {
					for b in &item.blocks {
						b.images(out);
					}
				}
			}
			BlockKind::Table { rows, .. } => {
				for row in rows {
					for cell in row {
						rich(cell, out);
					}
				}
			}
			_ => {}
		}
	}
}

/// Only schemes the operating system can safely hand to a browser or mail
/// client are ever opened; `file:`, `javascript:` and local paths are not.
pub fn openable_link(url: &str) -> bool {
	let Some((scheme, _)) = url.split_once(':') else {
		return false;
	};
	matches!(
		scheme.to_ascii_lowercase().as_str(),
		"http" | "https" | "mailto"
	)
}

fn apply_patch(patch: &html::Patch, style: &mut TextStyle) {
	match patch {
		html::Patch::Bold => style.bold = true,
		html::Patch::Italic => style.italic = true,
		html::Patch::Strike => style.strike = true,
		html::Patch::Code => style.code = true,
		html::Patch::Superscript => style.superscript = true,
		html::Patch::Link(url) => style.link = Some(url.clone()),
		html::Patch::None => {}
	}
}

fn html_rich(spans: Vec<html::Span>, source: &Range<usize>) -> RichText {
	spans
		.into_iter()
		.map(|span| {
			let mut style = TextStyle::default();
			for patch in &span.styles {
				apply_patch(patch, &mut style);
			}
			Inline {
				kind: span.image.map_or_else(
					|| InlineKind::Text(span.text),
					InlineKind::Image,
				),
				style,
				source: source.clone(),
			}
		})
		.collect()
}

/// Merge neighboring runs that share a style so a dropped comment or tag does
/// not leave a double space behind.
fn merge_text(text: RichText) -> RichText {
	let mut out: RichText = Vec::with_capacity(text.len());
	for span in text {
		let InlineKind::Text(t) = &span.kind else {
			out.push(span);
			continue;
		};
		let mut merged = false;
		if let Some(last) = out.last_mut()
			&& last.style == span.style
			&& let InlineKind::Text(prev) = &mut last.kind
		{
			if prev.ends_with(char::is_whitespace)
				&& t.starts_with(char::is_whitespace)
			{
				let len = prev.trim_end().len();
				prev.truncate(len);
				prev.push(' ');
				prev.push_str(t.trim_start());
			} else {
				prev.push_str(t);
			}
			last.source.end = span.source.end;
			merged = true;
		}
		if !merged {
			out.push(span);
		}
	}
	out
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
		assert!(p.iter().any(|s| s.style.bold
			&& matches!(&s.kind, InlineKind::Text(t) if t == "raw")));
	}
	#[test]
	fn html_comments_disappear_and_attributes_are_ignored() {
		let doc = parse(
			"A <!-- hidden --> B <b class=\"x\" style=\"y\">bold</b> <em>i</em> <del>d</del> <code>c</code> <sup>s</sup> <a href=\"/u\">l</a>.\n",
		);
		let BlockKind::Paragraph(p) = &doc.blocks[0].kind else {
			panic!()
		};
		assert_eq!(plain_text(p), "A B bold i d c s l.");
		let style = |text: &str| {
			p.iter()
				.find(|s| matches!(&s.kind, InlineKind::Text(t) if t == text))
				.unwrap_or_else(|| panic!("missing {text}"))
				.style
				.clone()
		};
		assert!(style("bold").bold);
		assert!(style("i").italic);
		assert!(style("d").strike);
		assert!(style("c").code);
		assert!(style("s").superscript);
		assert_eq!(style("l").link.as_deref(), Some("/u"));
	}
	#[test]
	fn html_blocks_become_rule_heading_and_paragraph() {
		let doc = parse(
			"<h2>Title <em>here</em></h2>\n\n<hr>\n\n<p>Body</p>\n\n<!-- gone -->\n",
		);
		assert_eq!(doc.blocks.len(), 3);
		let BlockKind::Heading { level, text } = &doc.blocks[0].kind else {
			panic!()
		};
		assert_eq!(*level, 2);
		assert_eq!(plain_text(text), "Title here");
		assert!(text.iter().any(|s| s.style.italic));
		assert!(matches!(doc.blocks[1].kind, BlockKind::Rule));
		assert!(
			matches!(&doc.blocks[2].kind, BlockKind::Paragraph(p) if plain_text(p) == "Body")
		);
	}
	#[test]
	fn unsupported_html_keeps_the_source() {
		let doc = parse("<div class=\"x\">\n\nspan <span>s</span>\n");
		assert!(matches!(
			&doc.blocks[0].kind,
			BlockKind::Code { language, text }
				if language == "HTML source" && text.contains("div")
		));
		let BlockKind::Paragraph(p) = &doc.blocks[1].kind else {
			panic!()
		};
		assert!(plain_text(p).contains("<span>s</span>"));
	}
	#[test]
	fn only_safe_link_schemes_are_openable() {
		assert!(openable_link("https://example.com/a?b=c#d"));
		assert!(openable_link("HTTP://example.com"));
		assert!(openable_link("mailto:reader@example.com"));
		assert!(!openable_link("javascript:alert(1)"));
		assert!(!openable_link("file:///etc/passwd"));
		assert!(!openable_link("ftp://example.com"));
		assert!(!openable_link("other.md"));
		assert!(!openable_link("//example.com"));
		assert!(!openable_link("#section"));
	}
	#[test]
	fn content_id_tracks_semantics_not_source_spelling() {
		assert_eq!(
			parse("Hello **world**\n").content_id,
			parse("Hello __world__\n").content_id
		);
		assert_ne!(
			parse("Hello\n").content_id,
			parse("Hello there\n").content_id
		);
		assert_ne!(parse("A\n\nB\n").content_id, parse("B\n\nA\n").content_id);
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
