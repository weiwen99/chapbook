# AGENTS.md

本文件面向在本仓库工作的 AI 编码代理与贡献者。

## 项目简介

**chapbook**（简称 **CB**）是一个轻量静态文件服务器，二进制名同为 `chapbook`。功能：目录列表（Materialize UI、可排序）、
静态文件服务（Range 请求）、`.md` 经 comrak、`.org` 经 orgize、Office/CSV 文档经 anydoc、源代码文件经
syntect 渲染（全部纯 Rust、零运行时外部依赖）。

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
  math.rs      # LaTeX 数学公式服务端 KaTeX 渲染（md span 替换 / org Text tokenize）
  org.rs       # orgize 渲染 .org：预扫描元数据 + 自定义 HtmlHandler + src 块高亮
  markdown.rs  # comrak 渲染 .md：front matter、HeadingAdapter 锚点/TOC、代码高亮适配器
  office.rs    # anydoc 转换 Office/CSV 为 GFM markdown：格式表（排除 PDF）、大小上限
  routes.rs    # axum 路由与 handler
  assets.rs    # include_str! 内嵌前端资源
assets/        # materialize.min.css / materialize.min.js（v2.3.3 社区分支）+ chapbook-theme.css（目录页主题）
               # + chapbook-doc.css（文档/代码页样式）
               # + katex/（KaTeX 0.16.7 样式表 woff2 裁剪版 + 20 个 woff2 字体）
               # + syntaxes/（14 个补充语法定义，syntect 默认集缺失的常见语言；见 THIRD_PARTY_NOTICES）
vendor/        # quick-js 0.4.1（katex crate 的 JS 引擎）本地副本：唯一改动是
               # src/bindings.rs ContextWrapper::new 里 JS_SetMaxStackSize 256KB -> 8MB
               # （KaTeX 解析 7+ 层 \cfrac 嵌套会栈溢出），经 [patch.crates-io] 生效
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
   pandoc/emacs 会结构性丢失）。md 内 raw HTML 转义显示（安全：文档页不执行内嵌 HTML）。
   数学公式**服务端 KaTeX 渲染**（`math.rs`，官方 KaTeX 0.16.7 经 QuickJS 执行）：
   md 开 comrak `math_dollars`/`math_latex` 扩展（`$..$`/`$$..$$`/`\(..\)`/`\[..\]`，pandoc
   同款启发式判定），输出 `<span data-math-style=...>` 由 `math::replace_comrak_math` 后处理
   替换；org 因 orgize 无 LaTeX 节点，在自定义 HtmlHandler 的 `Element::Text` 钩子里
   tokenize `\(..\)`/`\[..\]`/`$..$`/`$$..$$`/`\begin{env}..\end{env}`（环境名限定
   `math::take_environment` 白名单，src/example/code/verbatim 是独立元素不经过 Text，
   shell/Haskell 的 `$` 不会误伤）。**降级约定**：启发式拒绝（`$` 后接空白、超长）或
   KaTeX 解析失败一律回退原文显示，内容不丢；`$ column: $`、`$2 == 'patch'$` 保持原样。
   页面骨架在 `<head>` 注入 `/__/static/katex/katex.min.css`（裁剪版，仅 woff2 引用；
   字体走 `assets/katex/fonts/` 二进制内嵌），`chapbook-doc.css` 对 `.katex-display`
   加横向溢出滚动（长 align 公式不撑破布局），颜色继承正文（亮/暗自适应）。
   已废弃：Markdeep 客户端渲染与 pandoc/emacs 子进程管线（提案
   docs/2026-08-05-proposal-remove-dependency-of-pandoc.org，Phase 1/2 已实施）。
6. 文件服务必须支持 Range（tower-http `ServeFile` 提供）；缺失文件 404。
7. 源代码文件（`highlight.rs` 的扩展名映射表内）按 **Accept 协商**返回：`text/html` → syntect
   高亮 HTML（带行号 `<span class="ln">`，同一 DOC_STYLE）；否则（curl 等）→ 原文。
   `?raw=1` / `?view=1` 显式覆盖。非 UTF-8 与 >1MB 文件不渲染，裸字节透传。
8. Office/CSV 文档（`office.rs` 的 anydoc 格式表内：doc/docx/docm、ppt/pps/pot/pptx/pptm/ppsx/ppsm、
   xls/xlsx/xlsm/xlsb、odt/ods/odp、rtf、epub、csv；**PDF 除外**——浏览器原生打开）同样按
   **Accept 协商**返回：`text/html` → anydoc 转 GFM markdown 后经 `render_markdown_response`
   渲染为文档页（与 .md 同一 comrak 管线，TOC/锚点/高亮一致）；否则 → 原文。
   `?raw=1` / `?view=1` 显式覆盖。转换失败（ConvertError：加密/损坏/超限）与 >32 MiB
   （`office::MAX_RENDER_BYTES`）文件不渲染，ServeFile 裸字节透传。
   判定顺序：目录 → .md → .org → Office/CSV → 代码 → ServeFile。

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
| katex 0.4 | LaTeX 数学渲染 | 内嵌官方 KaTeX 0.16.7 `katex.min.js`，经 quick-js 执行（构建期编译 QuickJS C 源码，非运行时依赖）；渲染失败返回 Err，math.rs 回退原文；**升级时 `katex` crate 版本必须与 `assets/katex/` 的 CSS/字体版本一致**（见 THIRD_PARTY_NOTICES）；二进制 +~2.5MB（引擎 + 字体） |
| quick-js 0.4.1（vendor/ 本地 patch） | katex 的 JS 引擎 | 上游无栈配置 API；本地副本把 QuickJS JS 栈 256KB→8MB（`JS_SetMaxStackSize`，见 vendor/quick-js/src/bindings.rs），否则 KaTeX 解析深嵌套公式（`\cfrac` 7+ 层，pell 连分数文档真实场景）栈溢出回退原文；经 `[patch.crates-io]` 生效，升级 katex 时需同步核对 |
| anydoc 0.1.8 | Office/CSV → GFM markdown | 纯 Rust（zip/calamine/cfb/quick-xml/pdf-inspector），无子进程；**锁精确版本**（0.1.x 早期，API 可能变动；升级需核对 `Format` 表与 `ConvertError`）；MSRV 1.88；二进制 +~6MB；经 `log` facade 报恢复事件，`tracing-log` 桥接到 RUST_LOG |
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
3. 零运行时外部依赖（无子进程、无外部二进制）：.md/comrak、.org/orgize、代码/syntect、Office/CSV/anydoc
   全部纯 Rust；LaTeX 数学/katex crate（官方 KaTeX JS 经 quick-js 执行，quick-js 是构建期编译的
   C 源码，静态链接进二进制，不引入运行时依赖）；文档/代码渲染全部服务端完成，无前端 JS 依赖。
4. `.md` 不走客户端 JS 渲染（Markdeep 依赖外部 CDN，离线即无法渲染，且与 org 页视觉不一致），
   改由 comrak 服务端渲染；无 `-c/--css` 选项。
5. 全局字号约为浏览器默认的 80%，org/md 正文栏宽 100rem（与字号等比，保持每行字数）。
6. 代码块为方角（无圆角）；语法高亮用 GitHub prettylights（亮）/ GitHub Dark（暗）双模式色板，
   选择器基于 syntect 输出的 scope atom 类名（`pre code span.keyword` 等），代码块背景色不变。
