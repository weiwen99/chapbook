//! anydoc [`Document`] -> 安全 HTML (EPUB / FB2 共用).
//!
//! 所有正文都经 [`HtmlWriter`] 逐段生成并即时检查 [`MAX_HTML_BYTES`]:
//! 不先拼一个无上限的 String 再检查. 唯一可信的原样输出是模块自己生成的
//! 标签; 模型里的文本、属性、URL、语言名一律转义, 来源 HTML 永远不进入
//! `PreEscaped`.
//!
//! 锚点: 先扫一遍 blocks/notes (含 list / table / quote), 标题用
//! [`crate::render::slugify`] 分配唯一 slug, 同时把 Heading.anchor /
//! Inline::Anchor / Note.id 映射到目标 id. 重复源 id 首次出现生效; 内联
//! 锚点只在其目标首次出现时输出 `<span id>`, 避免重复 id. 内部链接只指向
//! 实际存在的目标.

use std::collections::{HashMap, HashSet};

use base64::Engine as _;

use anydoc::model::{
    Block, CellSlot, Document, ImageSource, Inline, LinkTarget, List, MarkerKind, Style, Table,
    TableKind,
};

use super::{
    Error, MAX_ASSET_BYTES, MAX_DEPTH, MAX_HTML_BYTES, MAX_INLINE_IMAGE_BYTES, decode_image_base64,
    image_media_type,
};

/// 有界 HTML 写入器. `push` 只接受本模块生成的标记或已校验的 MIME/URL;
/// 用户文本必须走 `push_escaped`. 每次追加前检查上限, 所以超限时不会先
/// 分配一个巨大的中间字符串.
pub(super) struct HtmlWriter {
    out: String,
    limit: usize,
}

impl HtmlWriter {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            out: String::new(),
            limit,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.out.len()
    }

    pub(super) fn push(&mut self, raw: &str) -> Result<(), Error> {
        let Some(total) = self.out.len().checked_add(raw.len()) else {
            return Err(Error::Limit("max_html_bytes"));
        };
        if total > self.limit {
            return Err(Error::Limit("max_html_bytes"));
        }
        self.out.push_str(raw);
        Ok(())
    }

    /// 文本 / 属性值转义. 属性一律用双引号包围, 因此 `"` 必须转义; `'`
    /// 也一并转义, 让同一函数可安全用于任意属性值.
    pub(super) fn push_escaped(&mut self, text: &str) -> Result<(), Error> {
        let mut start = 0usize;
        for (index, ch) in text.char_indices() {
            let escaped = match ch {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&#39;",
                _ => continue,
            };
            if start < index {
                self.push(&text[start..index])?;
            }
            self.push(escaped)?;
            start = index + ch.len_utf8();
        }
        if start < text.len() {
            self.push(&text[start..])?;
        }
        Ok(())
    }

    /// 无分配的十进制无符号数写入.
    pub(super) fn push_u64(&mut self, mut value: u64) -> Result<(), Error> {
        let mut buf = [0u8; 20];
        let mut index = buf.len();
        loop {
            index -= 1;
            buf[index] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        let digits = std::str::from_utf8(&buf[index..]).expect("ASCII digits");
        self.push(digits)
    }

    /// 所有写入都已在写入时检查上限; 完成时不再失败.
    pub(super) fn finish(self) -> String {
        self.out
    }
}

/* ---------- 锚点 / TOC 预扫描 ---------- */

struct NotePlan {
    slug: String,
}

#[derive(Default)]
struct Plan {
    /// 源 anchor id -> 目标 DOM id (重复源 id 首次出现生效).
    anchors: HashMap<String, String>,
    /// Note.id -> `Document.notes` 下标 (重复 id 首次出现生效).
    note_targets: HashMap<String, usize>,
    notes: Vec<NotePlan>,
    /// 正文按文档顺序的标题 slug; 渲染时用同一游标.
    heading_slugs: Vec<String>,
    toc: Vec<crate::render::TocEntry>,
    /// 已由标题 / note 区块输出的目标 id; 内联锚点不再重复输出.
    emitted: HashSet<String>,
    toc_estimate: usize,
}

struct Planner {
    used: HashSet<String>,
    plan: Plan,
    /// 非标题锚点的生成计数; id 与标题 slug 共用 `used` 保证全局唯一.
    next_generated: usize,
}

impl Planner {
    fn new() -> Self {
        Self {
            used: HashSet::new(),
            plan: Plan::default(),
            next_generated: 0,
        }
    }

