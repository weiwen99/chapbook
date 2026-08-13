//! Org-mode 文档渲染 (orgize 0.9, 纯内存 AST, 无子进程、无外部依赖).
//!
//! 管线: `Org::parse` (不会失败) → 预扫描 AST 收集标题/TOC/元数据 → 自定义
//! `HtmlHandler` 渲染 body fragment → 由 `render::doc_page` 自组完整页面.
//!
//! 与旧 pandoc/emacs 管线的差异 (Phase 1/2, 见
//! docs/2026-08-05-proposal-remove-dependency-of-pandoc.org):
//! - 代码块经 syntect 高亮 (与 md/代码文件共用 highlight 模块与调色板)
//! - 标题带自建锚点 `<a id= href=>`, TOC 由同一 slug 函数生成 (render::slugify)
//! - 4 级标题完整保留 (pandoc/emacs 会结构性丢失)

use std::collections::HashSet;
use std::io::{Error as IoError, Write as IoWrite};

use orgize::export::{DefaultHtmlHandler, HtmlEscape, HtmlHandler};
use orgize::{Element, Event, Org};

use crate::{highlight, render};

/// 预扫描得到的文档元信息 (TOC 与页面骨架渲染用).
#[derive(Default)]
struct DocMeta {
    title: Option<String>,
    author: Option<String>,
    date: Option<String>,
    /// 预渲染的 `<nav id="TOC">…</nav>`; 未启用或没有标题时为空串
    toc_html: String,
    /// 正文标题锚点 slug, 与 TOC 同一顺序、同一 slug 函数生成
    slugs: Vec<String>,
}

/// 渲染 .org 源文本为 body fragment, 返回 (页面标题, body HTML).
/// 标题取 `#+TITLE`; 缺失时回退文件名 (由调用方传入).
pub fn render(source: &str, file_name: &str) -> (String, String) {
    let org = Org::parse(source);
    let meta = collect_meta(&org);
    let title = meta.title.clone().unwrap_or_else(|| file_name.to_string());
    let mut body = Vec::new();
    let mut handler = OrgHtmlHandler::new(meta);
    // 写入 Vec 不会失败, 解析也不会失败, 此 expect 实际不可达
    org.write_html_custom(&mut body, &mut handler)
        .expect("orgize render to Vec cannot fail");
    (
        title,
        String::from_utf8(body).expect("orgize output is valid utf-8"),
    )
}

/// 预扫描: 收集元信息 keyword 与标题列表, 生成 slug 与 TOC HTML.
fn collect_meta(org: &Org) -> DocMeta {
    let mut title = None;
    let mut author = None;
    let mut date = None;
    let mut toc_enabled = true;
    let mut toc_depth = None;
    let mut raw_titles: Vec<(usize, String)> = Vec::new();

    for event in org.iter() {
        let Event::Start(element) = event else {
            continue;
        };
        match element {
            Element::Keyword(kw) => {
                if kw.key.eq_ignore_ascii_case("TITLE") && title.is_none() {
                    title = Some(kw.value.to_string());
                } else if kw.key.eq_ignore_ascii_case("AUTHOR") && author.is_none() {
                    author = Some(kw.value.to_string());
                } else if kw.key.eq_ignore_ascii_case("DATE") && date.is_none() {
                    date = Some(kw.value.to_string());
                } else if kw.key.eq_ignore_ascii_case("OPTIONS") {
                    parse_toc_option(&kw.value, &mut toc_enabled, &mut toc_depth);
                }
            }
            Element::Title(t) => raw_titles.push((t.level, t.raw.to_string())),
            _ => {}
        }
    }

    // 同一 slug 函数 (render::slugify): TOC 链接与正文锚点必须一一对应 (测试锁定)
    let mut used = HashSet::new();
    let entries: Vec<render::TocEntry> = raw_titles
        .into_iter()
        .map(|(level, text)| {
            let slug = render::slugify(&text, &mut used);
            render::TocEntry { level, text, slug }
        })
        .collect();
    let slugs = entries.iter().map(|e| e.slug.clone()).collect();
    let toc_html = if toc_enabled && !entries.is_empty() {
        render::toc_html(&entries, toc_depth)
    } else {
        String::new()
    };

    DocMeta {
        title,
        author,
        date,
        toc_html,
        slugs,
    }
}

/// 解析 `#+OPTIONS: toc:...` 值 (空格分隔的 key:value 列表, 只关心 toc):
/// toc:nil -> 关闭; toc:N -> 限深度 N; 其余/缺省 -> 开启不限深度.
fn parse_toc_option(value: &str, enabled: &mut bool, depth: &mut Option<usize>) {
    for token in value.split_whitespace() {
        let Some(rest) = token.strip_prefix("toc:") else {
            continue;
        };
        let rest = rest.trim_matches(|c| c == '"' || c == '\'');
        match rest {
            "nil" | "no" | "none" | "off" | "false" => *enabled = false,
            "" | "t" | "yes" | "on" | "true" => {}
            digits if digits.chars().all(|c| c.is_ascii_digit()) => {
                *enabled = true;
                *depth = digits.parse().ok();
            }
            _ => {}
        }
    }
}

