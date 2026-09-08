#!/usr/bin/env python3
"""Generate reproducible, exactly 10 KiB UTF-8 benchmark inputs."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
ROOT.mkdir(parents=True, exist_ok=True)
PARAGRAPHS = [
    "中文阅读依赖合理的行宽与行距。标点应该遵守禁则，避免将右括号、逗号和句号放在行首。原生阅读器使用系统字体，也要处理 English words 与中文的混合排版。\n\n",
    "Typography balances the spaces between words throughout a paragraph. An extraordinarily complicated explanation becomes easier to follow when the reader can concentrate on its meaning. Hyphenation and optimal line breaking work together to keep the texture of the paragraph consistent.\n\n",
    "## A short section\n\nThis paragraph includes **strong emphasis**, *italic text*, `inline code`, and a [reference](https://example.com). The layout remains readable when the window changes width.\n\n",
]
for name, math in [("ordinary-10k.md", False), ("math-10k.md", True)]:
    text = "# Markview benchmark\n\n"
    if math:
        text += "公式与正文混排 $E=mc^2$、$x_1^2+x_2^2$、$\\frac{a+b}{c}$。\n\n"
        text += "$$\\int_0^1 x^2\\,dx=\\frac{1}{3}$$\n\n"
    i = 0
    while len((text + PARAGRAPHS[i % len(PARAGRAPHS)]).encode()) <= 10240:
        text += PARAGRAPHS[i % len(PARAGRAPHS)]
        i += 1
    remaining = 10240 - len(text.encode())
    # Padding is readable ASCII, with regular spaces and no unbreakable word.
    padding = ("Small words make a readable final line. " * 300)[:remaining]
    text += padding
    data = text.encode()
    assert len(data) == 10240
    (ROOT / name).write_bytes(data)