    fn build(doc: &Document) -> Result<Plan, Error> {
        let mut planner = Self::new();
        planner.blocks(&doc.blocks, 0)?;
        for (index, note) in doc.notes.iter().enumerate() {
            // note 区块的 DOM id 是服务端生成的, 不直接使用来源 id/包内路径.
            let slug = planner.fresh_id("note");
            planner.plan.emitted.insert(slug.clone());
            planner.plan.notes.push(NotePlan { slug: slug.clone() });
            if !note.id.trim().is_empty() {
                planner
                    .plan
                    .note_targets
                    .entry(note.id.clone())
                    .or_insert(index);
                planner.plan.anchors.entry(note.id.clone()).or_insert(slug);
            }
            planner.blocks(&note.blocks, 0)?;
        }
        Ok(planner.plan)
    }

    /// 不透明目标 id: 来源 anchor / note id 只进映射, 不进 DOM 文本.
    fn fresh_id(&mut self, prefix: &str) -> String {
        loop {
            self.next_generated = self.next_generated.saturating_add(1);
            let n = self.next_generated;
            let candidate = format!("{prefix}-{n}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<(), Error> {
        check_depth(depth)?;
        for block in blocks {
            self.block(block, depth)?;
        }
        Ok(())
    }

    fn block(&mut self, block: &Block, depth: usize) -> Result<(), Error> {
        check_depth(depth)?;
        match block {
            Block::Heading {
                level,
                anchor,
                content,
            } => {
                let text = plain_text(content, depth)?;
                let raw = if text.trim().is_empty() {
                    "heading"
                } else {
                    text.as_str()
                };
                let slug = crate::render::slugify(raw, &mut self.used);
                self.plan.heading_slugs.push(slug.clone());
                self.plan.emitted.insert(slug.clone());
                if let Some(anchor) = anchor
                    && !anchor.is_empty()
                {
                    self.plan
                        .anchors
                        .entry(anchor.clone())
                        .or_insert(slug.clone());
                }
                let level = (*level as usize).clamp(1, 6);
                self.plan.toc_estimate = self
                    .plan
                    .toc_estimate
                    .saturating_add(64)
                    .saturating_add(level.saturating_mul(8))
                    .saturating_add(escaped_len(&text))
                    .saturating_add(escaped_len(&slug));
                if self.plan.toc_estimate > MAX_HTML_BYTES {
                    return Err(Error::Limit("max_html_bytes"));
                }
                self.plan
                    .toc
                    .push(crate::render::TocEntry { level, text, slug });
                self.inlines(content, depth)?;
            }
            Block::Paragraph(inlines) => self.inlines(inlines, depth)?,
            Block::List(list) => self.list(list, depth)?,
            Block::Table(table) => self.table(table, depth)?,
            Block::BlockQuote(blocks) => self.blocks(blocks, depth + 1)?,
            Block::CodeBlock { .. } | Block::Rule => {}
        }
        Ok(())
    }

    fn list(&mut self, list: &List, depth: usize) -> Result<(), Error> {
        for item in &list.items {
            self.blocks(&item.blocks, depth + 1)?;
        }
        Ok(())
    }

    fn table(&mut self, table: &Table, depth: usize) -> Result<(), Error> {
        for row in &table.grid {
            for slot in row {
                if let CellSlot::Origin(cell) = slot {
                    self.blocks(&cell.blocks, depth + 1)?;
                }
            }
        }
        Ok(())
    }

    fn inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<(), Error> {
        check_depth(depth)?;
        for inline in inlines {
            match inline {
                Inline::Anchor(id) => {
                    if !id.is_empty() && !self.plan.anchors.contains_key(id) {
                        let slug = self.fresh_id("anchor");
                        self.plan.anchors.insert(id.clone(), slug);
                    }
                }
                Inline::Link { content, .. } => self.inlines(content, depth + 1)?,
                Inline::Text { .. }
                | Inline::Image { .. }
                | Inline::NoteRef(_)
                | Inline::LineBreak => {}
            }
        }
        Ok(())
    }
}

fn check_depth(depth: usize) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        Err(Error::Limit("max_depth"))
    } else {
        Ok(())
    }
}

/// 有界纯文本 (TOC / 标题 slug 用). 与 anydoc 的 `inlines_to_plain_text`
/// 语义一致, 但 Link 嵌套也受 [`MAX_DEPTH`] 约束, 不让恶意嵌套把递归栈
/// 打穿.
fn plain_text(inlines: &[Inline], depth: usize) -> Result<String, Error> {
    check_depth(depth)?;
    let mut out = String::new();
    collect_plain_text(inlines, depth, &mut out)?;
    Ok(out)
}