/// 自定义 HtmlHandler: 页面骨架元素 (title-block-header / TOC / main 包裹)
/// 与标题锚点; 其余元素全部交给 DefaultHtmlHandler.
struct OrgHtmlHandler {
    inner: DefaultHtmlHandler,
    meta: DocMeta,
    heading_index: usize,
}

impl OrgHtmlHandler {
    fn new(meta: DocMeta) -> Self {
        OrgHtmlHandler {
            inner: DefaultHtmlHandler,
            meta,
            heading_index: 0,
        }
    }
}

impl Default for OrgHtmlHandler {
    fn default() -> Self {
        OrgHtmlHandler::new(DocMeta::default())
    }
}

impl HtmlHandler<IoError> for OrgHtmlHandler {
    fn start<W: IoWrite>(&mut self, mut w: W, element: &Element) -> Result<(), IoError> {
        match element {
            Element::Document { .. } => {
                // pandoc 行为对齐: title-block-header 在 TOC 之前, 正文在 <main> 内
                if let Some(title) = &self.meta.title {
                    write!(
                        w,
                        "<header id=\"title-block-header\"><h1 class=\"title\">{}</h1>",
                        HtmlEscape(title)
                    )?;
                    if let Some(author) = &self.meta.author {
                        write!(w, "<p class=\"author\">{}</p>", HtmlEscape(author))?;
                    }
                    if let Some(date) = &self.meta.date {
                        write!(w, "<p class=\"date\">{}</p>", HtmlEscape(date))?;
                    }
                    write!(w, "</header>")?;
                }
                write!(w, "{}", self.meta.toc_html)?;
                write!(w, "<main>")?;
            }
            // 裸图片链接 → `<img>` (org-mode 行为: `[[file:img.png]]` 无描述即内嵌图片);
            // 有描述的链接保持 `<a>` (如 `[[file:x.png][说明]]`, 用户可能想引用而非展示).
            // 所有链接先过 URL 安全校验: 危险协议 (javascript/vbscript/data/file) 的链接
            // 保留锚点标签但 href 置空, 危险协议的裸图片不得升级为 `<img>`.
            // orgize 的 Link 是叶子元素 (desc 为纯文本, 无嵌套行内标记事件), 整段
            // `<a>` 在 start 输出, end 无对应事件 — 与 DefaultHtmlHandler 结构一致.
            Element::Link(link) => {
                if !dangerous_org_url(&link.path)
                    && link.desc.is_none()
                    && is_image_path(&link.path)
                {
                    write!(
                        w,
                        "<img src=\"{}\" alt=\"{}\">",
                        HtmlEscape(&link.path),
                        HtmlEscape(&link.path)
                    )?;
                } else {
                    let href = if dangerous_org_url(&link.path) {
                        ""
                    } else {
                        link.path.as_ref()
                    };
                    let label = link.desc.as_ref().unwrap_or(&link.path);
                    write!(
                        w,
                        "<a href=\"{}\">{}</a>",
                        HtmlEscape(href),
                        HtmlEscape(label)
                    )?;
                }
            }
            // HTML export block 只输出转义文本, 绝不委托活动 HTML (默认 handler 会原样写出)
            Element::ExportBlock(block) => {
                if block.data.eq_ignore_ascii_case("HTML") {
                    write!(w, "{}", HtmlEscape(&block.contents))?;
                }
            }
            // HTML snippet (orgize `@@html:…@@`) 同样只输出转义文本
            Element::Snippet(snippet) => {
                if snippet.name.eq_ignore_ascii_case("HTML") {
                    write!(w, "{}", HtmlEscape(&snippet.value))?;
                }
            }
            Element::Title(title) => {
                let level = title.level.min(6);
                let slug = self
                    .meta
                    .slugs
                    .get(self.heading_index)
                    .map(String::as_str)
                    .unwrap_or_default();
                self.heading_index += 1;
                write!(w, "<h{level}><a id=\"{slug}\" href=\"#{slug}\">")?;
            }
            // 数学: orgize 不解析 LaTeX, `$..$`/`\(..\)`/`\begin{align}` 都是普通 Text.
            // 在 Text 元素上 tokenize (src/example/code/verbatim 是独立元素, 不经过这里),
            // 数学段服务端 KaTeX 渲染, 失败回退原文; 其余文本照常转义.
            Element::Text { value } => {
                write!(w, "{}", crate::math::org_text_html(value))?;
            }
            // src 块: syntect 高亮 (无语言时交回默认 handler 输出 `<pre class="example">`)
            Element::SourceBlock(block) if !block.language.is_empty() => {
                write!(
                    w,
                    "<div class=\"org-src-container\"><pre class=\"src src-{}\"><code>{}</code></pre></div>",
                    HtmlEscape(&block.language),
                    highlight::highlight(&block.contents, Some(&block.language), false)
                )?;
            }
            _ => self.inner.start(w, element)?,
        }
        Ok(())
    }

