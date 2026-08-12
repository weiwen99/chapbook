//! 路由集成测试: 元信息 / 文件服务 / 目录列表 / 文档与代码渲染 / 响应协商 / 安全.

use std::io::Write as _;
use std::path::Path;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode, header};
use tempfile::TempDir;
use tower::ServiceExt;

use chapbook::routes;

const TEXT1: &str = "text 1";
const TEXT2: &str = r#"{"key":"value"}"#;

fn app(root: &Path) -> Router {
    routes::app(root.to_path_buf())
}

async fn get(app: Router, uri: &str) -> Response<Body> {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_string(res: Response<Body>) -> String {
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn content_type(res: &Response<Body>) -> String {
    res.headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// 建一个含 1.txt 和 subdir/2.json 的测试目录.
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("1.txt"), TEXT1).unwrap();
    std::fs::create_dir(dir.path().join("subdir")).unwrap();
    std::fs::write(dir.path().join("subdir/2.json"), TEXT2).unwrap();
    dir
}

#[tokio::test]
async fn meta_api_works() {
    let dir = fixture();
    let res = get(app(dir.path()), "/__/status").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_string(res).await, "simple static server is running.\n");
}

#[tokio::test]
async fn serves_top_level_files() {
    let dir = fixture();
    let res = get(app(dir.path()), "/1.txt").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_string(res).await, TEXT1);
}

#[tokio::test]
async fn serves_nested_dir_files() {
    let dir = fixture();
    let res = get(app(dir.path()), "/subdir/2.json").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "application/json");
    assert_eq!(body_string(res).await, TEXT2);
}

#[tokio::test]
async fn missing_file_returns_404() {
    let dir = fixture();
    let res = get(app(dir.path()), "/nope.txt").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// text/* 响应必须带 charset=utf-8, 否则浏览器按 Latin-1 猜码, UTF-8 中文乱码
#[tokio::test]
async fn text_files_served_with_utf8_charset() {
    let dir = fixture();
    let chinese = "// 中文注释\nfn main() {}\n";
    std::fs::write(dir.path().join("main.rs"), chinese).unwrap();

    for (uri, expected_ct) in [
        ("/1.txt", "text/plain; charset=utf-8"),
        ("/main.rs", "text/x-rust; charset=utf-8"),
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "uri: {uri}");
        assert_eq!(content_type(&res), expected_ct, "uri: {uri}");
    }
    let res = get(app(dir.path()), "/main.rs").await;
    assert_eq!(body_string(res).await, chinese);
}

