# Markview Stylesheet format v1

样式表是 UTF-8 TOML 文件，完整后缀为 `.mvss.toml`。它定义可迁移的主题；个人基础字号、正文宽度、两端对齐和英语断词保存在 `settings.toml`。

## 使用与安装

```sh
markview ss install paper.mvss.toml
markview ss install paper.mvss.toml --force
markview document.md --style paper --style light
markview --render document.md --output preview.png --style paper
```

安装前会校验完整文件；安装只复制 TOML，不安装字体、不自动启用。同名文件默认只接受 `version` 更大的样式表；版本相同或更低时拒绝安装。`--force` 忽略版本比较并允许原子替换。

用户样式目录为 `settings.toml` 同级的 `styles/`：

| 平台 | 目录 |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/styles/`，默认 `~/.config/markview/styles/` |
| macOS | `~/Library/Application Support/markview/styles/` |
| Windows | `%APPDATA%/markview/styles/` |

只扫描第一层 `.mvss.toml` 文件。文件名去掉完整后缀就是 ID，允许中文和空格，不允许路径。`light`、`dark` 是随附样式的保留 ID。设置面板中的 **Styles…**（或 `Ctrl+T`）打开列表，可以启用、停用、上下排序、分页及打开样式目录。文件错误与缺失状态会显示在列表中；缺失的已选样式仍可停用。

```toml
# settings.toml
version = 1
style = ["paper", "dark"]
font_size = 18.0
width = 760.0
justify = true
hyphenate = true
cjk-type = "none" # SC、TC、JP 或 none
```

`paper > dark > light 基础`：左侧优先，同一元素逐字段合并，所有数组整体替换。样式表可以只写少数覆盖项。

字体先通过 `[[fontdef]]` 定义。`lookfor` 按顺序检查系统字体文件，找到第一个后固定使用；所有候选都不存在时样式无法渲染。规则中的 `font` 使用定义的 `id`：

```toml
[[fontdef]]
id = "monospace"
lookfor = ["Consolas", "Fira Code"]

# 同一 id 可用 type = "SC"、"TC"、"JP" 分别定义 CJK 变种；省略 type 表示普通字体

[code]
font = [{ family = "monospace" }]
```

用户设置可以覆盖某个定义的候选字体；该设置不在 UI 中显示：

```toml
[[fontdef-override]]
id = "monospace"
override = "Monaspace Argon"
```

省略 `style` 表示跟随系统，在随附亮暗样式之间选择；`style = []` 固定使用亮色基础。手动列表不随系统切换。旧 `theme` 被读取为对应列表，下一次保存移除旧字段；同时存在时以 `style` 为准。

`--style ID` 可重复，先出现的优先，替换本次会话的列表而不持久化。它不能与 `--light`、`--dark` 同用。离屏模式不读取个人设置，默认亮色，支持显式样式参数。

保存样式或设置后自动生效，兼容编辑器的原子替换保存。样式列表任意一项无效时，整次样式更新不提交，保留上一份有效样式并提示；首次启动没有有效样式时使用亮色基础。修复后自动恢复。纯颜色变化复用布局；字体和几何变化重新排版并保持阅读位置。

## 示例

```toml
format_version = 1
version = 1

[meta]
name = "纸与墨"
description = "衬线英文与中文楷体强调"
author = "Example"

[body]
color = "#292524"
background = "#FAF8F2"
font = [
    { family = "serif" },
]
line_height = 1.65

[p]
space_after = 0.8

[h1]
size = 1.9
weight = 700
space_before = 1.2
space_after = 0.6

[em]
font = [
    { family = "serif", variant = "italic" },
]

[strong]
weight = 700

[strong_em]
font = [
    { family = "serif", variant = "italic", weight = 700 },
]

[link]
color = "#315D86"
decoration = ["underline"]

[link.hover]
color = "#163B5C"

[code_block]
background = "#EEEAE2"
padding = [0.6, 0.8, 0.6, 0.8]
border_color = "#D8D1C5"
border_width = 1.0
radius = 5.0

[selection]
background = "#315D8648"

[scrollbar]
track = "#00000000"
thumb = "#88807580"
thumb_hover = "#888075C0"
thickness = 8.0
thickness_hover = 14.0
overflow_thickness = 8.0
overflow_thickness_hover = 8.0
gutter = 8.0

[ui]
font = [{ family = "sans-serif" }]
color = "#292524"
background = "#FAF8F2"
muted = "#70695F"
accent = "#315D86"
border_color = "#D8D1C5"
error = "#A13F3F"
shadow = "#1019232E"
scrim = "#1019233D"

