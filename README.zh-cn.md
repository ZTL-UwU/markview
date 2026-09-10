# Markview

Markview 是一个原生、只读的 Markdown 阅读器，面向希望专注阅读、而不是打开浏览器页面的用户。它在桌面窗口中渲染 Markdown、数学公式、代码、表格、链接和图片，不依赖浏览器、WebView、JavaScript 或外部 TeX 进程。

Markview 采用多线程处理和 GPU 加速渲染，兼顾较低的内存占用与出色的速度，同时为你的文档带来出版级的排版质量。中文文档的排版与优化也被作为第一优先级支持。

## 开始使用

Markview 需要 Rust 1.88 或更新版本、系统字体，以及可用的 Vulkan、OpenGL、Metal 或 Direct3D 12 驱动。

```sh
cargo run --release -- examples/welcome.md
cargo run --release -- /path/to/document.md
```

不指定文件会打开空窗口。也可以把 Markdown 文件拖入窗口，或使用 **Open**。Markview 读取 UTF-8 Markdown（包括 UTF-8 BOM）并监视文件变化，适合与编辑器并排使用。

Debian/Ubuntu 通常需要：

```sh
sudo apt-get install libfontconfig1-dev libxkbcommon-dev libwayland-dev fonts-noto-core fonts-noto-cjk
```

## 阅读操作

- `Ctrl+O` 打开文件；`Ctrl+T` 选择样式；`Ctrl+,` 打开设置。
- `Ctrl++` / `Ctrl+-` 调整字号；`Ctrl+[` / `Ctrl+]` 调整阅读栏宽度。
- 使用滚轮、方向键、Page Up/Down、空格、Home、End 或滚动条滚动。
- 拖动选择文本，使用 `Ctrl+C` 复制；`Ctrl+A` 全选。
- 点击链接可在系统浏览器中打开 `http`、`https` 或 `mailto` 链接。
- 将光标移到宽代码块、表格或公式上，可横向滚动。

macOS 使用 Command 代替 Ctrl。默认阅读栏宽度为 760 逻辑像素，默认字号为 18 逻辑像素。

## 支持的内容

支持 CommonMark 标题、段落、引用、列表、强调、代码块，GFM 表格和任务列表，脚注、GitHub 风格提示块、链接、受支持的原始 HTML、行内和块级数学公式，以及本地或远程图片。图片支持 PNG、JPEG、GIF、WebP、BMP、ICO 和 SVG；动图只显示第一帧。

阅读器有意保持只读：不能编辑或保存 Markdown，不提供目录或搜索，不跟随相对链接和锚点，不支持打印或多文档工作区。完整边界见[文档地图](docs/README.md)。

## 自定义样式

使用内置亮色/暗色样式，或安装 `.mvss.toml` 样式表：

```sh
markview ss install paper.mvss.toml
markview document.md --style paper
```

格式和支持的语义角色见[样式表指南](docs/stylesheets.md)。

## 开发

项目是 Rust workspace。请从[开发指南](docs/development.md)开始；[架构说明](docs/architecture.md)解释了修改代码时应保持的边界。

Markview 使用 MIT 许可证；第三方声明见 [THIRD_PARTY.md](THIRD_PARTY.md)。