async fn get_with_accept(app: Router, uri: &str, accept: &str) -> Response<Body> {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .header(header::ACCEPT, accept)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

/// 代码文件: 浏览器 (Accept: text/html) -> syntect 高亮 HTML (带行号); curl (Accept: */*) -> 原文.
/// ?raw=1 / ?view=1 显式覆盖.
#[tokio::test]
async fn code_file_content_negotiation() {
    let dir = fixture();
    let rust_src = "// 你好\nfn main() { let x = 1; }\n";
    std::fs::write(dir.path().join("main.rs"), rust_src).unwrap();

    // 浏览器 -> 高亮 HTML
    let res = get_with_accept(
        app(dir.path()),
        "/main.rs",
        "text/html,application/xhtml+xml",
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    let body = body_string(res).await;
    assert!(
        body.contains(r#"<pre class="sourceCode"><code>"#),
        "body: {body}"
    );
    assert!(
        body.contains(r#"<span class="ln">1</span>"#),
        "should have line numbers: {body}"
    );
    assert!(body.contains("/* chapbook-doc-style */"), "body: {body}");
    assert!(body.contains("<title>main.rs</title>"), "body: {body}");

    // curl (Accept: */*) -> 原文
    let res = get_with_accept(app(dir.path()), "/main.rs", "*/*").await;
    assert_eq!(content_type(&res), "text/x-rust; charset=utf-8");
    assert_eq!(body_string(res).await, rust_src);

    // ?raw=1 即使浏览器 Accept 也返回原文
    let res = get_with_accept(app(dir.path()), "/main.rs?raw=1", "text/html").await;
    assert_eq!(content_type(&res), "text/x-rust; charset=utf-8");

    // ?view=1 即使 curl Accept 也返回 HTML
    let res = get_with_accept(app(dir.path()), "/main.rs?view=1", "*/*").await;
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
}

/// 内容含 ``` 的代码文件: 按代码渲染 (syntect 对非法语法宽容), 内容不截断.
#[tokio::test]
async fn code_file_with_embedded_fences() {
    let dir = fixture();
    let src = "# doc\n\n```python\nprint(1)\n```\n\ntail_marker_line = 1\n";
    std::fs::write(dir.path().join("readme.py"), src).unwrap();

    let res = get_with_accept(app(dir.path()), "/readme.py", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    // 高亮会把 token 包进 span, 断言内容须用标识符
    assert!(
        body.contains("tail_marker_line"),
        "content after embedded fence must survive: {body}"
    );
}

/// 常见语言覆盖: 内嵌补充语法 (TypeScript/TOML/Kotlin 等) 渲染出高亮 span 而非纯文本.
/// Dockerfile 无扩展名, 走文件名匹配路径.
#[tokio::test]
async fn code_file_highlighting_covers_common_languages() {
    let dir = fixture();
    let cases: &[(&str, &str)] = &[
        ("main.ts", "interface Foo {\n  bar: string;\n}\n"),
        ("Cargo.toml", "[package]\nname = \"x\"\n"),
        ("Dockerfile", "FROM rust:1.97\nRUN cargo build\n"),
        ("app.conf", "key = value\n"),
        ("main.kt", "fun main() { val x = 1 }\n"),
        ("main.swift", "let x = 1\n"),
        ("main.dart", "void main() { var x = 1; }\n"),
        ("main.ex", "defmodule Foo do\n  def bar, do: :ok\nend\n"),
        ("schema.graphql", "type Query { hello: String }\n"),
        (
            "main.proto",
            "syntax = \"proto3\";\nmessage Foo { string name = 1; }\n",
        ),
        ("main.zig", "const x: i32 = 1;\n"),
        ("default.nix", "{ pkgs }: pkgs.hello\n"),
    ];
    for (file, content) in cases {
        std::fs::write(dir.path().join(file), content).unwrap();
    }
    for (file, _) in cases {
        let res = get_with_accept(app(dir.path()), &format!("/{file}"), "text/html").await;
        assert_eq!(res.status(), StatusCode::OK, "{file}");
        let body = body_string(res).await;
        assert!(
            body.contains("<pre class=\"sourceCode\"><code>"),
            "{file}: {body}"
        );
        assert!(
            body.contains(r#"<span class="ln">1</span>"#),
            "{file}: {body}"
        );
        // 有高亮 span 而非 plain text 兜底
        assert!(
            body.contains("<span class="),
            "{file} rendered as plain text: {body}"
        );
    }
}

/// 非 UTF-8 代码文件: 不做编码猜测, 裸字节透传.
#[tokio::test]
async fn non_utf8_code_file_passthrough() {
    let dir = fixture();
    // GBK 编码的 "中文" 等非法 UTF-8 序列
    let gbk_bytes: &[u8] = b"// \xd6\xd0\xce\xc4\xc7\xf8\n";
    std::fs::write(dir.path().join("legacy.rs"), gbk_bytes).unwrap();

    let res = get_with_accept(app(dir.path()), "/legacy.rs", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/x-rust; charset=utf-8");
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.as_ref(), gbk_bytes);
}

/// 超过 1MB 的代码文件不做渲染, 直接走 ServeFile.
#[tokio::test]
async fn oversized_code_file_not_rendered() {
    let dir = fixture();
    let big = "x".repeat(1024 * 1024 + 1);
    std::fs::write(dir.path().join("big.rs"), &big).unwrap();

    let res = get_with_accept(app(dir.path()), "/big.rs", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/x-rust; charset=utf-8");
    assert_eq!(body_string(res).await, big);
}

/// 未知扩展名维持裸文本现状.
#[tokio::test]
async fn unknown_extension_stays_raw() {
    let dir = fixture();
    std::fs::write(dir.path().join("data.xyz123"), "hello").unwrap();
    let res = get_with_accept(app(dir.path()), "/data.xyz123", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_ne!(content_type(&res), "text/html; charset=utf-8");
    assert_eq!(body_string(res).await, "hello");
}

/// .html 是网页不是代码: 即使浏览器 Accept 也直接透传原文 (text/html), 让浏览器渲染网页.
#[tokio::test]
async fn html_files_render_natively() {
    let dir = fixture();
    let page = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body><h1>网页</h1></body></html>";
    std::fs::write(dir.path().join("page.html"), page).unwrap();

    let res = get_with_accept(app(dir.path()), "/page.html", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    let body = body_string(res).await;
    assert_eq!(
        body, page,
        "html file must be served verbatim, not highlighted"
    );
    assert!(!body.contains("sourceCode"), "body: {body}");
}

#[tokio::test]
async fn directory_listing_survives_broken_symlink() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("real.txt"), "real").unwrap();
    // Emacs 风格的锁文件: 目标不存在的悬空符号链接
    std::os::unix::fs::symlink("user@host.12345:67890", dir.path().join(".#real.txt")).unwrap();

    let res = get(app(dir.path()), "/").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains("real.txt"),
        "body should list real.txt: {body}"
    );
    // 主题样式与 thead 结构 (表头不应被条纹染色)
    assert!(
        body.contains(r#"href="/__/static/css/chapbook-theme.css""#),
        "body: {body}"
    );
    assert!(body.contains("<thead>"), "body: {body}");
}

/// .md 经 comrak 纯 Rust 渲染 (无 pandoc 依赖, 恒跑):
/// 完整页面 + 自建 TOC + syntect 高亮 + 同一 DOC_STYLE.
#[tokio::test]
async fn markdown_rendered_via_comrak() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("test.md"),
        "# Hello\n\nThis is **markdown**.\n\n## Section 2\n\n```scala\nval x = 1\n```\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/test.md").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    let body = body_string(res).await;
    assert!(body.contains("<strong>markdown</strong>"), "body: {body}");
    // 与 org 同一视觉系统: DOC_STYLE + 自建 TOC + syntect 高亮
    assert!(body.contains("/* chapbook-doc-style */"), "body: {body}");
    assert!(body.contains(r#"<nav id="TOC""#), "body: {body}");
    assert!(
        body.contains(r#"<pre class="sourceCode"><code class="language-scala">"#),
        "body: {body}"
    );
    // 高亮 token 类名 (syntect scope atom) 或纯文本都要保住代码内容;
    // token 被包进 span, 断言须按 token 边界
    assert!(body.contains(">val<"), "body: {body}");
    assert!(body.contains(">x<"), "body: {body}");
    assert!(body.contains(">1<"), "body: {body}");
}

/// .md 数学: comrak math_dollars/math_latex 扩展 + 服务端 KaTeX 渲染.
/// `$..$`/`$$..$$`/`\(..\)` 输出 .katex 结构; 页面引入 katex.min.css.
#[tokio::test]
async fn markdown_math_rendered_via_katex() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("math.md"),
        "# Math\n\n欧拉公式 $e^{i\\pi} + 1 = 0$ 与 \\(x^2\\), 以及\n\n$$\\cfrac{1}{1+\\cfrac{1}{2}} = \\frac{2}{3}$$\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/math.md").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    // 两个 inline + 一个 display 数学都渲染成 KaTeX HTML
    assert_eq!(body.matches(r#"class="katex""#).count(), 3, "body: {body}");
    assert!(body.contains(r#"class="katex-display""#), "body: {body}");
    // 数学 span 全部替换, 不残留 comrak 定界符
    assert!(!body.contains("data-math-style"), "body: {body}");
    // 页面引入 KaTeX 样式表 (与服务端渲染输出配套)
    assert!(
        body.contains(r#"href="/__/static/katex/katex.min.css""#),
        "body: {body}"
    );
}

/// .md 数学降级: comrak 判定为数学但 KaTeX 不认识的宏 → 保留 span 原文 (内容不丢).
#[tokio::test]
async fn markdown_math_fallback_keeps_raw() {
    let dir = fixture();
    std::fs::write(dir.path().join("badmath.md"), "# Bad\n\n$\\badmacro{x}$\n").unwrap();

    let res = get(app(dir.path()), "/badmath.md").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(!body.contains(r#"class="katex""#), "body: {body}");
    assert!(body.contains(r"\badmacro{x}"), "body: {body}");
}

/// md 与 org 共用同一 slug 函数: TOC href 与正文标题 id 一一对应 (含去重后缀).
#[tokio::test]
async fn markdown_toc_anchors_match_heading_ids() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("toc.md"),
        "# Intro\n\ntext\n\n## Deep **Dive**\n\n# Intro\n\n## 中文 标题\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/toc.md").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;

    let nav = body
        .split(r#"<nav id="TOC">"#)
        .nth(1)
        .and_then(|rest| rest.split("</nav>").next())
        .expect("TOC nav must exist");
    let mut toc_hrefs: Vec<&str> = nav
        .split("href=\"#")
        .skip(1)
        .map(|s| s.split('"').next().unwrap())
        .collect();
    toc_hrefs.sort_unstable();

    let mut heading_ids: Vec<&str> = Vec::new();
    for tag in [
        "<h1><a id=\"",
        "<h2><a id=\"",
        "<h3><a id=\"",
        "<h4><a id=\"",
        "<h5><a id=\"",
        "<h6><a id=\"",
    ] {
        for part in body.split(tag).skip(1) {
            heading_ids.push(part.split('"').next().unwrap());
        }
    }
    heading_ids.sort_unstable();

    assert_eq!(
        toc_hrefs, heading_ids,
        "TOC hrefs and heading ids must match 1:1: {body}"
    );
    // 粗体被扁平化: "Deep **Dive**" -> "Deep Dive" -> slug deep-dive
    assert!(heading_ids.contains(&"deep-dive"), "body: {body}");
    // 重复标题去重
    assert!(heading_ids.contains(&"intro"), "body: {body}");
    assert!(heading_ids.contains(&"intro-1"), "body: {body}");
}

/// YAML front matter: title 进 <title> 与 title-block-header, front matter 本身不显示.
#[tokio::test]
async fn markdown_front_matter_title() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("fm.md"),
        "---\ntitle: \"我的文档\"\n---\n\n# Section\n\nbody\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/fm.md").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains("<title>我的文档</title>"), "body: {body}");
    assert!(
        body.contains(
            r#"<header id="title-block-header"><h1 class="title">我的文档</h1></header>"#
        ),
        "body: {body}"
    );
    // front matter 不渲染为内容 (无孤立 hr/文本)
    assert!(!body.contains("title: \"我的文档\""), "body: {body}");
}

/// .org 经 orgize 纯 Rust 渲染 (无 pandoc/emacs 依赖, 恒跑):
/// 完整页面 + 自建 TOC + 标题锚点 + src 块 + 同一 DOC_STYLE.
#[tokio::test]
async fn org_rendered_via_orgize() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("test.org"),
        "* Hello Org\nThis is /org-mode/ text with =verbatim= code.\n\n#+begin_src scala\nval x = 1\n#+end_src\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/test.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    let body = body_string(res).await;
    // orgize 用 <i> 表示斜体 (pandoc 是 <em>)
    assert!(body.contains("<i>org-mode</i>"), "body: {body}");
    // verbatim -> <code> (orgize 输出无 class)
    assert!(body.contains("<code>verbatim</code>"), "body: {body}");
    // 自建 TOC 保留
    assert!(body.contains(r#"<nav id="TOC""#), "body: {body}");
    // 同一视觉系统: DOC_STYLE 注入 + 宽屏侧栏守卫
    assert!(body.contains("/* chapbook-doc-style */"), "body: {body}");
    assert!(
        body.contains("@media screen and (min-width: 75rem)"),
        "body: {body}"
    );
    // src 块: orgize 输出 <pre class="src src-scala">, 无高亮 token
    assert!(
        body.contains(r#"<pre class="src src-scala">"#),
        "body: {body}"
    );
    // CSS 中 pre.src 选择器保住代码块底色
    assert!(body.contains("pre.src"), "body: {body}");
}

/// .org 数学: orgize 不解析 LaTeX, 由 Text 元素 tokenize 后服务端 KaTeX 渲染.
/// `\(..\)`/`$..$` inline 与 `$$..$$`/`\begin{align}` display 都出 .katex 结构;
/// src 块内的 `$` (shell/Haskell) 不经过 Text, 保持原文.
#[tokio::test]
async fn org_math_rendered_via_katex() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("math.org"),
        r#"* Math
设其第 \( \sqrt{2} \) 项的渐进分数为 $a_{i-1}$。

\begin{align}
x^{3.0001} &= x^3 \cdot x^{0.0001} \\
&= \sqrt[10000]{x}
\end{align}

$$G^2_{i-1} - DB^2_{i-1} = Q_i$$

#+begin_src sh
echo $RELEASE_VERSION
#+end_src
"#,
    )
    .unwrap();

    let res = get(app(dir.path()), "/math.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    // 2 inline + 1 裸 align 环境 + 1 $$ 块 = 4 个 KaTeX 渲染
    assert_eq!(body.matches("class=\"katex\"").count(), 4, "body: {body}");
    // align 环境渲染为 mtable (对齐结构), 且内容完整
    assert!(body.contains("<mtable"), "body: {body}");
    assert!(!body.contains("data-math-style"), "body: {body}");
    // src 块里的 shell $ 变量不被当数学 (syntect 高亮把 `$` 与变量名拆成相邻 span)
    assert!(body.contains("RELEASE_VERSION"), "body: {body}");
}

/// .org 伪数学 ($ 后接空白 / KaTeX 解析失败): 保持原文, 不渲染.
#[tokio::test]
async fn org_fake_math_stays_raw() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("fakemath.org"),
        "价格 $ column: $ 与 $2 == 'patch' || $ 不变\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/fakemath.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    // 页面 head 含 katex.min.css 链接, 断言须限定 .katex 渲染结构
    assert!(!body.contains("class=\"katex\""), "body: {body}");
    assert!(body.contains("$ column: $"), "body: {body}");
    assert!(
        body.contains("$2 == &apos;patch&apos; || $"),
        "body: {body}"
    );
}

/// KaTeX 静态资源: 样式表 (裁剪版, 仅 woff2 引用) 与字体二进制可达; 未知字体 404.
#[tokio::test]
async fn katex_static_assets_served() {
    let dir = fixture();
    let res = get(app(dir.path()), "/__/static/katex/katex.min.css").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/css");
    let css = body_string(res).await;
    // 裁剪: 只留 woff2 引用 (woff/ttf 已移除)
    assert!(css.contains("woff2"), "css: {css}");
    assert!(!css.contains(".woff)"), "css: {css}");
    assert!(!css.contains(".ttf)"), "css: {css}");

    let res = get(
        app(dir.path()),
        "/__/static/katex/fonts/KaTeX_Main-Regular.woff2",
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "font/woff2");

    let res = get(app(dir.path()), "/__/static/katex/fonts/nope.woff2").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// 4 级标题保真: pandoc/emacs 都会丢 h4, orgize 保留 (spike plan-56 场景).
#[tokio::test]
async fn org_keeps_level4_headings() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("h4.org"),
        "* Top\n** Sub\n*** Subsub\n**** Level 4\ncontent\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/h4.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains("<h4>"), "h4 must survive: {body}");
    assert!(body.contains(">Level 4<"), "body: {body}");
}

/// TOC 链接与正文标题锚点必须由同一 slug 函数生成: nav 内 href 与标题 id 一一对应.
/// 重复标题走 -1/-2 去重后缀.
#[tokio::test]
async fn org_toc_anchors_match_heading_ids() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("toc.org"),
        "* Intro\ncontent\n** Deep Dive\nmore\n* Intro\nagain\n* Level 4 标题\nx\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/toc.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;

    let nav = body
        .split(r#"<nav id="TOC">"#)
        .nth(1)
        .and_then(|rest| rest.split("</nav>").next())
        .expect("TOC nav must exist");
    let mut toc_hrefs: Vec<&str> = nav
        .split("href=\"#")
        .skip(1)
        .map(|s| s.split('"').next().unwrap())
        .collect();
    toc_hrefs.sort_unstable();

    let mut heading_ids: Vec<&str> = Vec::new();
    for tag in [
        "<h1><a id=\"",
        "<h2><a id=\"",
        "<h3><a id=\"",
        "<h4><a id=\"",
        "<h5><a id=\"",
        "<h6><a id=\"",
    ] {
        for part in body.split(tag).skip(1) {
            heading_ids.push(part.split('"').next().unwrap());
        }
    }
    heading_ids.sort_unstable();

    assert_eq!(
        toc_hrefs, heading_ids,
        "TOC hrefs and heading ids must match 1:1: {body}"
    );
    // 重复标题去重: 两个 "Intro" -> intro 与 intro-1
    assert!(heading_ids.contains(&"intro"), "body: {body}");
    assert!(heading_ids.contains(&"intro-1"), "body: {body}");
}

/// #+TITLE -> <title> 与 header 下 h1.title (pandoc 行为对齐); #+AUTHOR/#+DATE 同渲染.
#[tokio::test]
async fn org_title_keyword_sets_page_title() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("titled.org"),
        "#+TITLE: 我的文档\n#+AUTHOR: Alice\n#+DATE: 2026-08-05\n\n* Section\nbody\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/titled.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains("<title>我的文档</title>"), "body: {body}");
    assert!(
        body.contains(
            r#"<header id="title-block-header"><h1 class="title">我的文档</h1><p class="author">Alice</p><p class="date">2026-08-05</p></header>"#
        ),
        "body: {body}"
    );
}

/// #+OPTIONS: toc:nil 关闭自建 TOC (标题与锚点仍正常渲染).
#[tokio::test]
async fn org_options_toc_nil_disables_toc() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("notoc.org"),
        "#+OPTIONS: toc:nil num:nil\n\n* Section\nbody\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/notoc.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(!body.contains(r#"<nav id="TOC""#), "body: {body}");
    assert!(body.contains("<h1>"), "headings still render: {body}");
}

/// 词法前缀比较不会消除 `..` 分量, `/%2e%2e/%2e%2e/...` 可以读到根目录外的文件.
/// 必须逐分量解析, 溢出即 403.
#[tokio::test]
async fn path_traversal_is_forbidden() {
    let dir = fixture();
    for uri in [
        "/%2e%2e/%2e%2e/%2e%2e/etc/passwd",
        "/subdir/../../../../../etc/passwd",
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "uri: {uri}");
    }
}

#[tokio::test]
async fn range_request_returns_partial_content() {
    let dir = fixture();
    let res = app(dir.path())
        .oneshot(
            Request::builder()
                .uri("/1.txt")
                .header(header::RANGE, "bytes=0-3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        res.headers().get(header::CONTENT_RANGE).unwrap(),
        &format!("bytes 0-3/{}", TEXT1.len()).as_str()
    );
    assert_eq!(body_string(res).await, &TEXT1[..4]);
}

#[tokio::test]
async fn invalid_sort_param_returns_404() {
    let dir = fixture();
    // 非法 sort 参数 -> 404 (不是 400), 保持既有行为
    let res = get(app(dir.path()), "/?sort=Name:Invalid").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sort_by_size_desc() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("small.txt"), "x").unwrap();
    std::fs::write(dir.path().join("large.txt"), "x".repeat(100)).unwrap();

    let res = get(app(dir.path()), "/?sort=Size:Desc").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    let large_pos = body.find("large.txt").unwrap();
    let small_pos = body.find("small.txt").unwrap();
    assert!(
        large_pos < small_pos,
        "Size:Desc should list large.txt first: {body}"
    );
}

/// 文件名含空格: 链接必须 percent-encode 为 %20 (表单语义的 `+` 在 path segment 是 bug),
/// 且 %20 URL 能正确解码回文件.
#[tokio::test]
async fn space_in_filename_is_percent_encoded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a b.txt"), "space").unwrap();

    let res = get(app(dir.path()), "/").await;
    let body = body_string(res).await;
    assert!(body.contains(r#"href="/a%20b.txt""#), "body: {body}");

    let res = get(app(dir.path()), "/a%20b.txt").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_string(res).await, "space");
}

#[tokio::test]
async fn static_assets_served() {
    let dir = fixture();
    let res = get(app(dir.path()), "/__/static/css/materialize.min.css").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/css");
    assert!(body_string(res).await.contains("Materialize"));

    let res = get(app(dir.path()), "/__/static/css/chapbook-theme.css").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/css");
    assert!(body_string(res).await.contains("chapbook-theme"));

    let res = get(app(dir.path()), "/__/static/js/materialize.min.js").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "application/javascript");

    let res = get(app(dir.path()), "/__/static/css/nope.css").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ---------- Office 文档 / CSV 渲染 (anydoc) ----------

/// 构造最小 docx (zip 包: [Content_Types].xml + _rels/.rels + word/document.xml).
fn docx_bytes(body_text: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
        )
        .unwrap();
        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
        )
        .unwrap();
        zip.start_file("word/document.xml", options).unwrap();
        write!(
            zip,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{body_text}</w:t></w:r></w:p></w:body></w:document>"#
        )
        .unwrap();
        zip.finish().unwrap();
    }
    buf.into_inner()
}

/// CSV: 浏览器 (Accept: text/html) -> anydoc 转 GFM markdown 表格 -> comrak 渲染文档页.
/// 表格必须进 <table> (comrak GFM 表格扩展), 与 .md 同一文档页骨架.
#[tokio::test]
async fn office_csv_rendered_as_table() {
    let dir = fixture();
    let csv = "name,age\nAlice,30\nBob,25\n";
    std::fs::write(dir.path().join("data.csv"), csv).unwrap();

    let res = get_with_accept(app(dir.path()), "/data.csv", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    let body = body_string(res).await;
    assert!(body.contains("<table>"), "body: {body}");
    assert!(body.contains("Alice"), "body: {body}");
    assert!(body.contains("Bob"), "body: {body}");
}

/// CSV: 脚本客户端 (Accept: */*) -> 原文 text/csv, 不被劫持成 HTML.
#[tokio::test]
async fn office_csv_raw_for_script_clients() {
    let dir = fixture();
    let csv = "name,age\nAlice,30\n";
    std::fs::write(dir.path().join("data.csv"), csv).unwrap();

    let res = get_with_accept(app(dir.path()), "/data.csv", "*/*").await;
    assert_eq!(res.status(), StatusCode::OK);
    // ensure_text_charset 会给 text/* 补 charset, 与 text/plain 行为一致
    assert_eq!(content_type(&res), "text/csv; charset=utf-8");
    assert_eq!(body_string(res).await, csv);
}

/// CSV: ?view=1 强制渲染, ?raw=1 强制原文 (与代码文件同一协商覆盖).
#[tokio::test]
async fn office_view_raw_overrides() {
    let dir = fixture();
    let csv = "name,age\nAlice,30\n";
    std::fs::write(dir.path().join("data.csv"), csv).unwrap();

    let res = get_with_accept(app(dir.path()), "/data.csv?view=1", "*/*").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    assert!(body_string(res).await.contains("Alice"));

    let res = get_with_accept(app(dir.path()), "/data.csv?raw=1", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/csv; charset=utf-8");
    assert_eq!(body_string(res).await, csv);
}

/// docx: anydoc 转 GFM markdown -> comrak 渲染, 正文文本出现在文档页.
#[tokio::test]
async fn office_docx_rendered_via_anydoc() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("report.docx"),
        docx_bytes("Hello from docx"),
    )
    .unwrap();

    let res = get_with_accept(app(dir.path()), "/report.docx", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/html; charset=utf-8");
    let body = body_string(res).await;
    assert!(body.contains("Hello from docx"), "body: {body}");
    // 与 .md 同一文档页骨架 (DOC_STYLE)
    assert!(body.contains("chapbook-doc"), "body: {body}");
}

/// 损坏的 Office 文件: 转换失败 -> 裸字节透传, 浏览器下载后由本地应用打开.
#[tokio::test]
async fn office_garbage_falls_back_to_raw() {
    let dir = fixture();
    let garbage = b"this is not a docx at all, just some bytes that look like a word file";
    std::fs::write(dir.path().join("broken.docx"), garbage).unwrap();

    let res = get_with_accept(app(dir.path()), "/broken.docx", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    let ct = content_type(&res);
    assert!(!ct.contains("text/html"), "content-type: {ct}");
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.to_vec(), garbage);
}

/// 超过 office::MAX_RENDER_BYTES 的文件不做转换 (转换是 CPU 活, 大文件耗时无界).
#[tokio::test]
async fn oversized_office_file_not_rendered() {
    let dir = fixture();
    let big = "a,b\n".repeat(9 * 1024 * 1024); // "a,b\n" 4 字节 x 9 MiB 行 = 36 MiB > 32 MiB
    std::fs::write(dir.path().join("big.csv"), big).unwrap();
    assert!(std::fs::metadata(dir.path().join("big.csv")).unwrap().len() > 32 * 1024 * 1024);

    let res = get_with_accept(app(dir.path()), "/big.csv", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/csv; charset=utf-8");
}

/// PDF 不进 anydoc 渲染路径: 浏览器原生打开 (ServeFile 透传 application/pdf).
#[tokio::test]
async fn pdf_still_served_raw() {
    let dir = fixture();
    let pdf = b"%PDF-1.4 fake pdf bytes";
    std::fs::write(dir.path().join("doc.pdf"), pdf).unwrap();

    let res = get_with_accept(app(dir.path()), "/doc.pdf", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "application/pdf");
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.to_vec(), pdf);
}
