# chapbook

chapbook（/ˈtʃæpbʊk/，小册子）—— 轻量高性能静态文件服务器：把一个目录变成一本可读的小册子。
单文件可执行，前端资源全部内嵌，无外部文件依赖。

## Features

- **Browser Views** — 树、列表、网格为三个同级视图，共用顶部图标工具区；hover/键盘焦点显示含义，隐藏项开关与六键双向排序适用于目录、树、搜索和目录预览
- **Recursive Search** — `/__/search?q=` 服务端递归文件名搜索（过滤后确定性 BFS 取前 500 个候选，再按当前规则排序，不建索引）
- **Keyboard Navigation** — `j/k/↑/↓` 移动选中行，`Enter`/`o` 打开，`p` 预览，`v` 按列表→网格→树→列表循环，`/` 聚焦搜索框，`Backspace` 上级目录；输入框和修饰键不触发视图切换
- **Preview Panel** — 侧边面板以 fragment 渲染 .md/.org/Office/代码/图片/音视频，以及内容为严格 UTF-8 的未知扩展名纯文本，并以 sandboxed iframe 显示 .html/.htm 浏览器原生页面；HTML 预览可在页面与高亮源码（带行号）之间切换，不离开列表即可查看。树内容区打开 HTML 时提供同一 页面/源码 工具栏
- **Markdown Rendering** — `.md` 文件经 comrak 纯 Rust 渲染为自适应亮/暗色的 HTML：自建 TOC 侧栏、标题锚点、syntect 语法高亮、YAML front matter 标题、KaTeX 数学公式；无外部 CDN 依赖
- **Org-mode Rendering** — `.org` 文件经 orgize 纯 Rust 渲染为自适应亮/暗色的 HTML：自建 TOC 侧栏、标题锚点、4 级标题完整保留、代码块语法高亮、KaTeX 数学公式；无子进程、无外部依赖
- **Office / CSV Rendering** — `.doc/.docx/.ppt/.pptx/.xls/.xlsx/.odt/.ods/.odp/.rtf/.epub/.csv` 经 anydoc 纯 Rust 转为 GFM Markdown 后复用 comrak 渲染文档页（TOC/锚点/高亮与 `.md` 一致）；转换失败自动回退原文件下载；PDF 除外——浏览器原生打开
- **Code Highlighting** — 源代码文件（含 `.txt`/`.log`/`.env`）按 Accept 协商返回 syntect 高亮 HTML（带行号）或原文；`?raw=1` / `?view=1` 显式覆盖。覆盖 60+ 语言（内置 + 内嵌补充 TypeScript/TOML/Kotlin/Swift/Dockerfile/GraphQL 等）
- **Range Requests & Media** — 支持断点续传（tower-http `ServeFile` 内置）；音视频浏览器内播放与 seek
- **Open Locally & Download** — `?download=1` 显式下载（RFC 8187 编码文件名）；本机访问时可用系统默认应用打开文件（xdg-open / open，仅 loopback + token 校验通过时）
- **Security** — 逐分量路径解析，路径穿越返回 403；原始内容用 `sandbox allow-scripts` CSP 隔离（非下载的成功 PDF/音视频响应例外，强制 nosniff）；HTML 预览的 iframe 同样只允许 scripts，不含 `allow-same-origin`；所有 CB 完整页带 `frame-ancestors 'none'; script-src 'self'` 与 `X-Frame-Options: DENY`；Org 危险链接拒绝；悬空符号链接不会拖垮目录列表
- **单文件分发** — 前端资源与 syntect 语法集全部内嵌进二进制，无外部文件依赖

## Build

### Prerequisites

