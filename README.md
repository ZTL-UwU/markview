# Markview

只读、原生的 Markdown 阅读器。Rust + winit + wgpu，不使用浏览器、WebView、JavaScript 或外部 TeX 进程。

正文使用 Knuth–Plass 段落优化、中文分行禁则和英语断字；行内公式按真实基线参与行高计算。修改磁盘文件后自动刷新，可与外部编辑器分屏使用。

## 运行

需要 Rust 1.88+、系统字体，以及可用的 Vulkan / OpenGL / Metal / Direct3D 12 驱动。

```sh
cargo run --release -- examples/welcome.md
cargo run --release -- /path/to/document.md
```

不传路径会打开空窗口，也可以拖放文件或使用打开按钮。只读取 UTF-8 文件；支持 UTF-8 BOM，大小暂限 32 MiB。

Linux 构建依赖 Fontconfig。Debian/Ubuntu 可安装：

```sh
sudo apt-get install libfontconfig1-dev libxkbcommon-dev libwayland-dev fonts-noto-core fonts-noto-cjk
```

优先使用系统 Noto Serif / Noto Serif CJK SC，缺失时通过 Fontique 回退到系统字体。标题和控件使用系统无衬线字体，代码使用等宽字体。数学字体已经内嵌，运行时无需下载。

## 操作

| 操作 | 快捷键 |
| --- | --- |
| 打开文件 | Ctrl+O |
| 明暗主题 | Ctrl+T |
| 增减字号 | Ctrl+加号 / 减号，或 Ctrl+滚轮 |
| 调整栏宽 | Ctrl+[ / Ctrl+]；工具栏 W− / W+ |
| 两端 / 左对齐 | Ctrl+L |
| 英语断字开关 | Ctrl+H |
| 阅读滚动 | 滚轮、上下箭头、PageUp / PageDown、空格、Home / End |
| 打开链接 | 左键点击并释放；拖选不会打开链接，悬停显示目标地址 |
| 选择与复制 | 拖选、Shift+点击扩展；Ctrl+A 全选、Ctrl+C 复制、Escape 清除 |
| 阅读设置 | Settings 按钮或 Ctrl+,；设置自动保存，Reset defaults 恢复默认值 |
| 超宽代码、表格、公式 | 光标悬停其上，Shift+滚轮或水平触控板手势 |
| 工具栏键盘操作 | Tab / Shift+Tab、Enter；Escape 退出焦点 |

macOS 可用 Command 代替 Ctrl。默认字号 18、正文行距至少 1.65、最大栏宽 760 逻辑像素。公式较高时自动增加行高。颜色主题变化复用已有布局。

保存后合并短时间内的文件事件；持续写入最多等待 100 ms 就开始一次刷新。兼容原地写入、临时文件重命名替换、删除后重建。更新使用递增版本，过期结果不会覆盖新版本。上方插入内容时尝试保持阅读位置；原本在底部时跟随新增内容。读取失败或不完整 UTF-8 保留上一份可读画面，并在状态栏说明。

## 语法范围

| 内容 | MVP 行为 |
| --- | --- |
| CommonMark 标题、段落、引用、分隔线 | 原生排版；保留硬换行，软换行按普通空格处理 |
| 有序 / 无序列表、嵌套、松散列表 | 保留编号、缩进及段落结构 |
| 粗体、斜体、删除线、行内代码 | 字体与装饰样式 |
| 围栏 / 缩进代码块 | 等宽显示，保留空白；暂无语法高亮 |
| GFM 表格 | 列对齐、表头、单元格换行；超宽表格局部滚动 |
| GFM 任务列表 | 只读状态框 |
| 链接、引用链接、裸 URL、邮箱 | 解析并显示链接样式；悬停显示地址，点击在系统浏览器打开 http / https / mailto |
| 脚注、GitHub 提示块 | 编号脚注与提示引用块 |
| 行内、块级数学 | RaTeX 原生解析与绘制 |
| 原始 HTML | 注释忽略；与 Markdown 同义的简单标签按相同语义排版，属性忽略；其余标签显示源码 |
| 图片 | 显示替代文本，不读取本地图片或发起网络请求 |

