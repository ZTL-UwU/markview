#!/usr/bin/env python3
"""Generate reproducible, exactly 100 KiB fixtures for large-document timing.

Two variants share the same paragraph mix so the only variable is formula
density:
  - math-cjk-100k.md  CJK + English + inline/display math
  - text-cjk-100k.md  the same without math, for a shaping-only comparison
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
ROOT.mkdir(parents=True, exist_ok=True)

MATH_PARAGRAPH = (
    "公式与正文混排 $E=mc^2$、$x_1^2+x_2^2$、$\\frac{a+b}{c}$，"
    "以及向量 $\\vec{v}=\\nabla\\phi$ 和积分 $\\int_0^1 x^2\\,dx$。\n\n"
)
DISPLAY = "$$\\int_0^1 x^2\\,dx=\\frac{1}{3}$$\n\n"
PARAGRAPHS = [
    "中文阅读依赖合理的行宽与行距。标点应该遵守禁则，避免将右括号、逗号和句号放在行首。原生阅读器使用系统字体，也要处理 English words 与中文的混合排版。\n\n",
    "Typography balances the spaces between words throughout a paragraph. An extraordinarily complicated explanation becomes easier to follow when the reader can concentrate on its meaning. Hyphenation and optimal line breaking work together to keep the texture of the paragraph consistent.\n\n",
    "## A short section\n\nThis paragraph includes **strong emphasis**, *italic text*, `inline code`, and a [reference](https://example.com). The layout remains readable when the window changes width.\n\n",
]
MATH_UNITS = [MATH_PARAGRAPH, DISPLAY, *PARAGRAPHS]
TEXT_UNITS = PARAGRAPHS
TARGET = 100 * 1024


def exact(name: str, units: list[str]) -> None:
    text = "# Markview large benchmark\n\n"
    i = 0
    while len((text + units[i % len(units)]).encode()) <= TARGET:
        text += units[i % len(units)]
        i += 1
    remaining = TARGET - len(text.encode())
    # Padding is readable ASCII, one UTF-8 byte per character, so the exact
    # byte target is always reachable without splitting a code point.
    text += ("Small words make a readable final line. " * 4000)[:remaining]
    data = text.encode()
    assert len(data) == TARGET, len(data)
    (ROOT / name).write_bytes(data)
    print(
        f"{ROOT / name}: {len(data)} bytes, "
        f"{text.count('$') // 2} math spans, {i} units"
    )


exact("math-cjk-100k.md", MATH_UNITS)
exact("text-cjk-100k.md", TEXT_UNITS)
