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
    routes::app(root.to_path_buf()).expect("app initialization")
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

/// .org 裸图片链接 (`[[file:img.png]]` 无描述) → `<img>`; 有描述的链接保持 `<a>`.
#[tokio::test]
async fn org_bare_image_links_render_as_img() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("img.org"),
        "图片测试\n\n[[./static/pic.jpg]]\n\n[[./static/pic.png][查看原图]]\n\n[[https://example.com/doc.pdf]]\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/img.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    // 裸图片链接 → img (alt 与 src 同路径)
    assert!(
        body.contains(r#"<img src="./static/pic.jpg" alt="./static/pic.jpg">"#),
        "body: {body}"
    );
    // 有描述 → 普通链接
    assert!(
        body.contains(r#"<a href="./static/pic.png">查看原图</a>"#),
        "body: {body}"
    );
    // 非图片 (pdf) → 普通链接
    assert!(
        body.contains(r#"<a href="https://example.com/doc.pdf">"#),
        "body: {body}"
    );
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

/// 完整文档页骨架: body 带 cb-doc-page 类, 正文恰好一个 cb-doc 包装,
/// DOC_STYLE 标记保留 (片段任务后续复用同一 .cb-doc 结构).
#[tokio::test]
async fn org_full_page_has_page_class_and_single_doc_wrapper() {
    let dir = fixture();
    std::fs::write(dir.path().join("wrap.org"), "* Hi\nbody\n").unwrap();

    let res = get(app(dir.path()), "/wrap.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains(r#"<body class="cb-doc-page">"#),
        "body: {body}"
    );
    assert_eq!(
        body.matches(r#"<div class="cb-doc">"#).count(),
        1,
        "body: {body}"
    );
    assert!(body.contains("/* chapbook-doc-style */"), "body: {body}");
    assert!(body.contains("<main>"), "body: {body}");
    assert!(body.contains("<title>wrap.org</title>"), "body: {body}");
}

/// Hostile org 整页渲染: HTML export block / snippet / javascript 链接
/// 只输出转义文本, 不出现活动元素标签.
#[tokio::test]
async fn org_hostile_html_and_links_are_escaped() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("hostile.org"),
        "* Evil\n\n#+begin_export html\n<script>alert(1)</script><img src=x onerror=alert(2)>\n#+end_export\n\n@@html:<b onclick=alert(3)>x</b>@@\n\n[[javascript:alert(4)]]\n",
    )
    .unwrap();

    let res = get(app(dir.path()), "/hostile.org").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    // 无活动元素标签
    assert!(!body.contains("<script"), "body: {body}");
    assert!(!body.contains("<img"), "body: {body}");
    assert!(!body.contains("<b "), "body: {body}");
    // 转义后的内容完整保留
    assert!(
        body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "body: {body}"
    );
    assert!(
        body.contains("&lt;b onclick=alert(3)&gt;x&lt;/b&gt;"),
        "body: {body}"
    );
    // 危险链接: 空 href, 标签保留
    assert!(
        body.contains(r#"<a href="">javascript:alert(4)</a>"#),
        "body: {body}"
    );
}

/// .txt/.log 映射 Plain Text 语法: 浏览器 Accept -> 转义文本的代码页 (行号, 无 token span);
/// 脚本客户端 -> 原文 text/plain.
#[tokio::test]
async fn txt_and_log_render_as_plain_text_code_page() {
    let dir = fixture();
    std::fs::write(dir.path().join("note.txt"), "hello <world> & bye\n").unwrap();
    std::fs::write(dir.path().join("app.log"), "line one\nline two\n").unwrap();

    for (file, escaped_fragment) in [
        ("note.txt", "&lt;world&gt; &amp; bye"),
        ("app.log", "line one"),
    ] {
        let res = get_with_accept(app(dir.path()), &format!("/{file}"), "text/html").await;
        assert_eq!(res.status(), StatusCode::OK, "{file}");
        assert_eq!(content_type(&res), "text/html; charset=utf-8", "{file}");
        let body = body_string(res).await;
        assert!(
            body.contains(r#"<pre class="sourceCode"><code>"#),
            "{file}: {body}"
        );
        assert!(
            body.contains(r#"<span class="ln">1</span>"#),
            "{file}: {body}"
        );
        assert!(body.contains(escaped_fragment), "{file}: {body}");
        assert!(!body.contains(r#"class="keyword""#), "{file}: {body}");
        assert!(body.contains("/* chapbook-doc-style */"), "{file}: {body}");
    }

    // 脚本客户端 (Accept: */*) 仍拿原文
    let res = get_with_accept(app(dir.path()), "/note.txt", "*/*").await;
    assert_eq!(content_type(&res), "text/plain; charset=utf-8");
    assert_eq!(body_string(res).await, "hello <world> & bye\n");
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

/// 非 UTF-8 md/org 全文回退 ServeFile 必须保留原请求头:
/// Range 请求返回 206 部分内容, 而非全量 200.
#[tokio::test]
async fn non_utf8_markdown_and_org_range_regression() {
    let dir = fixture();
    // GBK "中文" 等非法 UTF-8 序列
    let gbk: &[u8] = b"\xd6\xd0\xce\xc4\xc7\xf8 body";
    std::fs::write(dir.path().join("legacy.md"), gbk).unwrap();
    std::fs::write(dir.path().join("legacy.org"), gbk).unwrap();

    for name in ["legacy.md", "legacy.org"] {
        let res = app(dir.path())
            .oneshot(
                Request::builder()
                    .uri(format!("/{name}"))
                    .header(header::RANGE, "bytes=0-3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT, "{name}");
        assert_eq!(
            res.headers().get(header::CONTENT_RANGE).unwrap(),
            &format!("bytes 0-3/{}", gbk.len()).as_str(),
            "{name}"
        );
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.as_ref(), &gbk[..4], "{name}");
    }
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

/// 非 UTF-8 basename: 目录行仍显示名称 (bdi 隔离, 经显示 codec 转义),
/// 但 href() 为 None -> 不渲染任何锚点, 更不能有 href="" (空 href 点击会导航回当前目录).
#[cfg(unix)]
#[tokio::test]
async fn non_utf8_basename_listed_without_anchor() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(OsString::from_vec(b"\xFFbad.txt".to_vec())),
        b"x",
    )
    .unwrap();

    let res = get(app(dir.path()), "/").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains(
            r#"<td><bdi dir="auto">\xFFbad.txt</bdi><span class="cb-non-utf8">非 UTF-8 名称（仅显示）</span></td>"#
        ),
        "body: {body}"
    );
    // 非 UTF-8 行不渲染任何锚点: 无空 href (空 href 点击会导航回当前目录),
    // 且该条目不产生任何 <a>; 表头排序链接仍在 (页面 head 的静态资源 link
    // 是 <link> 不是 <a>, 与条目锚点无关)
    assert!(!body.contains(r#"<a href="">"#), "body: {body}");
    assert!(!body.contains(r#"<a href="/\xFFbad.txt""#), "body: {body}");
    // 表头排序链接仍在
    assert!(body.contains(r#"href="?sort="#), "body: {body}");
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

// ---------- Task 5: ResponseMode / download / CSP / fragments / search ----------

/// 模式优先级 (download > raw > fragment > view) 的两两组合裁定.
#[tokio::test]
async fn response_mode_priority_pairs() {
    let dir = fixture();
    let source = "# Md\nbody\n";
    std::fs::write(dir.path().join("doc.md"), source).unwrap();
    let app = app(dir.path());

    // download > raw / fragment / view: 原始字节 + attachment
    for uri in [
        "/doc.md?download=1&raw=1",
        "/doc.md?download=1&fragment=1",
        "/doc.md?download=1&view=1",
    ] {
        let res = get(app.clone(), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_DISPOSITION).is_some(),
            "{uri}: download must win"
        );
        assert_eq!(body_string(res).await, source, "{uri}");
    }
    // raw > fragment / view: 原始字节, 无 attachment
    for uri in ["/doc.md?raw=1&fragment=1", "/doc.md?raw=1&view=1"] {
        let res = get(app.clone(), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_DISPOSITION).is_none(),
            "{uri}: raw must win over fragment/view"
        );
        assert_eq!(body_string(res).await, source, "{uri}");
    }
    // fragment > view: 严格 fragment (生成 HTML, 无 doctype)
    let res = get(app.clone(), "/doc.md?fragment=1&view=1").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS).is_some(),
        "fragment must set nosniff"
    );
    let body = body_string(res).await;
    assert!(body.contains(r#"<div class="cb-fragment""#), "body: {body}");
    assert!(!body.contains("<!DOCTYPE"), "body: {body}");
}

/// ?raw=1 对 md/org 返回原文 (此前 raw 被忽略仍渲染 HTML); ?download=1 原文 + attachment.
#[tokio::test]
async fn markdown_org_raw_and_download_return_source() {
    let dir = fixture();
    let md = "# Md\nbody\n";
    let org = "* Org\nbody\n";
    std::fs::write(dir.path().join("doc.md"), md).unwrap();
    std::fs::write(dir.path().join("doc.org"), org).unwrap();

    for (uri, expected) in [("/doc.md?raw=1", md), ("/doc.org?raw=1", org)] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert_eq!(body_string(res).await, expected, "{uri}");
    }
    for (uri, expected) in [("/doc.md?download=1", md), ("/doc.org?download=1", org)] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_DISPOSITION).is_some(),
            "{uri}"
        );
        assert_eq!(body_string(res).await, expected, "{uri}");
    }
}

/// RFC 8187: 中文/空格/括号/星号 basename 的 attachment header 可解析且逐字节精确.
#[tokio::test]
async fn download_uses_rfc8187_content_disposition() {
    let dir = tempfile::tempdir().unwrap();
    let source = "# 标题\n\n正文\n";
    std::fs::write(dir.path().join("报告 (final)*.md"), source).unwrap();
    let uri = "/%E6%8A%A5%E5%91%8A%20(final)*.md?download=1";

    let res = get_with_accept(app(dir.path()), uri, "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"____final__.md\"; \
         filename*=UTF-8''%E6%8A%A5%E5%91%8A%20%28final%29%2A.md"
    );
    assert_eq!(body_string(res).await, source);

    // Accept 不影响 download; Range 请求仍 206 且保留 attachment
    let res = app(dir.path())
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::ACCEPT, "text/html")
                .header(header::RANGE, "bytes=0-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        res.headers().get(header::CONTENT_RANGE).unwrap(),
        &format!("bytes 0-1/{}", source.len()).as_str()
    );
    assert!(res.headers().get(header::CONTENT_DISPOSITION).is_some());
    assert_eq!(body_string(res).await, &source[..2]);
}

/// download 覆盖 md/org/代码/二进制/Office, 一律原始字节 + attachment, Accept 被忽略.
#[tokio::test]
async fn download_covers_all_file_types_accept_ignored() {
    let dir = fixture();
    std::fs::write(dir.path().join("doc.md"), "# Md\n").unwrap();
    std::fs::write(dir.path().join("doc.org"), "* Org\n").unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("data.bin"), b"\x00\x01\x02BIN").unwrap();
    std::fs::write(dir.path().join("broken.docx"), b"garbage docx bytes").unwrap();

    for (uri, expected_body, name) in [
        ("/doc.md?download=1", "# Md\n", "doc.md"),
        ("/doc.org?download=1", "* Org\n", "doc.org"),
        ("/main.rs?download=1", "fn main() {}\n", "main.rs"),
        ("/data.bin?download=1", "\x00\x01\x02BIN", "data.bin"),
        (
            "/broken.docx?download=1",
            "garbage docx bytes",
            "broken.docx",
        ),
    ] {
        let res = get_with_accept(app(dir.path()), uri, "text/html").await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        let expected_disposition =
            format!("attachment; filename=\"{name}\"; filename*=UTF-8''{name}");
        assert_eq!(
            res.headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap()
                .to_str()
                .unwrap(),
            expected_disposition.as_str(),
            "{uri}"
        );
        assert_eq!(body_string(res).await, expected_body, "{uri}");
    }
}

/// 所有 ServeFile 响应统一附加 `sandbox allow-scripts` CSP, 绝不 allow-same-origin.
#[tokio::test]
async fn raw_responses_get_sandbox_csp_without_allow_same_origin() {
    let dir = fixture();
    std::fs::write(dir.path().join("evil.html"), "<script>fetch('/')</script>").unwrap();
    std::fs::write(
        dir.path().join("evil.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg'><script>fetch('/')</script></svg>",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("doc.md"), "# Md\n").unwrap();

    for uri in [
        "/evil.html",
        "/evil.svg",
        "/1.txt",
        "/main.rs?raw=1",
        "/doc.md?download=1",
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        let csp = res
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(csp, "sandbox allow-scripts", "{uri}");
        assert!(!csp.contains("allow-same-origin"), "{uri}");
    }
}

/// Chromium 会拒绝加载携带 document `sandbox` CSP 的 audio/video 子资源。
/// 可播放媒体因此省略 sandbox，但必须 nosniff；主动内容与下载仍走 sandbox 出口。
#[tokio::test]
async fn playable_media_omits_sandbox_and_requires_nosniff() {
    let dir = fixture();
    std::fs::write(dir.path().join("tone.wav"), b"RIFF").unwrap();
    std::fs::write(dir.path().join("clip.mp4"), b"media").unwrap();

    for uri in ["/tone.wav", "/clip.mp4"] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_SECURITY_POLICY).is_none(),
            "{uri}: document sandbox breaks Chromium media loading"
        );
        assert_eq!(
            res.headers().get(header::X_CONTENT_TYPE_OPTIONS),
            Some(&header::HeaderValue::from_static("nosniff")),
            "{uri}: media exemption must not permit MIME sniffing"
        );
    }

    let res = app(dir.path())
        .oneshot(
            Request::builder()
                .uri("/tone.wav")
                .header(header::RANGE, "bytes=0-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert!(res.headers().get(header::CONTENT_SECURITY_POLICY).is_none());
    assert_eq!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS),
        Some(&header::HeaderValue::from_static("nosniff"))
    );

    let res = app(dir.path())
        .oneshot(
            Request::builder()
                .uri("/tone.wav")
                .header(header::RANGE, "bytes=999-1000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        res.headers().get(header::CONTENT_SECURITY_POLICY).unwrap(),
        "sandbox allow-scripts"
    );
    assert!(
        !res.headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("allow-same-origin")
    );

    let res = get(app(dir.path()), "/tone.wav?download=1").await;
    assert_eq!(
        res.headers().get(header::CONTENT_SECURITY_POLICY).unwrap(),
        "sandbox allow-scripts"
    );
    assert!(res.headers().contains_key(header::CONTENT_DISPOSITION));
}

/// 可信 UI (目录/搜索页): 精确 anti-framing 头, 无 sandbox; 完整文档页两者皆无.
#[tokio::test]
async fn trusted_pages_have_anti_frame_and_no_sandbox() {
    let dir = fixture();
    std::fs::write(dir.path().join("test.md"), "# T\nbody\n").unwrap();
    std::fs::write(dir.path().join("test.org"), "* T\nbody\n").unwrap();

    for uri in ["/", "/__/search?q=1.txt", "/__/search"] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        let csp = res
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(csp, "frame-ancestors 'none'", "{uri}");
        assert_eq!(
            res.headers().get(header::X_FRAME_OPTIONS).unwrap(),
            "DENY",
            "{uri}"
        );
    }

    // 完整生成文档页: 不 sandbox 也不需要 anti-frame (无 token)
    for uri in ["/test.md", "/test.org"] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_SECURITY_POLICY).is_none(),
            "{uri}: trusted doc page must not be sandboxed"
        );
        assert!(
            res.headers().get(header::X_FRAME_OPTIONS).is_none(),
            "{uri}"
        );
    }
}

/// 目录忽略 download/raw/view (恒完整可信目录页); fragment 模式返回迷你列表.
#[tokio::test]
async fn directory_ignores_mode_keys_except_fragment() {
    let dir = fixture();
    for uri in ["/?download=1", "/?raw=1", "/?view=1"] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_DISPOSITION).is_none(),
            "{uri}"
        );
        let body = body_string(res).await;
        assert!(body.contains("<!DOCTYPE"), "{uri}: full page expected");
        assert!(body.contains("1.txt"), "{uri}");
    }
    let res = get(app(dir.path()), "/?fragment=1").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains(r#"class="cb-doc cb-dir-fragment""#),
        "body: {body}"
    );
    assert!(!body.contains("<!DOCTYPE"), "body: {body}");
}