数学分隔符采用 Comrak 的 `math_dollars`、`math_code` 规则：`$...$`、`$$...$$`、GitHub 数学代码片段和标记为 `math` 的围栏代码块。金额中的美元符号可写为 `\$`。本版不启用 `\(...\)` / `\[...\]` 扩展。

原始 HTML 只解释与 Markdown 同义的简单标签：行内 `<b>` `<strong>` `<i>` `<em>` `<del>` `<s>` `<code>` `<kbd>` `<sup>` `<a href>` `<br>`，以及独占一行的 `<h1>`–`<h6>`、`<p>`、`<hr>`。`<!-- 注释 -->` 直接丢弃，`class`、`style` 等属性不参与解析，其他标签仍按源码显示。

RaTeX 支持分式、根号、上下标、积分、矩阵等；不支持的语法、尚未写完的公式或超过 16 KiB 的单条公式显示 LaTeX 源码。行内公式内部不换行，超宽公式单独占行并可横向滚动。数学中的黑色随主题映射为正文色，其他显式颜色保留。

目前支持基本选择复制和持久化阅读设置。搜索、目录、图片、高亮、系统文件关联、完整辅助功能、打印导出及多文档工作区尚未实现。链接只把 http、https、mailto 交给系统处理；相对路径与站内锚点不跳转。优先验证中英文；其他复杂文字继承底层塑形能力，但混合方向段落尚未做全面质量验证。彩色 emoji 暂用单色轮廓显示。GFM 的 HTML 显示策略与网页渲染不同。

选择复制输出阅读文本：代码保留原始空白，表格用制表符分列、换行分行，公式按整体选择并输出 LaTeX。排版生成的断字号不会被复制。改字号、栏宽保留选择；文件语义内容变化后清除选择（仅元数据变化或读取失败则保留）。拖选到正文视口上下边缘可自动滚动。第一版不提供 Markdown 源码复制、双击选词和完整键盘选择导航。

设置优先级为默认值 → 用户配置 → 显式 CLI 参数；仅用户实际调整的字段写入配置。主题只有在用户主动切换后才写入配置，否则跟随系统；Reset defaults 会重新跟随系统。面板覆盖正文，不改变栏宽；面板区域接收控件输入，点击正文可继续阅读操作。配置位置与错误恢复说明见 [架构文档](docs/architecture.md)。`--render`、`--bench`、`--smoke-test` 不读取个人配置，确保结果可复现。

## 实现与边界

采用三个 crate 的 Cargo workspace：`markview` 管理桌面应用与平台服务，`markview-core` 管理文档、阅读文本和排版，`markview-render` 管理 GPU 渲染。根目录的运行命令不变。

`Comrak AST → Document → ReaderSnapshot（文档 + LayoutSnapshot）→ 可见区绘制 → wgpu`

模块职责、版本模型和选择接口见 [docs/architecture.md](docs/architecture.md)。

- `document` 保留语义结构和源码范围，缓存身份包括解析后的引用内容，避免引用定义变化时复用错误布局。`html` 只把与 Markdown 同义的简单标签映射到同一套语义，注释丢弃、属性忽略，未知标签保留源码。
- `layout` 使用 Parley/Fontique 塑形，ICU4X 提供合法断点，hypher 提供英语断字。`linebreak` 实现整段动态规划、伸缩胶、断字惩罚、相邻行松紧等级与强制换行。断行后重新塑形并校验宽度。正文默认两端对齐、末行左对齐；极松的行允许保留参差行尾，避免无限拉大空隙。排版同时按行记录链接片段矩形，供悬停与点击命中测试，跨行链接不会把中间的正文一并框住。
- 正常段落使用优化断行。每段最多评估 250,000 次候选连接，超预算或无可行方案时按合法断点贪心降级；诊断工具报告降级数。不可拆分对象保持原尺寸并局部滚动。
- `math` 直接使用 RaTeX DisplayList，缓存最多 256 个公式结果。正文与数学共用 GPU 字形缓存。
- `render` 使用 Swash 栅格化字形，tiny-skia 栅格化特殊数学路径，固定 4 MiB R8 图集，按实际 DPI 缓存。字形使用四档水平亚像素相位，基线对齐物理像素，位图逐像素显示，避免小数位置上的二次滤波导致文字模糊。大路径分块处理。图集满时清空并重建当前可见帧，不使用已失效的 UV；单个可见帧仍超过图集上限时报告错误。
- `watch` 监控父目录，30 ms 静默防抖、100 ms 最长等待；另有 500 ms 元数据轮询补偿。独立的 `worker` 只保存最新待处理请求，主线程只接收最新版本结果；字号与栏宽调整复用已解析文档，不重新读文件。布局缓存最多 256 个块 / 100,000 条绘制指令，仅保留当前文档的缓存。
- `app` 在选择、控件或指针下的链接变化时重绘：悬停显示目标地址并改为手型光标，点击通过 `open` 调用系统默认程序，只放行 http、https、mailto 三种 scheme。
- 静止阅读时事件循环等待事件，不持续绘制。字形资源、布局快照与临时缓冲仍可能随文档复杂度增长；100 MB 目标适用于声明的普通文档基准，不是任意输入的硬上限。