fn collect_plain_text(inlines: &[Inline], depth: usize, out: &mut String) -> Result<(), Error> {
    check_depth(depth)?;
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Link { content, .. } => collect_plain_text(content, depth + 1, out)?,
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Anchor(_) | Inline::NoteRef(_) => {}
            Inline::LineBreak => out.push('\n'),
        }
    }
    Ok(())
}

/// 段落是否只剩锚点 / 空白 (不生成可见空 `<p>`). 语义对齐 anydoc
/// `inlines_are_empty`, 但递归有界.
fn renders_nothing(inlines: &[Inline], depth: usize) -> Result<bool, Error> {
    check_depth(depth)?;
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => {
                if !text.trim().is_empty() {
                    return Ok(false);
                }
            }
            Inline::Link { content, target } => {
                if !target.is_empty() || !renders_nothing(content, depth + 1)? {
                    return Ok(false);
                }
            }
            Inline::Image { .. } | Inline::NoteRef(_) => return Ok(false),
            Inline::Anchor(_) | Inline::LineBreak => {}
        }
    }
    Ok(true)
}

fn escaped_len(text: &str) -> usize {
    text.chars()
        .map(|ch| match ch {
            '&' => 5,
            '<' | '>' => 4,
            '"' | '\'' => 6,
            _ => ch.len_utf8(),
        })
        .fold(0usize, |total, len| total.saturating_add(len))
}

/* ---------- 渲染 ---------- */

struct Renderer<'a> {
    doc: &'a Document,
    plan: Plan,
    writer: HtmlWriter,
    heading_index: usize,
    /// AssetId -> 已生成的 data URI; None = 不支持 / 无效图片 (可见占位).
    image_uris: HashMap<usize, Option<String>>,
    /// data URI 源 (按字符串身份缓存) -> 归一化 data URI; None = 不支持.
    external_images: HashMap<(usize, usize), Option<String>>,
    /// 已计入预算的内嵌资产字节 (唯一资产).
    inline_bytes: usize,
}

/// 渲染正文 (含 TOC, 不含 `.cb-doc` 外壳). 调用方负责页面 title / 包装.
pub(super) fn render(doc: &Document) -> Result<String, Error> {
    let plan = Planner::build(doc)?;
    let mut writer = HtmlWriter::new(MAX_HTML_BYTES);
    if !plan.toc.is_empty() {
        let toc = crate::render::toc_html(&plan.toc, None);
        if toc.len() > MAX_HTML_BYTES {
            return Err(Error::Limit("max_html_bytes"));
        }
        writer.push(&toc)?;
    }
    debug_assert!(writer.len() <= MAX_HTML_BYTES);
    let mut renderer = Renderer {
        doc,
        plan,
        writer,
        heading_index: 0,
        image_uris: HashMap::new(),
        external_images: HashMap::new(),
        inline_bytes: 0,
    };
    renderer.blocks(&doc.blocks, 0)?;
    renderer.notes()?;
    Ok(renderer.writer.finish())
}

