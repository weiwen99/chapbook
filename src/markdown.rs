//! Markdown 文档渲染 (comrak 0.54, 纯 Rust, 无子进程).
//!
//! 管线: YAML front matter 剥离 (`title:` 提取) → comrak 渲染
//! (`HeadingAdapter` 生成标题锚点并采集 TOC 条目; `SyntaxHighlighterAdapter`
//! 接 highlight 模块做 syntect 高亮) → 自建 TOC → `render::doc_page` 自组页面.
//!
//! 与旧 pandoc 管线的差异 (Phase 2, 见
//! docs/2026-08-05-proposal-remove-dependency-of-pandoc.org):
//! - 代码块经 syntect 高亮, 类名与 org/代码文件一致 (同一调色板)
//! - 数学公式按原文显示 (pandoc 的 KaTeX 本就是 CDN 链接, 离线不渲染; katex-rs 候选未采用)
//! - 文档内 raw HTML 转义显示 (pandoc 原样透传; 安全改进, 服务端渲染不执行内嵌 HTML)

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;

use comrak::adapters::{HeadingAdapter, HeadingMeta, SyntaxHighlighterAdapter};
use comrak::nodes::Sourcepos;
use comrak::{Options, markdown_to_html_with_plugins, options::Plugins};

use crate::{highlight, render};

/// 渲染 .md 源文本为 body fragment, 返回 (页面标题, body HTML).
/// 标题取 YAML front matter 的 `title:`; 缺失时回退文件名 (由调用方传入).
pub fn render(source: &str, file_name: &str) -> (String, String) {
    let (front_title, source) = strip_front_matter(source);
    let title = front_title.clone().unwrap_or_else(|| file_name.to_string());

    let mut options = Options::default();
    // GFM 核心扩展 + pandoc markdown 默认具备的 footnotes / definition lists
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.tagfilter = true;
    options.extension.footnotes = true;
    options.extension.description_lists = true;
    options.parse.smart = true; // 与 pandoc markdown 的 smart 排版对齐
    // raw HTML 转义显示 (安全: 文档页不应执行内嵌 HTML)
    options.render.escape = true;
    options.render.tasklist_classes = true;

    let heading = HeadingCollector::default();
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&heading);
    plugins.render.codefence_syntax_highlighter = Some(&MdHighlighter);

    let body = markdown_to_html_with_plugins(source, &options, &plugins);

    // pandoc 行为对齐: front matter 有 title 时渲染 title-block-header
    let header = front_title
        .as_deref()
        .map(render::title_header)
        .unwrap_or_default();
    let toc = render::toc_html(&heading.into_entries(), None);
    (title, format!("{header}{toc}{body}"))
}

/// 剥离 YAML front matter (首行 `---` 起始、以 `---` 或 `...` 闭合的文档头),
/// 提取 `title:` (首个非空值, 去掉引号). 未闭合时视为普通内容, 原样返回.
fn strip_front_matter(source: &str) -> (Option<String>, &str) {
    let Some(first_end) = source.find('\n') else {
        return (None, source);
    };
    if source[..first_end].trim_end() != "---" {
        return (None, source);
    }

    let mut title = None;
    let mut remaining = &source[first_end + 1..];
    loop {
        let (line, rest) = match remaining.split_once('\n') {
            Some((line, rest)) => (line, rest),
            None => (remaining, ""),
        };
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            // 找到闭合: 剥离 front matter, 正文从下一行开始
            return (title, rest);
        }
        if title.is_none()
            && let Some(value) = trimmed.strip_prefix("title:")
        {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            if !value.is_empty() {
                title = Some(value.to_string());
            }
        }
        if rest.is_empty() {
            // 未闭合: 不是 front matter, 整篇视为普通内容
            return (None, source);
        }
        remaining = rest;
    }
}

/// 采集标题 (文档顺序 + 去重 slug), 渲染时输出 `<hN><a id= href=>` 锚点.
/// 锚点 slug 与 org 共用 `render::slugify` — TOC 链接与正文锚点一一对应 (测试锁定).
/// 适配器 trait 要求 Send + Sync, 采集状态用 Mutex (渲染期单线程访问, 无竞争).
struct HeadingCollector {
    entries: Mutex<(Vec<render::TocEntry>, HashSet<String>)>,
}

impl Default for HeadingCollector {
    fn default() -> Self {
        HeadingCollector {
            entries: Mutex::new((Vec::new(), HashSet::new())),
        }
    }
}

impl HeadingCollector {
    fn into_entries(self) -> Vec<render::TocEntry> {
        self.entries.into_inner().unwrap().0
    }
}

impl HeadingAdapter for HeadingCollector {
    fn enter(
        &self,
        output: &mut dyn fmt::Write,
        heading: &HeadingMeta,
        _sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        let mut inner = self.entries.lock().unwrap();
        let slug = render::slugify(&heading.content, &mut inner.1);
        write!(
            output,
            "<h{}><a id=\"{}\" href=\"#{}\">",
            heading.level, slug, slug
        )?;
        inner.0.push(render::TocEntry {
            level: heading.level as usize,
            text: heading.content.clone(),
            slug,
        });
        Ok(())
    }

    fn exit(&self, output: &mut dyn fmt::Write, heading: &HeadingMeta) -> fmt::Result {
        write!(output, "</a></h{}>", heading.level)
    }
}

/// 代码块高亮适配器: 接 highlight 模块 (syntect classed spans, 不带行号).
struct MdHighlighter;

impl SyntaxHighlighterAdapter for MdHighlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        output.write_str(&highlight::highlight(code, lang, false))
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        _attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        output.write_str("<pre class=\"sourceCode\">")
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        match attributes.get("class") {
            Some(class) => write!(output, "<code class=\"{}\">", class),
            None => output.write_str("<code>"),
        }
    }
}