    fn end<W: IoWrite>(&mut self, mut w: W, element: &Element) -> Result<(), IoError> {
        match element {
            Element::Document { .. } => write!(w, "</main>")?,
            Element::Title(title) => {
                let level = title.level.min(6);
                write!(w, "</a></h{level}>")?;
            }
            _ => self.inner.end(w, element)?,
        }
        Ok(())
    }
}

/// 链接 URL 是否危险 (javascript / vbscript / data / file 协议).
/// 取首个 `:` 之前的候选协议名, 去掉所有 ASCII 空白/控制字符后 ASCII 小写比较;
/// 无 `:` 的相对路径与 `#fragment` 天然安全, 未知协议名保持允许.
fn dangerous_org_url(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let normalized: String = scheme
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_ascii_control())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    matches!(
        normalized.as_str(),
        "javascript" | "vbscript" | "data" | "file"
    )
}

/// 裸链接是否为图片路径 (按扩展名判定, 大小写不敏感; 忽略 query 部分).
fn is_image_path(path: &str) -> bool {
    const IMAGE_EXT: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "avif", "ico",
    ];
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let ext = path.rsplit('.').next().unwrap_or("");
    IMAGE_EXT.contains(&ext.to_ascii_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(source: &str) -> String {
        render(source, "test.org").1
    }

    /// HTML export block 只输出转义文本: 不得出现活动元素标签.
    #[test]
    fn html_export_block_outputs_only_escaped_text() {
        let body = body_of(
            "#+begin_export html\n<script>alert(1)</script><img src=x onerror=alert(2)>\n#+end_export\n",
        );
        assert!(!body.contains("<script"), "body: {body}");
        assert!(!body.contains("<img"), "body: {body}");
        assert!(
            body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "body: {body}"
        );
        assert!(
            body.contains("&lt;img src=x onerror=alert(2)&gt;"),
            "body: {body}"
        );
    }

    /// HTML snippet 只输出转义文本: 不得出现活动元素标签.
    #[test]
    fn html_snippet_outputs_only_escaped_text() {
        let body = body_of("@@html:<b onclick=alert(3)>x</b>@@");
        assert!(!body.contains("<b "), "body: {body}");
        assert!(
            body.contains("&lt;b onclick=alert(3)&gt;x&lt;/b&gt;"),
            "body: {body}"
        );
    }

    /// 非 HTML export block 保持默认行为 (不输出).
    #[test]
    fn non_html_export_stays_hidden() {
        let body = body_of("#+begin_export latex\n\\textbf{hi}\n#+end_export\n");
        assert!(!body.contains("\\textbf"), "body: {body}");
    }

    /// 危险协议 (大小写混合 / 内嵌 ASCII 空白或控制字符) -> href 为空, 标签保留;
    /// 危险协议的图片扩展名不得升级为 `<img>`.
    #[test]
    fn dangerous_urls_get_empty_href() {
        for url in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java script:alert(1)",
            "java\tscript:alert(1)",
            "java\u{1}script:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "file:///etc/passwd",
            "file:img.png",
        ] {
            let body = body_of(&format!("[[{url}]]"));
            assert!(!body.contains("<img"), "url {url}: {body}");
            assert!(
                body.contains(&format!(r#"<a href="">{url}</a>"#)),
                "url {url}: {body}"
            );
        }
    }

    /// 相对路径 / #fragment / http / https / mailto / 未知协议保持可点击 (href 转义保留).
    #[test]
    fn safe_urls_keep_escaped_href() {
        for url in [
            "https://example.com/doc.pdf",
            "http://example.com/x?a=1",
            "mailto:user@example.com",
            "relative/path.txt",
            "#fragment",
            "custom-scheme:opaque",
        ] {
            let body = body_of(&format!("[[{url}]]"));
            assert!(
                body.contains(&format!(r#"<a href="{url}">"#)),
                "url {url}: {body}"
            );
        }
    }

    /// href 与 label 都经 HtmlEscape (属性引号/尖括号/& 不外泄).
    #[test]
    fn link_href_and_label_are_escaped() {
        let body = body_of(r#"[[https://example.com/a?x=1&y=2]["q" < & label]]"#);
        assert!(
            body.contains(
                r#"<a href="https://example.com/a?x=1&amp;y=2">&quot;q&quot; &lt; &amp; label</a>"#
            ),
            "body: {body}"
        );
    }

    /// 危险链接的标签同样转义.
    #[test]
    fn dangerous_link_label_is_escaped() {
        let body = body_of(r#"[[javascript:alert("x")]]"#);
        assert!(
            body.contains(r#"<a href="">javascript:alert(&quot;x&quot;)</a>"#),
            "body: {body}"
        );
    }

    /// 既有裸图片行为保留: 安全相对路径仍渲染 `<img>`.
    #[test]
    fn safe_bare_image_still_renders_img() {
        let body = body_of("[[./static/pic.jpg]]");
        assert!(
            body.contains(r#"<img src="./static/pic.jpg" alt="./static/pic.jpg">"#),
            "body: {body}"
        );
    }
}
