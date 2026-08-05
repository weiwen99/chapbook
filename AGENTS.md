# AGENTS.md

本文件面向在本仓库工作的 AI 编码代理与贡献者。

## 项目简介

**chapbook** 是一个轻量静态文件服务器，二进制名同为 `chapbook`。功能：目录列表（Materialize UI、可排序）、
静态文件服务（Range 请求）、`.md` 经 comrak、`.org` 经 orgize、源代码文件经 syntect 渲染
（全部纯 Rust、零运行时外部依赖）。

## 常用命令

```bash
cargo build                 # 调试构建
cargo build --release       # 发布构建，产物 target/release/chapbook（~5MB，LTO+strip，syntect 内嵌语法集）
cargo test                  # 全部测试（无外部依赖，恒跑）
cargo fmt --check           # 格式检查（提交前必须过）
cargo clippy --all-targets  # 静态检查（零警告基线，提交前必须过）
./target/release/chapbook /path/to/serve   # 运行；--help 查看参数
```

无 Makefile / 脚本封装，全部走 cargo 标准命令。

## CLI

```
chapbook [-h|--host <HOST>] [-p|--port <PORT>] <root-directory>
```

- `-h` 是 `--host`（历史约定），帮助只走 `--help`。**不要**把 `-h` 改回 help。
- 曾有 `-c/--css`（Markdeep 主题白名单），已随 Markdeep 一并移除：`.md` 改由 pandoc 渲染，该选项无意义。
  传 `-c` 会直接报 unknown argument，这是有意为之。
- `<root-directory>` 启动时 `canonicalize`，不存在则报错退出。

## 目录结构

```
src/
  main.rs      # 入口：日志初始化、TCP bind、优雅退出（Ctrl-C）
  lib.rs       # 库入口，导出下列模块（集成测试依赖 lib target）
  opts.rs      # clap CLI 定义与校验
  sort.rs      # SortBy/SortColumn/SortOrder，解析 "Column:Order"
  meta.rs      # FileMeta：元信息、human_size、href percent-encoding、时间格式化
  listing.rs   # 目录遍历与排序（坏目录项逐个跳过）
  render.rs    # HTML 生成（maud）：目录页、doc_page 骨架、共享 slug/TOC、DOC_STYLE 注入
  highlight.rs # syntect 代码高亮（org src 块 / md 代码块 / 代码文件共用）+ 语言识别
  org.rs       # orgize 渲染 .org：预扫描元数据 + 自定义 HtmlHandler + src 块高亮
  markdown.rs  # comrak 渲染 .md：front matter、HeadingAdapter 锚点/TOC、代码高亮适配器
  routes.rs    # axum 路由与 handler
  assets.rs    # include_str! 内嵌前端资源
assets/        # materialize.min.css / materialize.min.js（v2.3.3 社区分支）+ chapbook-theme.css（目录页主题）
               # + chapbook-doc.css（文档/代码页样式）
               # + syntaxes/（14 个补充语法定义，syntect 默认集缺失的常见语言；见 THIRD_PARTY_NOTICES）
tests/
  router.rs    # 路由集成测试（渲染/协商/安全）
  sort.rs      # SortBy 解析测试
```

## 行为契约（改动必须保持，测试锁定）

1. `GET /__/status` 返回 200，body 精确为 `simple static server is running.\n`。
2. 静态资源挂载在 `/__/static/{css,js}/...`（目录列表页的 HTML 硬编码引用该前缀）。
3. `?sort=Column:Order` 排序语法；**非法值返回 404**（不是 400，历史行为保持兼容）。
4. 目录列表页结构：`Index of {dir}` 标题、striped 表格、表头在 `<thead>` 中（放裸 `tr` 会被浏览器
   包进隐式 tbody 而染上条纹）、表头排序箭头（▲/▼）、非根目录含 `../` 行（`colspan="6"`，
   单 td 行的条纹背景只会染到单元格区域）。
5. `.md` 经 comrak 0.54 纯 Rust 渲染（GFM 扩展 + footnotes/description lists + smart 排版）；
   `.org` 经 orgize 0.9 纯 Rust 渲染（`Org::parse` 内存解析，不会失败、无子进程、无超时兜底）。
   两者都自建 TOC（`<nav id="TOC">`，与标题锚点由**同一 slug 函数** `render::slugify` 生成；
   org 的 `#+OPTIONS: toc:nil` / `toc:N` 可关闭/限深度）、标题带 `<a id= href=>` 锚点、
   页面骨架由 `render::doc_page` 自组（`<!DOCTYPE>`/head/title/body），输出在 `</head>` 前注入
   `render::DOC_STYLE`（同一视觉系统，覆盖 `#TOC` 与 `.sourceCode`/`.src`/`.example` 代码块）。
   md 的 YAML front matter 剥离并取 `title:`（→ `<title>` 与 `header#title-block-header` 下
   `h1.title`）；org 的 `#+TITLE`/`#+AUTHOR`/`#+DATE` 同渲染。4 级标题完整保留（orgize 与源码一致，
   pandoc/emacs 会结构性丢失）。md 内 raw HTML 转义显示（安全：文档页不执行内嵌 HTML）；
   数学公式按原文显示（pandoc 的 KaTeX 本就是 CDN 链接，离线不渲染）。
   已废弃：Markdeep 客户端渲染与 pandoc/emacs 子进程管线（提案
   docs/2026-08-05-proposal-remove-dependency-of-pandoc.org，Phase 1/2 已实施）。