[ui.panel]
background = "#F2EEE6F0"
```

TOML 内联表使用 `=`，不是 JSON 的 `:`。`format_version = 1` 和非负整数 `version` 必填；`format_version` 是配置格式版本，`version` 是主题版本号；`meta` 可省略，不参与级联。未知元素、字段、类型、枚举值和不支持的字段组合均报错。

## 固定语义元素

| 类型 | 表名 |
| --- | --- |
| 文档块 | `body`、`p`、`h1`～`h6`、`blockquote`、`list`、`list_item`、`footnote` |
| 行内文字 | `em`、`strong`、`strong_em`、`link`、`code`、`del`、`sup` |
| 代码 | `code_block`、`code_block.label` |
| 表格 | `table`、`table.header`、`table.cell` |
| 标记与公式 | `list.marker`、`task_marker`、`hr`、`math` |
| 阅读器 | `selection`、`scrollbar` |
| 应用 UI | `ui`、`ui.toolbar`、`ui.statusbar`、`ui.panel`、`ui.button` |

这些是语义角色，不是 CSS 选择器。HTML 中支持的同义标签使用相同角色。

文字元素支持 `color`、`font`、`weight`、`size`、`decoration`、`background`，其中 `body` 不支持 `size`。文档文字块额外支持 `line_height`、`space_before`、`space_after`；文档块容器额外支持 `padding`、`border_color`、`border_width`、`radius`。代码语言标签不支持容器几何。行内元素不支持内边距、边框和圆角。

引用块的边框表示左侧引用线；表格边框表示网格，表头以普通单元格为基础覆盖。任务标记额外支持背景与边框颜色。

特殊元素的字段：

| 元素 | 字段 |
| --- | --- |
| `hr` | `color`、`border_width`、`space_before`、`space_after` |
| `math` | `color`、`size`；使用数学引擎专用字体，公式显式非黑色颜色保留 |
| `selection` | `background` |
| `scrollbar` | 颜色 `track`、`thumb`、`thumb_hover`；尺寸 `thickness`、`thickness_hover`、`overflow_thickness`、`overflow_thickness_hover`、`gutter` |
| `ui` 和其子表 | 基础文字和颜色字段，额外 `muted`、`accent`、`error`、`border_color` |
| `ui` | 额外 `shadow`、`scrim` |
| `ui.button` | 额外 `hover_background`、`active_background`、`disabled_color`、`focus_color` |

UI 不开放控件间距、尺寸或布局；文字缩放以控件的默认文字大小为基础。

## 单位与继承

| 字段 | 含义 |
| --- | --- |
| 颜色 | sRGB `#RRGGBB` 或 `#RRGGBBAA`；`body.background` 必须不透明 |
| `size` | 正有限数；块字号相对个人基础字号，行内字号相对所在块；嵌套块不重复乘缩放 |
| `line_height` | 正有限数，表示本元素字号倍数；行盒至少容纳真实字形与公式 |
| `space_before`、`space_after`、`padding` | 非负有限数，单位为个人基础字号倍数 |
| `padding` | 单个数，或四项 `[上, 右, 下, 左]` |
| `border_width`、`radius` | 非负有限数，单位为逻辑像素 |
| `scrollbar.thickness`、`scrollbar.thickness_hover`、`scrollbar.overflow_thickness`、`scrollbar.overflow_thickness_hover` | 正有限数，单位为逻辑像素；两个值相等即关闭悬停加粗 |
| `scrollbar.gutter` | 非负有限数，单位为逻辑像素；`0` 表示溢出块不额外预留空白，横向滚动条改画在内容下沿 |
| `weight` | 整数 `1..1000`，默认 `400` |
| `decoration` | `"underline"`、`"line-through"` 数组；`[]` 明确取消装饰 |

先按文件优先级合并每个角色，再计算文档内的文字继承。文字从所在容器继承，背景、边框、间距不继承。省略字段保留低优先级定义；没有 `inherit`、`unset` 或变量语法。

组合强调使用 `strong_em`。该表缺少的字段来自 `em`，元素字重来自 `strong`；候选字体显式字重仍优先。其他行内冲突按“所在块 → 强调角色 → link → del → sup → code”依次覆盖，和 Markdown 嵌套写法无关；数组整体替换。

## 字体回退

`font` 是非空有序数组。每项只有 `family`、`variant`、`weight`：

- `family` 必填，只引用系统已安装字体；也接受 `serif`、`sans-serif`、`monospace`。
- `variant` 为 `normal`、`italic`、`oblique`，省略明确表示正体 `normal`。
- 候选 `weight` 省略时采用元素有效字重。
- 缺字体、缺真实目标字形、缺字符覆盖时跳过，不人工倾斜或加粗；可变字体能提供目标字形时可用。
- 按完整字素簇选择候选，保留复杂文字塑形上下文。所有候选失败时使用系统正体常规字体作最终回退，仍无字形时显示缺字符号。

因此，示例的英文可以使用 Noto Serif Italic，而回退到落霞文楷的中文保持正体。样式本身可迁移，最终可用字体仍取决于目标机器。v1 不支持字体文件、下载、OpenType 特性、脚本、远程资源、任意选择器、`import` 或自动编辑样式。
