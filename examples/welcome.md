# A quieter place to read

## 让文字与公式，自然地排在一起

Markview 是一个只读的原生 Markdown 阅读器。让编辑器负责写作，让阅读器专注于文字的呈现：合适的行宽、克制的颜色，以及不会打断阅读的实时更新。

好的排版不仅是在行尾换行。Knuth–Plass 会考虑整个段落，权衡词间距离、断字和相邻行的疏密。中文标点也应留在合适的位置：开括号「（《不能孤零零地落在行尾，句号、逗号和右括号则不应该挤到下一行的开头。

Typography is the art of making reading effortless. An optimized paragraph balances the spaces between words across several lines, instead of accepting the first line that happens to fit. Hyphenation gives extraordinarily long words another opportunity to find a comfortable place in a narrow column.

行内公式 $E=mc^2$ 与文字共享基线，而 $\frac{a+b}{\sqrt{x_1^2+x_2^2}}$ 会增加这一行所需的高度。**粗体**、*斜体*、~~删除线~~、`inline code` 与 [链接](https://example.com) 都保留各自的语义。

$$
\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}
$$

> [!NOTE]
> 在外部编辑器中修改这个文件，Markview 会自动刷新。阅读上文时保持位置，已经滚到底部时跟随新增内容。

## Structured reading

| Feature | Syntax | Reading behavior |
|:--|:--:|--:|
| Emphasis | **bold** and *italic* | Native text |
| Mathematics | $\sum_{i=1}^{n}i$ | Shared baseline |
| Links | [example](https://example.com) | Click to open |
| Task lists | GFM | Read only |
| HTML | `<strong>bold</strong>` | Markdown semantics |

- [x] Native window and GPU drawing
- [x] Paragraph optimization and English hyphenation
- [x] Inline and display mathematics
- [ ] Images, selection and search belong to a later release

3. An ordered list can start at three.
   - Nested lists preserve indentation.
   - Multiple paragraphs remain part of the same item.
4. The next item keeps its number.

```rust
fn read_document(path: &Path) -> Result<Document> {
    let source = std::fs::read_to_string(path)?;
    Ok(parse(&source))
}
```

## A few mathematical shapes

$$
\begin{pmatrix}a & b \\ c & d\end{pmatrix}
\begin{pmatrix}x \\ y\end{pmatrix}
=\begin{pmatrix}ax+by \\ cx+dy\end{pmatrix}
$$

$$
\underbrace{1+2+\cdots+n}_{n\text{ terms}} = \frac{n(n+1)}{2}
$$

脚注也是阅读的一部分。[^reading] 这里有一个裸链接：https://example.com，以及一个邮箱 reader@example.com。链接可以直接点击：悬停时右下角显示目标地址，点击后在系统浏览器打开。

[^reading]: 字体由系统提供。公式使用随应用附带的 KaTeX 字体，公式解析与排版由 Rust 实现的 RaTeX 完成。

---

## HTML that reads like Markdown

Simple HTML shares Markdown semantics: <strong>bold</strong>, <em>italic</em>, <del>struck</del>, <code>code</code> and <a href="https://example.com">links</a>. Attributes such as <b class="lead" style="color:red">class or style</b> are ignored, comments vanish<!-- never shown--> without leaving a gap, and unknown tags such as <span>stay as source</span>.

<h2>A heading can be an HTML block</h2>

<p>So can a <em>paragraph</em>, and the next line is a rule.</p>

<hr>

![A landscape that is not loaded](landscape.png)

```text
Wide content can be scrolled locally with Shift+wheel: 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789
```