impl Renderer<'_> {
    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<(), Error> {
        check_depth(depth)?;
        for block in blocks {
            self.block(block, depth)?;
        }
        Ok(())
    }

    fn block(&mut self, block: &Block, depth: usize) -> Result<(), Error> {
        check_depth(depth)?;
        match block {
            Block::Heading { level, content, .. } => {
                let slug = self
                    .plan
                    .heading_slugs
                    .get(self.heading_index)
                    .cloned()
                    .ok_or_else(|| Error::Invalid("heading plan mismatch".to_string()))?;
                self.heading_index += 1;
                let (open, close) = match (*level).clamp(1, 6) {
                    1 => ("<h1>", "</h1>"),
                    2 => ("<h2>", "</h2>"),
                    3 => ("<h3>", "</h3>"),
                    4 => ("<h4>", "</h4>"),
                    5 => ("<h5>", "</h5>"),
                    _ => ("<h6>", "</h6>"),
                };
                self.writer.push(open)?;
                self.writer.push("<a id=\"")?;
                self.writer.push_escaped(&slug)?;
                self.writer.push("\" href=\"#")?;
                self.writer.push_escaped(&slug)?;
                self.writer.push("\">")?;
                self.inlines(content, depth)?;
                self.writer.push("</a>")?;
                self.writer.push(close)?;
            }
            Block::Paragraph(inlines) => {
                if renders_nothing(inlines, depth)? {
                    // 只有锚点 / 空白的段落不生成可见空 <p>, 只保留落点.
                    self.inlines(inlines, depth)?;
                } else {
                    self.writer.push("<p>")?;
                    self.inlines(inlines, depth)?;
                    self.writer.push("</p>")?;
                }
            }
            Block::List(list) => self.list(list, depth)?,
            Block::Table(table) => self.table(table, depth)?,
            Block::BlockQuote(blocks) => {
                self.writer.push("<blockquote>")?;
                self.blocks(blocks, depth + 1)?;
                self.writer.push("</blockquote>")?;
            }
            Block::CodeBlock { lang, text } => {
                self.writer.push("<pre><code")?;
                if let Some(lang) = lang.as_deref().filter(|lang| !lang.is_empty()) {
                    self.writer.push(" class=\"language-")?;
                    self.writer.push_escaped(lang)?;
                    self.writer.push("\"")?;
                }
                self.writer.push(">")?;
                self.writer.push_escaped(text)?;
                self.writer.push("</code></pre>")?;
            }
            Block::Rule => self.writer.push("<hr>")?,
        }
        Ok(())
    }

    fn list(&mut self, list: &List, depth: usize) -> Result<(), Error> {
        let ordered = list.ordered();
        let has_task = list.items.iter().any(|item| item.checked.is_some());
        if ordered {
            self.writer.push("<ol")?;
            // HTML 原生编号只区分十进制; alpha/roman 用 type 保留来源 marker 家族.
            let marker_type = match list.marker {
                MarkerKind::LowerAlpha => Some("a"),
                MarkerKind::UpperAlpha => Some("A"),
                MarkerKind::LowerRoman => Some("i"),
                MarkerKind::UpperRoman => Some("I"),
                MarkerKind::Decimal | MarkerKind::Bullet => None,
            };
            if let Some(marker_type) = marker_type {
                self.writer.push(" type=\"")?;
                self.writer.push(marker_type)?;
                self.writer.push("\"")?;
            }
            if has_task {
                self.writer.push(" class=\"contains-task-list\"")?;
            }
            if list.start != 1 {
                self.writer.push(" start=\"")?;
                self.writer.push_u64(list.start)?;
                self.writer.push("\"")?;
            }
            self.writer.push(">")?;
        } else {
            self.writer.push(if has_task {
                "<ul class=\"contains-task-list\">"
            } else {
                "<ul>"
            })?;
        }
        for item in &list.items {
            self.list_item(item, depth)?;
        }
        self.writer.push(if ordered { "</ol>" } else { "</ul>" })
    }

    fn list_item(&mut self, item: &anydoc::model::ListItem, depth: usize) -> Result<(), Error> {
        let custom_label = item
            .marker_label
            .as_deref()
            .filter(|label| !label.is_empty());
        let task = item.checked.is_some();
        self.writer.push("<li")?;
        if task || custom_label.is_some() {
            self.writer.push(" class=\"")?;
            if task {
                self.writer.push("task-list-item")?;
                if custom_label.is_some() {
                    self.writer.push(" ")?;
                }
            }
            if custom_label.is_some() {
                self.writer.push("cb-list-custom")?;
            }
            self.writer.push("\"")?;
        }
        self.writer.push(">")?;
        if let Some(checked) = item.checked {
            self.writer.push("<input type=\"checkbox\" disabled")?;
            if checked {
                self.writer.push(" checked")?;
            }
            self.writer.push("> ")?;
        }
        if let Some(label) = custom_label {
            self.writer.push("<span class=\"cb-marker\">")?;
            self.writer.push_escaped(label)?;
            self.writer.push("</span> ")?;
        }
        self.blocks(&item.blocks, depth + 1)?;
        self.writer.push("</li>")
    }

    fn table(&mut self, table: &Table, depth: usize) -> Result<(), Error> {
        if table.kind == TableKind::Layout
            && table.is_single_cell()
            && let Some(CellSlot::Origin(cell)) = table.grid.first().and_then(|row| row.first())
        {
            return self.blocks(&cell.blocks, depth + 1);
        }

        let header_rows = table.header_rows.min(table.grid.len());
        // rowspan 不能跨 HTML 行组；同组保留跨度，th 保留表头语义。
        self.writer.push("<table><tbody>")?;
        for (index, row) in table.grid.iter().enumerate() {
            self.table_row(row, index < header_rows, depth)?;
        }
        self.writer.push("</tbody></table>")
    }

    fn table_row(&mut self, row: &[CellSlot], header: bool, depth: usize) -> Result<(), Error> {
        let (open, close) = if header {
            ("<th", "</th>")
        } else {
            ("<td", "</td>")
        };
        self.writer.push("<tr>")?;
        for slot in row {
            let CellSlot::Origin(cell) = slot else {
                continue;
            };
            self.writer.push(open)?;
            if cell.col_span > 1 {
                self.writer.push(" colspan=\"")?;
                self.writer.push_u64(cell.col_span as u64)?;
                self.writer.push("\"")?;
            }
            if cell.row_span > 1 {
                self.writer.push(" rowspan=\"")?;
                self.writer.push_u64(cell.row_span as u64)?;
                self.writer.push("\"")?;
            }
            self.writer.push(">")?;
            self.blocks(&cell.blocks, depth + 1)?;
            self.writer.push(close)?;
        }
        self.writer.push("</tr>")
    }

    fn inlines(&mut self, inlines: &[Inline], depth: usize) -> Result<(), Error> {
        check_depth(depth)?;
        for inline in inlines {
            self.inline(inline, depth)?;
        }
        Ok(())
    }

    fn inline(&mut self, inline: &Inline, depth: usize) -> Result<(), Error> {
        match inline {
            Inline::Text { text, style } => self.styled_text(text, *style),
            Inline::Link { content, target } => self.link(content, target, depth),
            Inline::Image { alt, source } => self.image(alt, source),
            Inline::Anchor(id) => {
                let Some(target) = self.plan.anchors.get(id).cloned() else {
                    return Ok(());
                };
                if self.plan.emitted.insert(target.clone()) {
                    self.writer.push("<span id=\"")?;
                    self.writer.push_escaped(&target)?;
                    self.writer.push("\"></span>")?;
                }
                Ok(())
            }
            Inline::NoteRef(id) => {
                let Some(&index) = self.plan.note_targets.get(id) else {
                    return self.writer.push("<span class=\"cb-note-ref\">[?]</span>");
                };
                let slug = self
                    .plan
                    .notes
                    .get(index)
                    .map(|note| note.slug.clone())
                    .ok_or_else(|| Error::Invalid("note plan mismatch".to_string()))?;
                self.writer.push("<a class=\"cb-note-ref\" href=\"#")?;
                self.writer.push_escaped(&slug)?;
                self.writer.push("\">[")?;
                self.writer.push_u64(index as u64 + 1)?;
                self.writer.push("]</a>")
            }
            Inline::LineBreak => self.writer.push("<br>"),
        }
    }

    fn styled_text(&mut self, text: &str, style: Style) -> Result<(), Error> {
        if text.is_empty() {
            return Ok(());
        }
        if style.bold {
            self.writer.push("<strong>")?;
        }
        if style.italic {
            self.writer.push("<em>")?;
        }
        if style.strike {
            self.writer.push("<del>")?;
        }
        if style.code {
            self.writer.push("<code>")?;
        }
        self.writer.push_escaped(text)?;
        if style.code {
            self.writer.push("</code>")?;
        }
        if style.strike {
            self.writer.push("</del>")?;
        }
        if style.italic {
            self.writer.push("</em>")?;
        }
        if style.bold {
            self.writer.push("</strong>")?;
        }
        Ok(())
    }

    fn link(&mut self, content: &[Inline], target: &LinkTarget, depth: usize) -> Result<(), Error> {
        match target {
            LinkTarget::External(url) => {
                let href = if crate::render::dangerous_url(url) {
                    ""
                } else {
                    url.as_str()
                };
                self.writer.push("<a href=\"")?;
                self.writer.push_escaped(href)?;
                self.writer.push("\">")?;
                self.inlines(content, depth + 1)?;
                self.writer.push("</a>")
            }
            LinkTarget::Anchor(id) => {
                let Some(slug) = self.plan.anchors.get(id).cloned() else {
                    // 缺失目标保留可读文字, 不产生死链接.
                    return self.inlines(content, depth + 1);
                };
                self.writer.push("<a href=\"#")?;
                self.writer.push_escaped(&slug)?;
                self.writer.push("\">")?;
                self.inlines(content, depth + 1)?;
                self.writer.push("</a>")
            }
            // 包内非 spine 的相对资源不能原样变成文件系统 URL; 只留文字.
            LinkTarget::Relative(_) => self.inlines(content, depth + 1),
        }
    }

    fn image(&mut self, alt: &str, source: &ImageSource) -> Result<(), Error> {
        match source {
            ImageSource::Asset(id) => self.asset_image(id.0, alt),
            ImageSource::External(url) => self.external_image(url, alt),
            ImageSource::Unavailable => placeholder(&mut self.writer, alt),
        }
    }

    fn asset_image(&mut self, id: usize, alt: &str) -> Result<(), Error> {
        if let Some(Some(uri)) = self.image_uris.get(&id) {
            return write_img(&mut self.writer, uri, alt);
        }
        if let Some(None) = self.image_uris.get(&id) {
            return placeholder(&mut self.writer, alt);
        }
        let uri = self.build_asset_uri(id)?;
        self.image_uris.insert(id, uri);
        if let Some(Some(uri)) = self.image_uris.get(&id) {
            write_img(&mut self.writer, uri, alt)
        } else {
            placeholder(&mut self.writer, alt)
        }
    }

    fn build_asset_uri(&mut self, id: usize) -> Result<Option<String>, Error> {
        let doc = self.doc;
        let Some(asset) = doc.assets.get(id) else {
            return Ok(None);
        };
        if asset.bytes.len() > MAX_INLINE_IMAGE_BYTES {
            return Err(Error::Limit("max_inline_image_bytes"));
        }
        let Some(media_type) = image_media_type(&asset.bytes)? else {
            return Ok(None);
        };
        if self.inline_bytes.saturating_add(asset.bytes.len()) > MAX_ASSET_BYTES {
            return Err(Error::Limit("max_asset_bytes"));
        }
        self.inline_bytes += asset.bytes.len();
        Ok(Some(data_uri(media_type, &asset.bytes)))
    }

    fn external_image(&mut self, url: &str, alt: &str) -> Result<(), Error> {
        if strip_scheme_ci(url, "data:").is_some() {
            let key = (url.as_ptr() as usize, url.len());
            if let Some(Some(uri)) = self.external_images.get(&key) {
                return write_img(&mut self.writer, uri, alt);
            }
            if let Some(None) = self.external_images.get(&key) {
                return placeholder(&mut self.writer, alt);
            }
            match normalize_data_image(url)? {
                Some((media_type, bytes)) => {
                    if self.inline_bytes.saturating_add(bytes.len()) > MAX_ASSET_BYTES {
                        return Err(Error::Limit("max_asset_bytes"));
                    }
                    self.inline_bytes += bytes.len();
                    let uri = data_uri(media_type, &bytes);
                    self.external_images.insert(key, Some(uri));
                }
                None => {
                    self.external_images.insert(key, None);
                }
            }
            return if let Some(Some(uri)) = self.external_images.get(&key) {
                write_img(&mut self.writer, uri, alt)
            } else {
                placeholder(&mut self.writer, alt)
            };
        }
        if is_http_image_url(url) {
            return write_img(&mut self.writer, url, alt);
        }
        placeholder(&mut self.writer, alt)
    }

    fn notes(&mut self) -> Result<(), Error> {
        let doc = self.doc;
        if doc.notes.is_empty() {
            return Ok(());
        }
        self.writer.push("<section class=\"cb-notes\">")?;
        for (index, note) in doc.notes.iter().enumerate() {
            let slug = self
                .plan
                .notes
                .get(index)
                .map(|note| note.slug.clone())
                .ok_or_else(|| Error::Invalid("note plan mismatch".to_string()))?;
            self.writer.push("<aside class=\"cb-note\" id=\"")?;
            self.writer.push_escaped(&slug)?;
            self.writer.push("\">")?;
            // 不额外生成 "[n]" 标签: FB2 注释自带源标题, 引用侧已有可点击编号.
            self.blocks(&note.blocks, 0)?;
            self.writer.push("</aside>")?;
        }
        self.writer.push("</section>")
    }
}