- Rust 1.88 或更新版本（edition 2024，建议通过 [rustup](https://rustup.rs) 安装）
- 无任何运行时外部依赖（pandoc/emacs 已随 Phase 2 完全退出）

### 构建可执行文件

```bash
# 发布构建（LTO + strip）
cargo build --release

# 产物路径
./target/release/chapbook
```

开发期可用 `cargo build`（快，未优化），产物在 `target/debug/chapbook`。

### Install

```bash
cp target/release/chapbook /usr/local/bin/
```

预编译二进制与中文发布说明见 [GitHub Releases](https://github.com/weiwen99/chapbook/releases)。
v0.4.3 支持 Linux x86_64（musl）和 macOS Apple Silicon，版本说明见
[v0.4.3 发布 notes](docs/release-notes-v0.4.3.md)。更新后需重启原有 chapbook 进程。

## Run

```bash
chapbook /path/to/serve
# 指定监听地址与端口
chapbook --host 127.0.0.1 --port 9000 /path/to/serve
```

```
Usage: chapbook [OPTIONS] <root-directory>

Arguments:
  <root-directory>  Directory to serve (must exist)

Options:
  -h, --host <HOST>  Bind address [default: 127.0.0.1]
  -p, --port <PORT>  Listen port [default: 8888]
      --help         Print help
  -V, --version      Print version
```

默认仅监听本机回环；局域网分享需显式 `--host 0.0.0.0`。

注意：`-h` 是 `--host` 的短选项，查看帮助请用 `--help`。

日志级别通过 `RUST_LOG` 控制，如 `RUST_LOG=debug chapbook .`。

## Browsing

目录、搜索结果和树视图的顶部左侧是搜索，右侧集中放置三视图图标、隐藏项开关和排序菜单。默认列表、名称升序、不显示隐藏项。
隐藏项按条目自己的 basename 是否以 `.` 开头判断（符号链接按链接名）；过滤不限制文件访问，直链仍可打开。

- 排序键：名称、末尾扩展名类型、大小、修改时间、访问时间、创建时间；每键均可升/降序。
  目录始终在文件前；名称按原始字节、区分大小写、不做自然排序（`file10` 在 `file2` 前）。
  同值固定按名称、完整相对路径升序。类型忽略 ASCII 大小写，`a.tar.gz` 为 `gz`、`.env` 无扩展名；
  目录大小是自身元信息长度，不递归统计；创建时间不可用时回退修改时间，再回退 epoch。
- `?sort=Size:Desc&hidden=1` 可分享显示规则，`hidden` 仅接受 `0|1`；非法排序或隐藏值返回 404。
  后续目录/搜索导航延续规则，离开搜索时移除 `q`。URL 不携带浏览布局，`view=1` 仍表示文件强制渲染。
- 有 JS 时，三个视图共享同源保存偏好；冷访问显式 URL 优先，历史项、reload 和 BFCache 按自己的快照恢复，
  不覆盖其他 tab 的默认偏好。同位置控件变化不增加历史，实际目录/文件/搜索导航才新增。
  排序/隐藏只更新受影响的条目区，不重新转换已打开的文档。
- 树目录的手动折叠会在刷新与显示规则更新后保留，不被子目录的展开记录或当前文档定位覆盖。
  折叠不会清空子树展开记忆；重新展开父目录后恢复子树。实际文件导航和新进入树视图仍可展开目标路径。
- 本机打开仅在有合法 token 的完整目录/搜索列表、网格可用，树壳与独立阅读页不携带该能力。
- 无 JS 时，列表的隐藏、排序、搜索和导航使用真实服务端链接/表单；不读浏览器偏好、不使用 cookie。
  树与网格依赖 JS。

### Reading

Markdown、Org、代码、Office／CSV 的独立阅读页使用紧凑的吸顶导航，不再显示浏览视图切换组和禁用的隐藏／排序控件。
顶栏显示路径面包屑与实际文件名，正文标题留在正文；PDF 仍交给浏览器原生查看器。

- **所在目录**：进入文件的父目录，沿用保存的浏览视图（无偏好时为列表）与当前排序／隐藏规则。
  只选中并滚动到可见文件，不自动打开预览，不改变偏好，也不为定位强行显示隐藏项。
- **文件树**：在树中继续阅读当前文件，新增一个历史项；后退恢复独立阅读页和原阅读位置。
- **搜索**：点击或按 `/` 展开文件名搜索，Escape 收起并返回搜索入口；提交进入既有搜索结果页。
  本文查找仍使用浏览器的 `⌘F / Ctrl+F`。
- **下载／复制链接**：下载原始文件，复制当前 URL（保留已有章节锚点）；剪贴板不可用时提供完整链接供手动复制。
- 宽屏以单行呈现，长路径可展开；窄屏保留“所在目录＋文件名＋更多”，完整路径和所有次级操作收在“更多”中，
  无需悬停。无 JS 时，所在目录、路径链接、搜索表单与下载仍可用，文件树与复制按钮不显示。

## Development

```bash
cargo run -- /tmp              # 直接运行
cargo test                     # 全部测试（无外部依赖，恒跑）
cargo fmt --check              # 格式检查
cargo clippy --all-targets -- -D warnings  # 静态检查（零警告）
```

浏览器行为验收使用现有 Chromium 与 Node，自建隔离服务、临时目录和 profile：

```bash
cargo build --release
CHROME_BIN=/path/to/chrome node tests/browser_mode_toggle_toolbar_acceptance.mjs
CHROME_BIN=/path/to/chrome node tests/browser_tree_async_acceptance.mjs
CHROME_BIN=/path/to/chrome node tests/browser_pdf_native_open_acceptance.mjs
```

默认使用 `target/release/chapbook`；可用 `CB_BIN` 指定其他构建产物。

行为契约、安全不变量与模块说明见 [AGENTS.md](AGENTS.md)；
设计提案见 [docs/](docs)。

## License

[Apache License 2.0](LICENSE)。内嵌第三方资源（Materialize 等）的许可证与归因见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