6. 文件服务必须支持 Range（tower-http `ServeFile` 提供）；缺失文件 404。
7. 源代码文件（`highlight.rs` 的扩展名映射表内）按 **Accept 协商**返回：`text/html` → syntect
   高亮 HTML（带行号 `<span class="ln">`，同一 DOC_STYLE）；否则（curl 等）→ 原文。
   `?raw=1` / `?view=1` 显式覆盖。非 UTF-8 与 >1MB 文件不渲染，裸字节透传。
   判定顺序：目录 → .md → .org → 代码 → ServeFile。

## 安全不变量

- **路径穿越**：`routes::resolve_within_root` 逐分量解析 URL 路径，`..` 弹出到 root 时返回 403。
  任何重构不得退化为"字符串前缀比较"——词法比较不消除 `..` 分量，`/%2e%2e/...` 可读到根目录外文件，
  `tests/router.rs::path_traversal_is_forbidden` 锁定该行为。
- 符号链接**保持跟随**语义（允许把内容软链进服务根目录）；目录项元信息读取失败时
  回退 `symlink_metadata`，再失败则跳过该项，不允许单个坏文件导致列表 500。
- href 编码使用 RFC 3986 percent-encoding（`meta::PATH_SEGMENT_ENCODE_SET`），空格为 `%20`；
  **不要**用表单语义编码（`+` 在 path segment 是字面加号，链接会坏），测试已锁定正确行为。

## 依赖与选型理由

| crate | 用途 | 备注 |
|---|---|---|
| axum 0.8 + tokio | HTTP 服务 | 路由含 `/`、`/{*path}` 两条通配（`/` 需显式注册） |
| tower-http (fs) | `ServeFile` | Range / Content-Type / Last-Modified 内置 |
| maud | HTML 模板 | 自动转义，文件名含 `<>&"` 安全；TOC/页面骨架渲染 |
| orgize 0.9 | .org 渲染 | 纯 Rust AST 渲染，无子进程；`default-features = false`（跳过 serde） |
| comrak 0.54 | .md 渲染 | `default-features = false`；HeadingAdapter/SyntaxHighlighterAdapter 插件化锚点与高亮 |
| syntect 5 | 代码高亮 | 内嵌语法集（默认 ~50 语言 + `assets/syntaxes/` 补充 14 个常见语言，二进制 +~0.5MB）；`ClassStyle::Spaced` 输出 scope atom 类名 |
| clap (derive) | CLI | `disable_help_flag`，`-h` 让给 `--host` |
| chrono | 时间格式化 | `yyyy-MM-dd HH:mm:ss` 本地时区 |
| percent-encoding | href 编码 | 见安全不变量 |
| tracing + tracing-subscriber | 日志 | `RUST_LOG` 环境变量控制级别 |

Materialize 前端资源来自 [materializecss/materialize](https://github.com/materializecss/materialize)
社区维护分支（v2.3.3，MIT），升级时替换 `assets/` 下两个 materialize 文件即可，无构建步骤。
视觉主题由 `assets/chapbook-theme.css` 覆盖（GitHub 色系，亮/暗自适应，与 org 渲染页同源）——v2.3.3 dist
的链接色是无效的 `colorFunc(...)` 声明，浏览器会丢弃并回退到 UA 默认蓝紫色，必须保留该覆盖。

## 设计决策

1. 路径穿越：词法前缀比较不消除 `..` 分量 → 逐分量解析，溢出 root 返回 403；符号链接保持跟随。
2. 文件名链接使用 percent-encoding（空格 `%20`），不用表单语义（`+`）。
3. 零运行时外部依赖（无子进程、无外部二进制）：.md/comrak、.org/orgize、代码/syntect 全部纯 Rust；
   文档/代码渲染全部服务端完成，无前端 JS 依赖。
4. `.md` 不走客户端 JS 渲染（Markdeep 依赖外部 CDN，离线即无法渲染，且与 org 页视觉不一致），
   改由 comrak 服务端渲染；无 `-c/--css` 选项。
5. 全局字号约为浏览器默认的 80%，org/md 正文栏宽 100rem（与字号等比，保持每行字数）。
6. 代码块为方角（无圆角）；语法高亮用 GitHub prettylights（亮）/ GitHub Dark（暗）双模式色板，
   选择器基于 syntect 输出的 scope atom 类名（`pre code span.keyword` 等），代码块背景色不变。