fn write_img(writer: &mut HtmlWriter, src: &str, alt: &str) -> Result<(), Error> {
    writer.push("<img src=\"")?;
    writer.push_escaped(src)?;
    writer.push("\" alt=\"")?;
    writer.push_escaped(alt)?;
    writer.push("\">")
}

fn placeholder(writer: &mut HtmlWriter, alt: &str) -> Result<(), Error> {
    writer.push("<span class=\"cb-image-missing\">")?;
    if alt.is_empty() {
        writer.push("[image unavailable]")?;
    } else {
        writer.push("[image: ")?;
        writer.push_escaped(alt)?;
        writer.push("]")?;
    }
    writer.push("</span>")
}

fn data_uri(media_type: &str, bytes: &[u8]) -> String {
    let encoded_len = bytes.len().div_ceil(3).saturating_mul(4);
    let mut uri = String::with_capacity(
        "data:"
            .len()
            .saturating_add(media_type.len())
            .saturating_add(";base64,".len())
            .saturating_add(encoded_len),
    );
    uri.push_str("data:");
    uri.push_str(media_type);
    uri.push_str(";base64,");
    base64::engine::general_purpose::STANDARD.encode_string(bytes, &mut uri);
    uri
}

fn strip_scheme_ci<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    if url.len() >= scheme.len()
        && url.is_char_boundary(scheme.len())
        && url[..scheme.len()].eq_ignore_ascii_case(scheme)
    {
        Some(&url[scheme.len()..])
    } else {
        None
    }
}

