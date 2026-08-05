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