## 测试、截图与基准

```sh
cargo test --workspace --all-targets --locked
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo build --release --locked

# 设置面板与选择高亮的真实 GPU 回归截图
cargo test --locked settings_and_selection_frame -- --ignored

# 使用真实 wgpu 管线离屏绘制，输出 PNG
target/release/markview --render examples/welcome.md --output artifacts/light.png
target/release/markview --render examples/welcome.md --dark --scroll 780 --output artifacts/dark.png
target/release/markview --render examples/welcome.md --scale 2 --width 1600 --height 1200 --output artifacts/2x.png

# 原生窗口首帧冒烟测试；首次文档帧完成后自动退出
target/release/markview --smoke-test examples/welcome.md --output artifacts/window.png

# 实际桌面上的原子保存与连续写入测试（仅修改临时文件，随后关闭测试窗口）
python3 scripts/smoke_watch.py target/release/markview

# 固定的 10 KiB 文档，各测 100 次全文重排和缓存刷新
target/release/markview --bench tests/fixtures/ordinary-10k.md --output artifacts/ordinary-bench.json
target/release/markview --bench tests/fixtures/math-10k.md --output artifacts/math-bench.json
```

`--render` / `--bench` 的 width、height 指物理像素，`--scale` 指像素与逻辑单位之比。窗口模式使用系统 DPI，width、height 指逻辑窗口尺寸。`--greedy` 供同字体、同栏宽的断行对照；`--left`、`--no-hyphens`、`--font-size`、`--column` 可调整排版。

窗口启动时在终端打印实际 DPR、物理帧缓冲尺寸和逻辑窗口尺寸；跨屏缩放变化也会记录。无需手动把 DPR 固定为 2，系统的分数缩放比例同样会用于字形栅格化。

基准明确区分初始化、首次文档打开、缓存已热的全文重排和块缓存刷新。计时包括读取、解析、全文几何布局、首屏字形准备及 **GPU 完成**；离屏结果不包括窗口系统与合成器呈现等待。首次打开单独记录，不把重复打开 P95 当作冷缓存首开 P95。操作系统文件缓存未人为清空。

内存读取 Linux `VmRSS` / `VmHWM`；另列图集、顶点缓冲及离屏目标的资源容量，不用应用堆分配替代 RSS，也不把可追踪的 GPU 字节数视为完整驱动显存。

本次实测与环境说明见 [docs/performance.md](docs/performance.md)。固定语料可以通过 `python3 scripts/generate_fixtures.py` 重建。详细测试覆盖语法、源码范围、引用缓存、最优断行穷举对照、中文禁则、基线、滚动锚点、链接命中区域、原子保存及版本淘汰。

## 平台状态

Linux 已在 Wayland + Intel Vulkan 和 Mesa llvmpipe 离屏后端验证。链接打开依赖系统默认程序（Linux 的 xdg-open / gio open 等、macOS 的 open、Windows 的 start），未安装启动器时状态栏会报告错误。Windows x64 (`x86_64-pc-windows-gnu`) 与 macOS ARM64 (`aarch64-apple-darwin`) 的 `cargo check --locked --all-targets` 交叉检查已通过。CI 另配置了三个原生 runner 上的编译、测试及静态检查，但尚未运行远程 CI，也尚未在 Windows/macOS 实机验证；交叉检查不等同于链接可发布二进制或实机交付。

依赖固定在 `Cargo.lock`。数学字体和依赖许可见 [THIRD_PARTY.md](THIRD_PARTY.md)。