/// fragment 成功路径: 干净 HTML fragment (无 doctype/head/style), nosniff,
/// wrapper 携带 encoded path; md/org/代码/CSV 内容都渲染.
#[tokio::test]
async fn fragment_documents_and_code_are_clean_wrapped_html() {
    let dir = fixture();
    std::fs::write(dir.path().join("doc.md"), "# 标题\n\n正文 *斜体*\n").unwrap();
    std::fs::write(dir.path().join("doc.org"), "* 标题\n正文\n").unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("data.csv"), "name,age\nAlice,30\n").unwrap();

    for (uri, needle) in [
        ("/doc.md?fragment=1", "<em>斜体</em>"),
        ("/doc.org?fragment=1", "标题"),
        ("/main.rs?fragment=1", r#"<pre class="sourceCode"><code>"#),
        ("/data.csv?fragment=1", "<table>"),
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert_eq!(content_type(&res), "text/html; charset=utf-8", "{uri}");
        assert_eq!(
            res.headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap()
                .to_str()
                .unwrap(),
            "nosniff",
            "{uri}"
        );
        let body = body_string(res).await;
        assert!(!body.contains("<!DOCTYPE"), "{uri}: {body}");
        assert!(!body.contains("<head"), "{uri}: {body}");
        assert!(!body.contains("<style"), "{uri}: {body}");
        assert!(
            body.contains(r#"<div class="cb-fragment""#),
            "{uri}: {body}"
        );
        assert!(body.contains(r#"<div class="cb-doc">"#), "{uri}: {body}");
        assert!(body.contains(needle), "{uri}: {body}");
        let encoded = uri
            .trim_start_matches('/')
            .split('?')
            .next()
            .unwrap()
            .to_string();
        assert!(
            body.contains(&format!(r#"data-native-path-encoded="{encoded}""#)),
            "{uri}: wrapper must carry encoded path: {body}"
        );
    }
}

/// 媒体 fragment: wrapper 携带 encoded path, 元素 src 是 exact URL (无 fragment query);
/// SVG 只以 <img> 嵌入, 不做 inline.
#[tokio::test]
async fn fragment_media_embeds_element_with_exact_url() {
    let dir = fixture();
    std::fs::write(dir.path().join("pic.png"), b"PNG").unwrap();
    std::fs::write(dir.path().join("clip.mp4"), b"MP4").unwrap();
    std::fs::write(dir.path().join("song.mp3"), b"MP3").unwrap();
    std::fs::write(dir.path().join("logo.svg"), b"<svg/>").unwrap();

    for (uri, needle, absent) in [
        ("/pic.png?fragment=1", r#"<img src="/pic.png""#, ""),
        (
            "/clip.mp4?fragment=1",
            r#"<video src="/clip.mp4" controls"#,
            "",
        ),
        (
            "/song.mp3?fragment=1",
            r#"<audio src="/song.mp3" controls"#,
            "",
        ),
        ("/logo.svg?fragment=1", r#"<img src="/logo.svg""#, "<svg"),
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert_eq!(content_type(&res), "text/html; charset=utf-8", "{uri}");
        assert!(
            res.headers().get(header::X_CONTENT_TYPE_OPTIONS).is_some(),
            "{uri}"
        );
        let body = body_string(res).await;
        assert!(body.contains(needle), "{uri}: {body}");
        assert!(!body.contains("<!DOCTYPE"), "{uri}: {body}");
        assert!(
            !body.contains("?fragment="),
            "{uri}: src must be exact URL: {body}"
        );
        if !absent.is_empty() {
            assert!(!body.contains(absent), "{uri}: {body}");
        }
    }
}

/// fragment 失败路径: 占位 fragment (200 + nosniff), 绝不含原始字节.
#[tokio::test]
async fn fragment_failures_are_placeholders_without_raw_bytes() {
    let dir = fixture();
    let big = "x".repeat(1024 * 1024 + 1);
    std::fs::write(dir.path().join("big.rs"), &big).unwrap();
    std::fs::write(dir.path().join("legacy.md"), b"# \xd6\xd0\xce\xc4\n").unwrap();
    let garbage = b"this is not a docx at all, just some bytes that look like a word file";
    std::fs::write(dir.path().join("broken.docx"), garbage).unwrap();
    std::fs::write(dir.path().join("data.xyz123"), "hello").unwrap();

    for uri in [
        "/big.rs?fragment=1",
        "/legacy.md?fragment=1",
        "/broken.docx?fragment=1",
        "/data.xyz123?fragment=1",
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert_eq!(content_type(&res), "text/html; charset=utf-8", "{uri}");
        assert_eq!(
            res.headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap()
                .to_str()
                .unwrap(),
            "nosniff",
            "{uri}"
        );
        let body = body_string(res).await;
        assert!(
            body.contains(r#"<p class="cb-no-preview">无法预览："#),
            "{uri}: {body}"
        );
        assert!(!body.contains(&"x".repeat(64)), "{uri}: {body}");
        assert!(!body.contains("docx at all"), "{uri}: {body}");
        assert!(!body.contains("hello"), "{uri}: {body}");
    }
}

/// fragment wrapper 的属性值是 encoded path, 绝不出现 decoded 路径.
#[tokio::test]
async fn fragment_wrapper_uses_encoded_path_attribute() {
    let dir = fixture();
    std::fs::write(dir.path().join("a b.md"), "# Space\n").unwrap();
    let res = get(app(dir.path()), "/a%20b.md?fragment=1").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        body.contains(r#"data-native-path-encoded="a%20b.md""#),
        "body: {body}"
    );
    assert!(
        !body.contains(r#"data-native-path-encoded="a b.md""#),
        "body: {body}"
    );
}

/// 搜索: 大小写不敏感子串匹配 (目录也参与), 空白查询直接提示页不遍历.
#[tokio::test]
async fn search_matches_case_insensitive_and_directories() {
    let dir = fixture();
    std::fs::write(dir.path().join("AlphaOne.txt"), "1").unwrap();
    std::fs::create_dir(dir.path().join("Notes")).unwrap();
    std::fs::write(dir.path().join("Notes/inner.txt"), "2").unwrap();
    std::fs::write(dir.path().join("beta.txt"), "3").unwrap();

    for q in ["ALPHAONE", "alphaone"] {
        let res = get(app(dir.path()), &format!("/__/search?q={q}")).await;
        assert_eq!(res.status(), StatusCode::OK, "q={q}");
        let body = body_string(res).await;
        assert!(body.contains(r#"href="/AlphaOne.txt""#), "q={q}: {body}");
        assert!(!body.contains("beta.txt"), "q={q}: {body}");
    }
    // 目录也参与匹配
    let res = get(app(dir.path()), "/__/search?q=notes").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains(r#"href="/Notes""#), "body: {body}");

    // 空/纯空白查询: 直接提示页, 不遍历
    for q in ["", "%20%20"] {
        let res = get(app(dir.path()), &format!("/__/search?q={q}")).await;
        assert_eq!(res.status(), StatusCode::OK, "q={q}");
        let body = body_string(res).await;
        assert!(body.contains("没有找到匹配的文件。"), "q={q}: {body}");
    }
}

/// 搜索结果顺序: BFS (深度升序, 同目录按字节序) — /a.txt, /b.txt, /sub/b.txt.
#[tokio::test]
async fn search_results_are_in_deterministic_bfs_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/b.txt"), "bb").unwrap();

    let res = get(app(dir.path()), "/__/search?q=txt").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    let a = body.find(r#"href="/a.txt""#).expect("a.txt in results");
    let b = body.find(r#"href="/b.txt""#).expect("b.txt in results");
    let sub = body
        .find(r#"href="/sub/b.txt""#)
        .expect("sub/b.txt in results");
    assert!(a < b && b < sub, "BFS order violated: {body}");
}

/// 超过 500 条匹配截断并提示.
#[tokio::test]
async fn search_truncates_at_500_results() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..510 {
        std::fs::write(dir.path().join(format!("match_{i:03}.txt")), "x").unwrap();
    }
    let res = get(app(dir.path()), "/__/search?q=match").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains("已截断显示前 500 条。"), "body: {body}");
}

/// 查询字符串经 maud 转义, 不注入 HTML.
#[tokio::test]
async fn search_query_is_escaped_in_html() {
    let dir = fixture();
    let res = get(app(dir.path()), "/__/search?q=%3Cscript%3E").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains(r#"value="&lt;script&gt;""#), "body: {body}");
    assert!(!body.contains(r#"value="<script>""#), "body: {body}");
}

/// 浏览器 JS 与文档 CSS 静态资源 (目录页/搜索页 head 引用) 可达.
#[tokio::test]
async fn browser_assets_served() {
    let dir = fixture();
    let res = get(app(dir.path()), "/__/static/css/chapbook-doc.css").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/css");
    assert!(body_string(res).await.contains("chapbook-doc-style"));

    let res = get(app(dir.path()), "/__/static/js/chapbook-browser.js").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "application/javascript");
    assert!(body_string(res).await.contains("actionUrl"));
}

// ---------- 验收审计补测 (2026-08-13) ----------

/// 测试端 RFC 3986 path segment 编码, 与 meta 的 PATH_SEGMENT_ENCODE_SET 同一规则:
/// 仅 unreserved (A-Za-z0-9-._~) 保留, 其余逐 UTF-8 字节 %XX 大写.
/// (percent_encoding 对非 ASCII 恒编码; 本测试 fixture 名不含 `!$()*`,
/// 故与服务器输出逐字节一致.)
fn t_enc_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 测试端 application/x-www-form-urlencoded (query 参数): 保留字母数字与 -._~,
/// 空格 -> `+`, 其余 %XX.
fn t_form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 显示 codec 对单个危险 scalar 的 `\u{NNNN}` 转义 (大写 hex, 至少 4 位).
fn t_u_escape(c: char) -> String {
    format!("\\u{{{:04X}}}", c as u32)
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// 搜索页结果行提取: 每行取第一个 href (名称锚点), 得到按 BFS 顺序的结果路径序列.
/// display-only 行 (无锚点) 被跳过.
fn search_result_hrefs(body: &str) -> Vec<String> {
    body.split(r#"<tr class="cb-row""#)
        .skip(1)
        .filter_map(|chunk| {
            let start = chunk.find(r#"href="/"#)?;
            let rest = &chunk[start + 6..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

/// Hostile org: Full 与 ?fragment=1 共用同一转义管线.
/// ExportBlock/Snippet 只输出转义文本 (无活动元素/事件属性); 危险 scheme 表
/// (混合大小写 / 前导内嵌 ASCII whitespace+control / vbscript / data / file)
/// 一律 href=""; http/https/mailto/相对路径/#fragment 保持可点击且转义.
#[tokio::test]
async fn org_hostile_fragment_and_scheme_table_escaped() {
    let dir = fixture();
    let hostile = "\
* Evil

#+begin_export html
<script>alert(1)</script><img src=x onerror=alert(2)>
#+end_export

@@html:<b onclick=alert(3)>x</b>@@

[[javascript:alert(4)]]
[[JaVaScRiPt:alert(5)]]
[[java script:alert(6)]]
[[java\tscript:alert(7)]]
[[java\u{1}script:alert(8)]]
[[vbscript:msgbox(1)]]
[[data:text/html;base64,PHNjcmlwdD4=]]
[[file:img.png]]
[[https://example.com/doc.pdf]]
[[http://example.com/x?a=1&b=2]]
[[mailto:user@example.com]]
[[relative/path.txt]]
[[#fragment]]
";
    std::fs::write(dir.path().join("hostile.org"), hostile).unwrap();

    for uri in ["/hostile.org", "/hostile.org?fragment=1"] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert_eq!(content_type(&res), "text/html; charset=utf-8", "{uri}");
        let body = body_string(res).await;

        // ExportBlock/Snippet 只输出转义文本: 无任何活动元素标签/危险 href
        for active in [
            "<script",
            "<img",
            "<b ",
            "<a href=\"javascript:",
            "<a href=\"vbscript:",
            "<a href=\"data:",
            "<a href=\"file:",
        ] {
            assert!(!body.contains(active), "{uri}: active {active:?}: {body}");
        }
        assert!(
            body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{uri}: {body}"
        );
        assert!(
            body.contains("&lt;img src=x onerror=alert(2)&gt;"),
            "{uri}: {body}"
        );
        assert!(
            body.contains("&lt;b onclick=alert(3)&gt;x&lt;/b&gt;"),
            "{uri}: {body}"
        );

        // 危险 scheme 表: href 为空, 标签保留
        for url in [
            "javascript:alert(4)",
            "JaVaScRiPt:alert(5)",
            "java script:alert(6)",
            "java\tscript:alert(7)",
            "java\u{1}script:alert(8)",
            "vbscript:msgbox(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "file:img.png",
        ] {
            assert!(
                body.contains(&format!(r#"<a href="">{url}</a>"#)),
                "{uri}: dangerous {url:?}: {body}"
            );
        }

        // 安全 scheme: 有效且经 HTML 转义的 href
        for url in [
            "https://example.com/doc.pdf",
            "http://example.com/x?a=1&b=2",
            "mailto:user@example.com",
            "relative/path.txt",
            "#fragment",
        ] {
            let escaped = url.replace('&', "&amp;");
            assert!(
                body.contains(&format!(r#"<a href="{escaped}">{escaped}</a>"#)),
                "{uri}: safe {url:?}: {body}"
            );
        }
    }
}

/// 六个危险 fixture (U+202E / U+200B / U+00A0 / U+2028 / 双 U+0020 / 尾随 U+0020)
/// 在各输出使用同一安全转义 label:
/// 目录完整页 (title/隐藏 heading/表格/网格/breadcrumb), 搜索页 (名称/位置/aria/title),
/// 目录 fragment (wrapper + 迷你列表), 文件预览 fragment title,
/// 以及 md/org/CSV/code 完整页无显式 title 时的 filename fallback.
#[tokio::test]
async fn unsafe_scalar_fixtures_safe_labels_across_outlets() {
    struct Case {
        raw: &'static str,
        label: &'static str,
        files: &'static [&'static str],
    }
    let cases: &[Case] = &[
        Case {
            raw: "\u{202E}",
            label: "\\u{202E}",
            files: &["\u{202E}.md", "\u{202E}.org", "\u{202E}.csv", "\u{202E}.rs"],
        },
        Case {
            raw: "\u{200B}",
            label: "\\u{200B}",
            files: &["\u{200B}.md", "\u{200B}.org", "\u{200B}.csv", "\u{200B}.rs"],
        },
        Case {
            raw: "\u{00A0}",
            label: "\\u{00A0}",
            files: &["\u{00A0}.md", "\u{00A0}.org", "\u{00A0}.csv", "\u{00A0}.rs"],
        },
        Case {
            raw: "\u{2028}",
            label: "\\u{2028}",
            files: &["\u{2028}.md", "\u{2028}.org", "\u{2028}.csv", "\u{2028}.rs"],
        },
        Case {
            raw: "a  b",
            label: "a\\x20\\x20b",
            files: &["a  b.md", "a  b.org", "a  b.csv", "a  b.rs"],
        },
        Case {
            raw: "report.md ",
            label: "report.md\\x20",
            files: &[],
        },
    ];

    let dir = tempfile::tempdir().unwrap();
    for case in cases {
        let dpath = dir.path().join(case.raw);
        std::fs::create_dir(&dpath).unwrap();
        std::fs::write(dpath.join("inner.txt"), "inner").unwrap();
        for f in case.files {
            let content = match (*f).rsplit('.').next().unwrap_or_default() {
                "md" => "# T\nbody\n",
                "org" => "* T\nbody\n",
                "csv" => "name,age\nAlice,30\n",
                _ => "fn main() {}\n",
            };
            std::fs::write(dir.path().join(f), content).unwrap();
        }
    }

    let app = app(dir.path());
    for case in cases {
        let raw = case.raw;
        let label = case.label;
        let enc = t_enc_segment(raw);
        // 文件 label: 同一 codec 转义 = dir label + 扩展名 (scalar 只出现在 raw 中)
        let files: Vec<(String, String)> = case
            .files
            .iter()
            .map(|f| {
                let enc = t_enc_segment(f);
                let flabel = format!("{label}{}", &(*f)[raw.len()..]);
                (enc, flabel)
            })
            .collect();

        // 目录完整页: 表格 + 网格同一 bdi label; aria/title 属性即同一 label;
        // href 与 data-native-path-encoded 是同一 RFC 3986 编码 (仅前导斜杠不同).
        let body = body_string(get(app.clone(), "/").await).await;
        for l in std::iter::once(label.to_string()).chain(files.iter().map(|(_, l)| l.clone())) {
            assert!(
                count_occurrences(&body, &format!(r#"<bdi dir="auto">{l}</bdi>"#)) >= 2,
                "case {raw:?} label {l}: {body}"
            );
            assert!(
                body.contains(&format!(r#"aria-label="{l}""#)),
                "case {raw:?} label {l}: {body}"
            );
            assert!(
                body.contains(&format!(r#"title="{l}""#)),
                "case {raw:?} label {l}: {body}"
            );
        }
        assert!(
            body.contains(&format!(r#"href="/{enc}""#)),
            "case {raw:?}: {body}"
        );
        assert!(
            body.contains(&format!(r#"data-native-path-encoded="{enc}""#)),
            "case {raw:?}: {body}"
        );
        for (fenc, _) in &files {
            assert!(
                body.contains(&format!(r#"href="/{fenc}""#)),
                "case {raw:?} file {fenc}: {body}"
            );
            assert!(
                body.contains(&format!(r#"data-native-path-encoded="{fenc}""#)),
                "case {raw:?} file {fenc}: {body}"
            );
        }
        // 原始 scalar 不进入页面 (U+0020 fixture 本身含空格, 跳过)
        if !raw.contains(' ') {
            assert!(
                !body.contains(raw),
                "case {raw:?}: raw scalar leaked: {body}"
            );
        }

        // 目录完整页: <title> / 辅助技术可读的隐藏 heading / breadcrumb 同一 label;
        // 当前路径只由 breadcrumb 可见显示, 不再重复 Index of 行.
        let dbody = body_string(get(app.clone(), &format!("/{enc}")).await).await;
        assert!(
            dbody.contains(&format!("<title>/{label}</title>")),
            "case {raw:?}: {dbody}"
        );
        assert!(
            dbody.contains(&format!(
                r#"<h1 class="cb-visually-hidden">目录 /{label} 中的文件</h1>"#
            )),
            "case {raw:?}: {dbody}"
        );
        assert!(!dbody.contains("Index of"), "case {raw:?}: {dbody}");
        assert!(
            dbody.contains(&format!(r#"<bdi dir="auto">{label}</bdi>"#)),
            "case {raw:?} breadcrumb: {dbody}"
        );

        // 搜索页: 名称/aria/title 同一 label; 位置列 (inner.txt 的父目录段) 同一 label
        let q = t_form_encode(label);
        let sbody = body_string(get(app.clone(), &format!("/__/search?q={q}")).await).await;
        assert!(
            sbody.contains(&format!(r#"aria-label="{label}""#)),
            "case {raw:?}: {sbody}"
        );
        assert!(
            sbody.contains(&format!(r#"title="{label}""#)),
            "case {raw:?}: {sbody}"
        );
        assert!(
            sbody.contains(&format!(r#"<bdi dir="auto">{label}</bdi>"#)),
            "case {raw:?}: {sbody}"
        );
        for (_, flabel) in &files {
            assert!(
                sbody.contains(&format!(r#"aria-label="{flabel}""#)),
                "case {raw:?} file {flabel}: {sbody}"
            );
        }
        let ibody = body_string(get(app.clone(), "/__/search?q=inner").await).await;
        assert!(
            ibody.contains(&format!(
                r#"href="/{enc}"><bdi dir="auto">{label}</bdi></a>"#
            )),
            "case {raw:?} location: {ibody}"
        );

        // 目录 fragment: wrapper 携带同一 encoded identity + title; 根 fragment 迷你列表同 label
        let frag_body = body_string(get(app.clone(), &format!("/{enc}?fragment=1")).await).await;
        assert!(
            frag_body.contains(&format!(
                r#"data-native-path-encoded="{enc}" title="{label}""#
            )),
            "case {raw:?}: {frag_body}"
        );
        let root_frag = body_string(get(app.clone(), "/?fragment=1").await).await;
        assert!(
            root_frag.contains(&format!(
                r#"href="/{enc}" data-native-path-encoded="{enc}""#
            )),
            "case {raw:?}: {root_frag}"
        );
        assert!(
            root_frag.contains(&format!(r#"<bdi dir="auto">{label}</bdi>"#)),
            "case {raw:?}: {root_frag}"
        );

        // 文件预览 fragment title / 完整页 filename fallback (md/org/CSV/code)
        if let Some((fenc, flabel)) = files.first() {
            let pbody = body_string(get(app.clone(), &format!("/{fenc}?fragment=1")).await).await;
            assert!(
                pbody.contains(&format!(
                    r#"data-native-path-encoded="{fenc}" title="{flabel}""#
                )),
                "case {raw:?}: {pbody}"
            );
            for (fenc, flabel) in &files {
                let res = if (*fenc).ends_with(".csv") || (*fenc).ends_with(".rs") {
                    get_with_accept(app.clone(), &format!("/{fenc}"), "text/html").await
                } else {
                    get(app.clone(), &format!("/{fenc}")).await
                };
                assert_eq!(res.status(), StatusCode::OK, "case {raw:?} {fenc}");
                let full = body_string(res).await;
                assert!(
                    full.contains(&format!("<title>{flabel}</title>")),
                    "case {raw:?} {fenc}: {full}"
                );
            }
        } else {
            // 尾随空格 fixture 无扩展名: 占位 fragment wrapper 仍显示同一 label
            let pbody = body_string(get(app.clone(), &format!("/{enc}?fragment=1")).await).await;
            assert!(
                pbody.contains(&format!(
                    r#"data-native-path-encoded="{enc}" title="{label}""#
                )),
                "case {raw:?}: {pbody}"
            );
        }
    }
}

/// photo<U+202E>gnp.command 与 U+034F / U+2060-U+2064 / U+FE0F:
/// 目录页显示转义 label, 原始 scalar 不泄漏, 行仍可操作 (href/dataset/actions).
#[tokio::test]
async fn default_ignorable_fixtures_escaped_and_actionable() {
    let names = [
        "photo\u{202E}gnp.command",
        "a\u{034F}b.txt",
        "a\u{2060}b.txt",
        "a\u{2061}b.txt",
        "a\u{2062}b.txt",
        "a\u{2063}b.txt",
        "a\u{2064}b.txt",
        "a\u{FE0F}b.txt",
    ];
    let dir = tempfile::tempdir().unwrap();
    for n in names {
        std::fs::write(dir.path().join(n), "x").unwrap();
    }
    let app = app(dir.path());
    let body = body_string(get(app.clone(), "/").await).await;
    for n in names {
        let label: String = n
            .chars()
            .map(|c| {
                if ('\u{2060}'..='\u{2064}').contains(&c)
                    || matches!(c, '\u{202E}' | '\u{034F}' | '\u{FE0F}')
                {
                    t_u_escape(c)
                } else {
                    c.to_string()
                }
            })
            .collect();
        let enc = t_enc_segment(n);
        assert!(
            count_occurrences(&body, &format!(r#"<bdi dir="auto">{label}</bdi>"#)) >= 2,
            "{n:?}: {body}"
        );
        assert!(!body.contains(n), "raw scalar leaked for {n:?}: {body}");
        assert!(body.contains(&format!(r#"href="/{enc}""#)), "{n:?}: {body}");
        assert!(
            body.contains(&format!(r#"data-native-path-encoded="{enc}""#)),
            "{n:?}: {body}"
        );
        assert!(
            body.contains(r#"data-cb-action="preview""#),
            "{n:?}: actions must remain present: {body}"
        );
    }
    // UTF-8 路径 action 保留: photo<U+202E>gnp.command 仍可请求
    let res = get(app, "/photo%E2%80%AEgnp.command").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_string(res).await, "x");
}

/// 搜索页: escaped label 查询 (`\xFF` 字面) 能命中非 UTF-8 basename,
/// 但该行 display-only — 无 anchor / data-native-path-encoded / 行尾 action 控制.
#[cfg(unix)]
#[tokio::test]
async fn search_escaped_label_query_finds_raw_byte_name_display_only() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(OsString::from_vec(b"\xFFbad.txt".to_vec())),
        b"x",
    )
    .unwrap();
    std::fs::write(dir.path().join("good.txt"), "g").unwrap();

    let res = get(app(dir.path()), "/__/search?q=%5CxFF").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;

    assert!(
        body.contains(r#"<bdi dir="auto">\xFFbad.txt</bdi>"#),
        "body: {body}"
    );
    let chunk = body
        .split(r#"<tr class="cb-row""#)
        .find(|c| c.contains(r"\xFFbad.txt"))
        .expect("row for raw byte name")
        .split("</tr>")
        .next()
        .unwrap()
        .to_string();
    assert!(
        chunk.contains("cb-non-utf8"),
        "row must be display-only: {chunk}"
    );
    // 无指向文件自身的 anchor (位置列对顶层条目渲染通用根链接 href="/", 属导航 UI)
    assert!(
        !chunk.contains(r#"href="/\xFFbad.txt"#),
        "no anchor for non-UTF-8 row: {chunk}"
    );
    assert!(
        !chunk.contains("data-native-path-encoded"),
        "no dataset for non-UTF-8 row: {chunk}"
    );
    assert!(
        !chunk.contains("data-cb-action"),
        "no action controls for non-UTF-8 row: {chunk}"
    );
    assert!(
        !chunk.contains("cb-row-actions"),
        "no action cluster for non-UTF-8 row: {chunk}"
    );
    assert!(
        chunk.contains(r#"aria-label="\xFFbad.txt""#),
        "title/aria still carries escaped label: {chunk}"
    );

    // UTF-8 行不受影响: 普通文件行完整可操作 (同一搜索页机制)
    let gbody = body_string(get(app(dir.path()), "/__/search?q=good").await).await;
    assert!(
        gbody.contains(r#"data-native-path-encoded="good.txt""#),
        "gbody: {gbody}"
    );
    assert!(gbody.contains(r#"href="/good.txt""#), "gbody: {gbody}");
    assert!(
        gbody.contains(r#"data-cb-action="preview""#),
        "gbody: {gbody}"
    );
}

/// 六个 ASCII-space fixture (单内部 / 双内部 / 前导 / 无 / 尾随空格 / 字面 \x20)
/// 的 DOM label 两两可区分: 目录表格+网格、搜索名称/aria、目录 fragment、
/// href 编码与文件请求.
#[tokio::test]
async fn ascii_space_six_fixtures_distinct_across_outlets() {
    let cases: &[(&str, &str)] = &[
        ("a b.txt", "a b.txt"),
        ("a  b.txt", "a\\x20\\x20b.txt"),
        (" report.md", "\\x20report.md"),
        ("report.md", "report.md"),
        ("report.md ", "report.md\\x20"),
        (r"report.md\x20", r"report.md\\x20"),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (name, _) in cases {
        std::fs::write(dir.path().join(name), "x").unwrap();
    }
    let app = app(dir.path());

    // 六个 label 两两不同
    let distinct: std::collections::HashSet<&str> = cases.iter().map(|(_, l)| *l).collect();
    assert_eq!(distinct.len(), 6, "labels must be pairwise distinct");

    let body = body_string(get(app.clone(), "/").await).await;
    for (name, label) in cases {
        let enc = t_enc_segment(name);
        assert!(
            count_occurrences(&body, &format!(r#"<bdi dir="auto">{label}</bdi>"#)) >= 2,
            "{name:?}: {body}"
        );
        assert!(
            body.contains(&format!(r#"aria-label="{label}""#)),
            "{name:?}: {body}"
        );
        assert!(
            body.contains(&format!(r#"href="/{enc}""#)),
            "{name:?}: {body}"
        );
        assert!(
            body.contains(&format!(r#"data-native-path-encoded="{enc}""#)),
            "{name:?}: {body}"
        );
    }

    // 搜索: report 系 4 行 + a 系 2 行, label 各自可区分
    let sbody = body_string(get(app.clone(), "/__/search?q=report").await).await;
    for (_, label) in cases.iter().filter(|(n, _)| n.contains("report")) {
        assert!(
            sbody.contains(&format!(r#"aria-label="{label}""#)),
            "report search: {label}: {sbody}"
        );
    }
    let abody = body_string(get(app.clone(), "/__/search?q=a").await).await;
    for (_, label) in cases.iter().filter(|(n, _)| n.contains('a')) {
        assert!(
            abody.contains(&format!(r#"aria-label="{label}""#)),
            "a search: {label}: {abody}"
        );
    }

    // 目录 fragment 迷你列表: 同一 label + 同一编码
    let fbody = body_string(get(app.clone(), "/?fragment=1").await).await;
    for (name, label) in cases {
        let enc = t_enc_segment(name);
        assert!(
            fbody.contains(&format!(
                r#"href="/{enc}" data-native-path-encoded="{enc}""#
            )),
            "{name:?}: {fbody}"
        );
        assert!(
            fbody.contains(&format!(r#"<bdi dir="auto">{label}</bdi>"#)),
            "{name:?}: {fbody}"
        );
    }

    // 各 fixture 可经 encoded 路径请求到原始字节
    for (name, _) in cases {
        let enc = t_enc_segment(name);
        let res = get(app.clone(), &format!("/{enc}?raw=1")).await;
        assert_eq!(res.status(), StatusCode::OK, "{name:?}");
        assert_eq!(body_string(res).await, "x", "{name:?}");
    }
}

/// 24 个非 U+0020 White_Space scalar: 目录行 href 与 data-native-path-encoded
/// 使用同一 RFC 3986 编码字节 (仅前导斜杠不同), 行可操作, 编码路径可精确请求.
/// (U+0009-U+000D 控制符不能在 Windows 文件名中出现, Unix-only.)
#[cfg(unix)]
#[tokio::test]
async fn whitespace_scalars_encoded_identity_and_actionable() {
    const WS: &[char] = &[
        '\u{0009}', '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0085}', '\u{00A0}',
        '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}',
        '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{2028}', '\u{2029}',
        '\u{202F}', '\u{205F}', '\u{3000}',
    ];
    assert_eq!(WS.len(), 24);
    let dir = tempfile::tempdir().unwrap();
    for (i, c) in WS.iter().enumerate() {
        let name = format!("f{i}-{c}.txt");
        std::fs::write(dir.path().join(&name), "ws").unwrap();
    }
    let body = body_string(get(app(dir.path()), "/").await).await;
    for (i, c) in WS.iter().enumerate() {
        let name = format!("f{i}-{c}.txt");
        let label = format!("f{i}-{}.txt", t_u_escape(*c));
        let enc = t_enc_segment(&name);
        assert!(
            body.contains(&format!(r#"href="/{enc}""#)),
            "{c:?} ({enc}): {body}"
        );
        assert!(
            body.contains(&format!(r#"data-native-path-encoded="{enc}""#)),
            "{c:?} ({enc}): {body}"
        );
        // 行内 label 精确转义且保持可操作
        let chunk = body
            .split(r#"<tr class="cb-row""#)
            .find(|ch| ch.contains(&format!(r#"<bdi dir="auto">{label}</bdi>"#)))
            .unwrap_or_else(|| panic!("row for {c:?} ({enc}) not found: {body}"));
        assert!(
            chunk.contains(&format!(r#"href="/{enc}""#)),
            "{c:?}: {chunk}"
        );
        assert!(
            chunk.contains(&format!(r#"data-native-path-encoded="{enc}""#)),
            "{c:?}: {chunk}"
        );
        assert!(
            chunk.contains(r#"data-cb-action="preview""#),
            "{c:?}: {chunk}"
        );
        // 原始 scalar 不进入 label (ASCII 控制符可能作为 HTML 空白存在, 只查 >= 0x80)
        if *c as u32 >= 0x80 {
            assert!(!body.contains(*c), "{c:?} raw scalar leaked: {body}");
        }
        // 编码路径可精确请求
        let res = get(app(dir.path()), &format!("/{enc}")).await;
        assert_eq!(res.status(), StatusCode::OK, "{c:?} ({enc})");
        assert_eq!(body_string(res).await, "ws", "{c:?}");
    }
}

/// >500 匹配: 跨乱序创建的多个子目录, 两次请求返回完全相同的前 500 个 href 序列
/// (确定性 BFS: 深度升序, 同目录按原始文件名字节序), truncated=true.
#[tokio::test]
async fn search_many_matches_deterministic_order_and_truncation() {
    const DIRS: usize = 13;
    const PER_DIR: usize = 40; // 13*40 = 520 > 500
    let dir = tempfile::tempdir().unwrap();
    // 固定 LCG 伪随机: 模拟随机创建顺序 (不引入 rand 依赖)
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        state
    };
    let mut order: Vec<(usize, usize)> = Vec::new();
    for d in 0..DIRS {
        for f in 0..PER_DIR {
            order.push((d, f));
        }
    }
    for i in (1..order.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    for (d, f) in order {
        let dpath = dir.path().join(format!("d{d:02}"));
        std::fs::create_dir_all(&dpath).unwrap();
        std::fs::write(dpath.join(format!("match_{f:03}.txt")), "x").unwrap();
    }

    // 服务器只返回前 500 条 (第 501 条起截断)
    let expected: Vec<String> = (0..DIRS)
        .flat_map(|d| (0..PER_DIR).map(move |f| format!("/d{d:02}/match_{f:03}.txt")))
        .take(500)
        .collect();

    let app = app(dir.path());
    let b1 = body_string(get(app.clone(), "/__/search?q=match").await).await;
    let b2 = body_string(get(app.clone(), "/__/search?q=match").await).await;
    let seq1 = search_result_hrefs(&b1);
    let seq2 = search_result_hrefs(&b2);
    assert_eq!(
        seq1, seq2,
        "two identical requests must produce identical href sequence"
    );
    assert_eq!(
        seq1, expected,
        "results must be BFS order (depth, then bytewise name)"
    );
    assert_eq!(seq1.len(), 500, "must be truncated at exactly 500");
    assert!(b1.contains("已截断显示前 500 条"), "b1: {b1}");
    assert!(b2.contains("已截断显示前 500 条"), "b2: {b2}");
}

/// 恰好 500 条匹配: 不截断, 全部返回且顺序确定.
#[tokio::test]
async fn search_exactly_500_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..500 {
        std::fs::write(dir.path().join(format!("match_{i:03}.txt")), "x").unwrap();
    }
    let res = get(app(dir.path()), "/__/search?q=match").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(
        !body.contains("已截断"),
        "exactly 500 must not truncate: {body}"
    );
    let expected: Vec<String> = (0..500).map(|i| format!("/match_{i:03}.txt")).collect();
    assert_eq!(search_result_hrefs(&body), expected);
}

/// .txt/.log: ?raw=1 恒返回原始字节 (即使浏览器 Accept); ?view=1 恒渲染代码页;
/// 默认脚本 Accept 仍原文 (既有行为保留); 非 UTF-8 .txt raw 透传裸字节.
#[tokio::test]
async fn txt_log_raw_returns_original_bytes() {
    let dir = fixture();
    let txt = "hello <world> & bye\nsecond line\n";
    let log = "2026-08-13 10:00:00 INFO started\n2026-08-13 10:00:01 INFO done\n";
    std::fs::write(dir.path().join("note.txt"), txt).unwrap();
    std::fs::write(dir.path().join("app.log"), log).unwrap();

    for (file, content) in [("note.txt", txt), ("app.log", log)] {
        // raw=1 覆盖浏览器 Accept
        let res = get_with_accept(app(dir.path()), &format!("/{file}?raw=1"), "text/html").await;
        assert_eq!(res.status(), StatusCode::OK, "{file}");
        assert_eq!(content_type(&res), "text/plain; charset=utf-8", "{file}");
        assert_eq!(
            body_string(res).await,
            content,
            "{file}: raw=1 must return original bytes"
        );

        // view=1 覆盖脚本 Accept
        let res = get_with_accept(app(dir.path()), &format!("/{file}?view=1"), "*/*").await;
        assert_eq!(res.status(), StatusCode::OK, "{file}");
        assert_eq!(content_type(&res), "text/html; charset=utf-8", "{file}");
        let body = body_string(res).await;
        assert!(
            body.contains(r#"<pre class="sourceCode"><code>"#),
            "{file}: {body}"
        );
        assert!(
            body.contains(r#"<span class="ln">1</span>"#),
            "{file}: {body}"
        );
        assert!(!body.contains("<world>"), "{file}: must be escaped: {body}");

        // 默认 + 脚本 Accept: 原文 (既有行为保留)
        let res = get_with_accept(app(dir.path()), &format!("/{file}"), "*/*").await;
        assert_eq!(res.status(), StatusCode::OK, "{file}");
        assert_eq!(body_string(res).await, content, "{file}");
    }

    // 非 UTF-8 .txt: raw=1 裸字节透传
    let gbk: &[u8] = b"line \xd6\xd0\xce\xc4\n";
    std::fs::write(dir.path().join("legacy.txt"), gbk).unwrap();
    let res = get_with_accept(app(dir.path()), "/legacy.txt?raw=1", "text/html").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(content_type(&res), "text/plain; charset=utf-8");
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.as_ref(), gbk);
}

/// fragment 与静态资源响应明确不带 frame-ancestors / X-Frame-Options;
/// 目录页与搜索页保留这两个 anti-framing 头.
#[tokio::test]
async fn fragment_and_static_assets_lack_anti_frame_headers() {
    let dir = fixture();
    std::fs::write(dir.path().join("doc.md"), "# T\nbody\n").unwrap();
    std::fs::write(dir.path().join("doc.org"), "* T\nbody\n").unwrap();
    std::fs::write(dir.path().join("big.rs"), "x".repeat(1024 * 1024 + 1)).unwrap();
    std::fs::write(dir.path().join("pic.png"), b"PNG").unwrap();

    for uri in [
        "/doc.md?fragment=1",
        "/doc.org?fragment=1",
        "/?fragment=1",
        "/pic.png?fragment=1",
        "/big.rs?fragment=1",
        "/__/static/css/chapbook-theme.css",
        "/__/static/css/chapbook-doc.css",
        "/__/static/js/chapbook-browser.js",
        "/__/static/katex/katex.min.css",
        "/__/static/katex/fonts/KaTeX_Main-Regular.woff2",
    ] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(
            res.headers().get(header::CONTENT_SECURITY_POLICY).is_none(),
            "{uri}: fragment/static must not carry CSP"
        );
        assert!(
            res.headers().get(header::X_FRAME_OPTIONS).is_none(),
            "{uri}: fragment/static must not carry X-Frame-Options"
        );
    }

    // 目录/搜索页保留 anti-framing 头
    for uri in ["/", "/__/search?q=1.txt"] {
        let res = get(app(dir.path()), uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert_eq!(
            res.headers().get(header::CONTENT_SECURITY_POLICY).unwrap(),
            "frame-ancestors 'none'",
            "{uri}"
        );
        assert_eq!(
            res.headers().get(header::X_FRAME_OPTIONS).unwrap(),
            "DENY",
            "{uri}"
        );
    }
}
