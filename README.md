# chapbook

chapbook（/ˈtʃæpbʊk/，小册子）—— 轻量高性能静态文件服务器：把一个目录变成一本可读的小册子。
单文件可执行，前端资源全部内嵌，无外部文件依赖。

## Features

- **Directory Listing** — Material Design 交互式界面（Materialize v2.3.3 社区维护分支），可按名称、大小、类型、时间戳排序
- **Markdown Rendering** — `.md` 文件经 comrak 纯 Rust 渲染为自适应亮/暗色的 HTML：自建 TOC 侧栏、标题锚点、syntect 语法高亮、YAML front matter 标题；无外部 CDN 依赖
- **Org-mode Rendering** — `.org` 文件经 orgize 纯 Rust 渲染为自适应亮/暗色的 HTML：自建 TOC 侧栏、标题锚点、4 级标题完整保留、代码块语法高亮；无子进程、无外部依赖，任何机器上文档永远可读
- **Office / CSV Rendering** — `.doc/.docx/.ppt/.pptx/.xls/.xlsx/.odt/.ods/.odp/.rtf/.epub/.csv` 经 anydoc 纯 Rust 转为 GFM Markdown 后复用 comrak 渲染文档页（TOC/锚点/高亮与 `.md` 一致）；转换失败自动回退原文件下载；PDF 除外——浏览器原生打开
- **Code Highlighting** — 源代码文件按 Accept 协商返回 syntect 高亮 HTML（带行号）或原文；`?raw=1` / `?view=1` 显式覆盖。覆盖 60+ 语言（内置 + 内嵌补充 TypeScript/TOML/Kotlin/Swift/Dockerfile/GraphQL 等）
- **Range Requests** — 支持断点续传/音视频拖动（tower-http `ServeFile` 内置）
- **Security** — 逐分量路径解析，路径穿越返回 403；悬空符号链接不会拖垮目录列表
- **单文件分发** — 前端资源与 syntect 语法集全部内嵌进二进制，无外部文件依赖，release 产物约 12 MB（anydoc 的 calamine/pdf-inspector 解析器占其中约一半）

## Build

### Prerequisites

- Rust 工具链（edition 2021，建议通过 [rustup](https://rustup.rs) 安装）
- 无任何运行时外部依赖（pandoc/emacs 已随 Phase 2 完全退出）

### 构建可执行文件

```bash
# 发布构建（LTO + strip，产物约 12 MB，构建约 5 分钟）
cargo build --release

# 产物路径
./target/release/chapbook
```

开发期可用 `cargo build`（快，未优化），产物在 `target/debug/chapbook`。

### Install

```bash
cp target/release/chapbook /usr/local/bin/
```

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
  -h, --host <HOST>  Bind address [default: 0.0.0.0]
  -p, --port <PORT>  Listen port [default: 8888]
      --help         Print help
  -V, --version      Print version
```

注意：`-h` 是 `--host` 的短选项，查看帮助请用 `--help`。

日志级别通过 `RUST_LOG` 控制，如 `RUST_LOG=debug chapbook .`。

## Development

```bash
cargo run -- /tmp              # 直接运行
cargo test                     # 全部测试（无外部依赖，恒跑）
cargo fmt --check              # 格式检查
cargo clippy --all-targets     # 静态检查（零警告基线）
```

行为契约、安全不变量与模块说明见 [AGENTS.md](AGENTS.md)；
设计提案见 [docs/](docs)。

## License

[Apache License 2.0](LICENSE)。内嵌第三方资源（Materialize 等）的许可证与归因见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