fn is_http_image_url(url: &str) -> bool {
    (strip_scheme_ci(url, "http://").is_some() || strip_scheme_ci(url, "https://").is_some())
        && !url.bytes().any(|byte| byte.is_ascii_control())
}

/// 来源 `data:` URI 只接受 `;base64` 形式; 解码后必须通过同一图片签名
/// 判定, 返回 (嗅探 MIME, 原字节), 由调用方在预算通过后再生成规范 URI
/// (不信任来源 MIME).
fn normalize_data_image(url: &str) -> Result<Option<(&'static str, Vec<u8>)>, Error> {
    let Some(rest) = strip_scheme_ci(url, "data:") else {
        return Ok(None);
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Ok(None);
    };
    let is_base64 = meta
        .rsplit(';')
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return Ok(None);
    }
    let bytes = match decode_image_base64(payload) {
        Ok(bytes) => bytes,
        Err(Error::Limit(limit)) => return Err(Error::Limit(limit)),
        Err(_) => return Ok(None),
    };
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(Error::Limit("max_inline_image_bytes"));
    }
    let Some(media_type) = image_media_type(&bytes)? else {
        return Ok(None);
    };
    Ok(Some((media_type, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anydoc::model::{Document, Inline, LinkTarget, Style};

    fn paragraph(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(inlines)
    }

    fn tiny_png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(b"IDAT");
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    #[test]
    fn duplicate_anchor_first_wins_and_ids_stay_unique() {
        let doc = Document {
            blocks: vec![
                paragraph(vec![
                    Inline::Anchor("dup".to_string()),
                    Inline::Link {
                        content: vec![Inline::plain("jump")],
                        target: LinkTarget::Anchor("dup".to_string()),
                    },
                ]),
                paragraph(vec![Inline::Anchor("dup".to_string())]),
            ],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert_eq!(html.matches("id=\"anchor-1\"").count(), 1);
        assert!(html.contains("<a href=\"#anchor-1\">jump</a>"));
        assert!(!html.contains("dup"));
    }

    #[test]
    fn anchor_only_paragraph_keeps_target_without_empty_paragraph() {
        let doc = Document {
            blocks: vec![
                paragraph(vec![Inline::Anchor("chapter".to_string())]),
                paragraph(vec![Inline::plain("text")]),
            ],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("<span id=\"anchor-1\"></span>"));
        assert!(!html.contains("<p></p>"));
        assert!(html.contains("<p>text</p>"));
        assert!(!html.contains("id=\"chapter\""));
    }

    #[test]
    fn missing_internal_link_keeps_text_without_dead_href() {
        let doc = Document {
            blocks: vec![paragraph(vec![Inline::Link {
                content: vec![Inline::plain("nowhere")],
                target: LinkTarget::Anchor("missing".to_string()),
            }])],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("nowhere"));
        assert!(!html.contains("<a href="));
    }

    #[test]
    fn relative_links_never_expose_package_paths() {
        let doc = Document {
            blocks: vec![paragraph(vec![Inline::Link {
                content: vec![Inline::plain("asset")],
                target: LinkTarget::Relative("OEBPS/notes.xhtml".to_string()),
            }])],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("asset"));
        assert!(!html.contains("OEBPS"));
        assert!(!html.contains("<a"));
    }

    #[test]
    fn missing_image_is_visible_and_alt_is_escaped() {
        let doc = Document {
            blocks: vec![paragraph(vec![Inline::Image {
                alt: "<bad & alt>".to_string(),
                source: ImageSource::Unavailable,
            }])],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("cb-image-missing"));
        assert!(html.contains("&lt;bad &amp; alt&gt;"));
        assert!(!html.contains("<bad"));
    }

    #[test]
    fn embedded_image_keeps_exact_bytes_and_uses_sniffed_mime() {
        let bytes = tiny_png();
        let expected = data_uri("image/png", &bytes);
        let doc = Document {
            blocks: vec![paragraph(vec![Inline::Image {
                alt: "cover".to_string(),
                source: ImageSource::Asset(anydoc::model::AssetId(0)),
            }])],
            notes: Vec::new(),
            assets: vec![anydoc::model::Asset {
                id: anydoc::model::AssetId(0),
                media_type: "application/octet-stream".to_string(),
                origin_part: "cover.bin".to_string(),
                bytes,
            }],
        };
        let html = render(&doc).unwrap();
        assert!(html.contains(expected.as_str()));
        assert!(html.contains("alt=\"cover\""));
    }

    #[test]
    fn source_data_uri_is_revalidated_and_mime_is_not_trusted() {
        let png = tiny_png();
        let normalized = data_uri("image/png", &png);
        let encoded = normalized
            .strip_prefix("data:image/png;base64,")
            .expect("data URI prefix");
        let doc = Document {
            blocks: vec![paragraph(vec![Inline::Image {
                alt: "cover".to_string(),
                source: ImageSource::External(format!("data:text/html;base64,{encoded}")),
            }])],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains(normalized.as_str()));
        assert!(!html.contains("text/html"));

        let malicious = Document {
            blocks: vec![paragraph(vec![Inline::Image {
                alt: "<script>".to_string(),
                source: ImageSource::External(
                    "data:text/html,<script>alert(1)</script>".to_string(),
                ),
            }])],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&malicious).unwrap();
        assert!(html.contains("cb-image-missing"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn heading_anchor_and_toc_share_the_same_slug() {
        let doc = Document {
            blocks: vec![
                Block::Heading {
                    level: 2,
                    anchor: Some("chapter-1".to_string()),
                    content: vec![Inline::plain("Chapter One")],
                },
                paragraph(vec![Inline::Link {
                    content: vec![Inline::plain("jump")],
                    target: LinkTarget::Anchor("chapter-1".to_string()),
                }]),
            ],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("<h2><a id=\"chapter-one\" href=\"#chapter-one\">"));
        assert!(html.contains("<a href=\"#chapter-one\">Chapter One</a>"));
        assert!(html.contains("<a href=\"#chapter-one\">jump</a>"));
        assert!(!html.contains("chapter-1\""));
    }

    #[test]
    fn identical_headings_stay_unique_and_links_use_first_source_id() {
        let doc = Document {
            blocks: vec![
                Block::Heading {
                    level: 1,
                    anchor: Some("x".to_string()),
                    content: vec![Inline::plain("Same")],
                },
                Block::Heading {
                    level: 1,
                    anchor: Some("x".to_string()),
                    content: vec![Inline::plain("Same")],
                },
                paragraph(vec![Inline::Link {
                    content: vec![Inline::plain("go")],
                    target: LinkTarget::Anchor("x".to_string()),
                }]),
            ],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("<h1><a id=\"same\" href=\"#same\">Same</a></h1>"));
        assert!(html.contains("<h1><a id=\"same-1\" href=\"#same-1\">Same</a></h1>"));
        assert!(html.contains("<a href=\"#same\">go</a>"));
        assert_eq!(html.matches("id=\"same\"").count(), 1);
        assert_eq!(html.matches("id=\"same-1\"").count(), 1);
    }

    #[test]
    fn note_refs_land_on_generated_note_ids() {
        let doc = Document {
            blocks: vec![paragraph(vec![
                Inline::plain("See "),
                Inline::NoteRef("n1".to_string()),
            ])],
            notes: vec![anydoc::model::Note {
                id: "n1".to_string(),
                kind: anydoc::model::NoteKind::Endnote,
                blocks: vec![paragraph(vec![Inline::plain("note body")])],
            }],
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("<a class=\"cb-note-ref\" href=\"#note-1\">[1]</a>"));
        assert!(html.contains("<aside class=\"cb-note\" id=\"note-1\">"));
        assert!(html.contains("note body"));
        assert!(!html.contains("id=\"n1\""));
    }

    #[test]
    fn output_cap_is_checked_before_appending() {
        let mut writer = HtmlWriter::new(8);
        writer.push("<p>").unwrap();
        assert!(matches!(
            writer.push("012345"),
            Err(Error::Limit("max_html_bytes"))
        ));
        assert_eq!(writer.len(), 3);
    }

    #[test]
    fn styled_runs_are_escaped_and_nested() {
        let doc = Document {
            blocks: vec![paragraph(vec![Inline::Text {
                text: "<b>".to_string(),
                style: Style {
                    bold: true,
                    italic: true,
                    strike: false,
                    code: true,
                },
            }])],
            notes: Vec::new(),
            assets: Vec::new(),
        };
        let html = render(&doc).unwrap();
        assert!(html.contains("<strong><em><code>&lt;b&gt;</code></em></strong>"));
    }
}
