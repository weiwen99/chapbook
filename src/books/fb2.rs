//! FictionBook 2 (FB2) -> `anydoc::model::Document`.
//!
//! 严格转换: 解码与 XML 入口都用 `super::decode_xml` / `super::parse_xml`
//! (BOM/声明编码、无 replacement、DTD 预检、节点数上限). 本模块只认标准
//! FictionBook 命名空间或无命名空间; 未知实体、破损 XML、未知编码一律失败,
//! 不做猜测修复. 图片字节从 `<binary>` 严格 base64 解码, 只为实际被
//! `<image>` 引用的 id 建立资产; 其余二进制不解码.
//!
//! 输出只有公共 model: 不生成任何原始 HTML, 文本、样式与源锚点原样保留,
//! 由共享 renderer 负责转义与链接解析.

use std::borrow::Cow;
use std::cell::Cell as BudgetCell;
use std::collections::{HashMap, HashSet};

use anydoc::model::{
    Asset, AssetId, Block, Cell, CellSlot, Document, ImageSource, Inline, LinkTarget, Style, Table,
    TableKind, inlines_to_plain_text,
};
use roxmltree::Node;

use super::{Error, MAX_ASSET_BYTES, MAX_DEPTH};

/// 标准 FictionBook 2 命名空间; 无命名空间的旧文件也接受.
const FB2_NAMESPACE: &str = "http://www.gribuser.ru/xml/fictionbook/2.0";
/// XLink 命名空间: `l:href` 等前缀由文件自行绑定, 这里只认 URI.
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";

/// 整篇文档的表格网格位置上限: 真实单元格与 colspan/rowspan 展开都计入,
/// 在分配之前收费 — 恶意 span 或大量表格都不能逼出巨型网格.
const MAX_TABLE_POSITIONS: u64 = 200_000;

/// FB2 -> 公共 model. 一次解码 + 一次 DOM 解析, 不保留任何字节引用.
pub(super) fn parse(bytes: &[u8]) -> Result<Document, Error> {
    let text = super::decode_xml(bytes)?;
    let xml = super::parse_xml(&text)?;
    build(&xml)
}

fn build<'a, 'input>(xml: &'a roxmltree::Document<'input>) -> Result<Document, Error> {
    let root = xml.root_element();
    if root.tag_name().name() != "FictionBook" {
        return Err(Error::Unsupported("not a FictionBook document"));
    }
    match root.tag_name().namespace() {
        None | Some(FB2_NAMESPACE) => {}
        Some(_) => return Err(Error::Unsupported("unsupported FictionBook namespace")),
    }

    let binaries = binary_index(root);
    let (assets, asset_ids) = decode_assets(root, &binaries)?;
    let ctx = Ctx {
        assets: asset_ids,
        table_positions: BudgetCell::new(0),
    };

    let description = direct_child(root, "description");
    let title_info = description.and_then(|node| direct_child(node, "title-info"));
    let book_title = title_info
        .and_then(|node| direct_child(node, "book-title"))
        .map(|node| text_content(node).trim().to_string())
        .filter(|title| !title.is_empty());

    let mut blocks = Vec::new();

    // 封面: coverpage 里的 image 原样进 model, 缺失/不支持时是可见占位.
    if let Some(coverpage) = title_info.and_then(|node| direct_child(node, "coverpage")) {
        let mut inlines = Vec::new();
        for image in coverpage
            .children()
            .filter(|child| child.is_element() && child.tag_name().name() == "image")
        {
            inline_element(&ctx, image, Style::PLAIN, &mut inlines, 0)?;
        }
        if !inlines.is_empty() {
            blocks.push(Block::Paragraph(inlines));
        }
    }

    let bodies: Vec<_> = root
        .children()
        .filter(|child| child.is_element() && child.tag_name().name() == "body")
        .collect();

    let mut body_blocks = Vec::new();
    for body in &bodies {
        body_blocks.extend(convert_body(&ctx, *body, 0)?);
    }

    // 标题: 第一个 body 的 <title> 与 description/book-title 相同 (或没有
    // book-title) 时用它作文档标题, 避免重复; 不同则保留 book-title 作 H1,
    // 该 body 自己的分部标题仍留在正文, 不丢文本.
    let first_body_has_title = bodies
        .first()
        .is_some_and(|body| direct_child(*body, "title").is_some());
    let mut leading_title = None;
    if first_body_has_title {
        let position = body_blocks
            .iter()
            .position(|block| matches!(block, Block::Heading { level: 1, .. }));
        if let Some(position) = position {
            let same_as_book_title = match &body_blocks[position] {
                Block::Heading { content, .. } => book_title
                    .as_deref()
                    .is_none_or(|title| inlines_to_plain_text(content).trim() == title),
                _ => false,
            };
            if same_as_book_title {
                leading_title = Some(body_blocks.remove(position));
            }
        }
    }
    if leading_title.is_none() {
        leading_title = book_title.map(|title| Block::heading(1, vec![Inline::plain(title)]));
    }
    if let Some(title) = leading_title {
        blocks.push(title);
    }

    if let Some(annotation) = title_info.and_then(|node| direct_child(node, "annotation")) {
        let mut inner = convert_container(&ctx, annotation, 2, 0)?;
        if !inner.is_empty() {
            if let Some(id) = element_id(annotation) {
                attach_anchor(&mut inner, id);
            }
            blocks.push(Block::BlockQuote(inner));
        }
    }
    blocks.extend(body_blocks);

    Ok(Document {
        blocks,
        notes: Vec::new(),
        assets,
    })
}

/// 转换期上下文: 已解码资产按 binary id 查找; 表格展开预算按整篇文档累计,
/// 避免大量表格各自顶满单表上限.
struct Ctx<'a> {
    assets: HashMap<&'a str, AssetId>,
    table_positions: BudgetCell<u64>,
}

/* ---------- 二进制与资产 ---------- */

/// binary id -> 元素; 重复 id 首次出现生效.
fn binary_index<'a, 'input>(root: Node<'a, 'input>) -> HashMap<&'a str, Node<'a, 'input>> {
    let mut binaries = HashMap::new();
    for child in root.children().filter(Node::is_element) {
        if child.tag_name().name() != "binary" {
            continue;
        }
        if let Some(id) = element_id(child) {
            binaries.entry(id).or_insert(child);
        }
    }
    binaries
}

/// 只解码被 `<image>` 引用的 binary, 按首次引用顺序分配 AssetId.
/// 引用存在但没有对应 binary / 不是可识别图片 -> Unavailable 占位;
/// 被引用 binary 的 base64 非法或超过单图/总资产预算 -> 类型化失败.
fn decode_assets<'a, 'input>(
    root: Node<'a, 'input>,
    binaries: &HashMap<&'a str, Node<'a, 'input>>,
) -> Result<(Vec<Asset>, HashMap<&'a str, AssetId>), Error> {
    let mut assets = Vec::new();
    let mut asset_ids = HashMap::new();
    let mut total = 0usize;
    let mut seen = HashSet::new();
    for node in root.descendants().filter(Node::is_element) {
        if node.tag_name().name() != "image" {
            continue;
        }
        let Some(fragment) = href(node).and_then(fragment_id) else {
            continue;
        };
        let Some((&id, binary)) = binaries.get_key_value(fragment.as_ref()) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let encoded = text_content(*binary);
        if encoded.trim().is_empty() {
            continue;
        }
        let bytes = super::decode_image_base64(&encoded)?;
        let Some(media_type) = super::image_media_type(&bytes)? else {
            continue;
        };
        total = total
            .checked_add(bytes.len())
            .ok_or(Error::Limit("FB2 asset budget"))?;
        if total > MAX_ASSET_BYTES {
            return Err(Error::Limit("FB2 asset budget"));
        }
        let asset_id = AssetId(assets.len());
        assets.push(Asset {
            id: asset_id,
            media_type: media_type.to_string(),
            origin_part: id.to_string(),
            bytes,
        });
        asset_ids.insert(id, asset_id);
    }
    Ok((assets, asset_ids))
}

/* ---------- 块级转换 ---------- */

fn convert_body<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    let mut anchor = element_id(node);
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for child in node.children() {
        if child.is_text() {
            push_container_text(&mut pending, child.text().unwrap_or(""));
        } else if child.is_element() {
            match child.tag_name().name() {
                "title" => {
                    flush_inlines(&mut pending, &mut out);
                    let mut heading = title_heading(ctx, child, 1, depth)?;
                    attach_heading_anchor(&mut heading, anchor.take());
                    out.push(heading);
                }
                "section" if is_notes_body(node) => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_note_blocks(ctx, child, depth)?);
                }
                "section" => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_section(ctx, child, 2, depth)?);
                }
                name if is_block(name) => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_block(ctx, child, 2, depth)?);
                }
                _ => inline_element(ctx, child, Style::PLAIN, &mut pending, depth)?,
            }
        }
    }
    flush_inlines(&mut pending, &mut out);
    if let Some(id) = anchor {
        attach_anchor(&mut out, id);
    }
    Ok(out)
}

fn convert_section<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    level: u8,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    let mut anchor = element_id(node);
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for child in node.children() {
        if child.is_text() {
            push_container_text(&mut pending, child.text().unwrap_or(""));
        } else if child.is_element() {
            match child.tag_name().name() {
                "title" => {
                    flush_inlines(&mut pending, &mut out);
                    let mut heading = title_heading(ctx, child, level, depth)?;
                    attach_heading_anchor(&mut heading, anchor.take());
                    out.push(heading);
                }
                "section" => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_section(ctx, child, level.saturating_add(1), depth)?);
                }
                name if is_block(name) => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_block(ctx, child, level.saturating_add(1), depth)?);
                }
                _ => inline_element(ctx, child, Style::PLAIN, &mut pending, depth)?,
            }
        }
    }
    flush_inlines(&mut pending, &mut out);
    if let Some(id) = anchor {
        attach_anchor(&mut out, id);
    }
    Ok(out)
}

/// 容器 (epigraph/cite/annotation/未知包装元素) 的子节点按块/内联分流.
fn convert_container<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    level: u8,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for child in node.children() {
        if child.is_text() {
            push_container_text(&mut pending, child.text().unwrap_or(""));
        } else if child.is_element() {
            let name = child.tag_name().name();
            if is_block(name) {
                flush_inlines(&mut pending, &mut out);
                out.extend(convert_block(ctx, child, level, depth)?);
            } else {
                inline_element(ctx, child, Style::PLAIN, &mut pending, depth)?;
            }
        }
    }
    flush_inlines(&mut pending, &mut out);
    Ok(out)
}

fn convert_block<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    level: u8,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    match node.tag_name().name() {
        "section" => convert_section(ctx, node, level, depth),
        "title" => Ok(vec![title_heading(ctx, node, level, depth)?]),
        "p" | "v" | "subtitle" | "text-author" => Ok(vec![paragraph(ctx, node, depth)?]),
        "empty-line" => Ok(Vec::new()),
        "epigraph" | "cite" | "annotation" => {
            let mut inner = convert_container(ctx, node, level, depth)?;
            if inner.is_empty() {
                return Ok(Vec::new());
            }
            if let Some(id) = element_id(node) {
                attach_anchor(&mut inner, id);
            }
            Ok(vec![Block::BlockQuote(inner)])
        }
        "poem" => convert_poem(ctx, node, level, depth),
        "stanza" => convert_stanza(ctx, node, depth),
        "image" => {
            let mut inlines = Vec::new();
            inline_element(ctx, node, Style::PLAIN, &mut inlines, depth)?;
            Ok(vec![Block::Paragraph(inlines)])
        }
        "table" => Ok(convert_table(ctx, node, depth)?
            .map(Block::Table)
            .into_iter()
            .collect()),
        _ => convert_container(ctx, node, level, depth),
    }
}

/// poem: 诗题退成普通段落 (不进 TOC), stanza 保留为带 LineBreak 的诗行.
fn convert_poem<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    level: u8,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    let mut inner = Vec::new();
    for child in node.children() {
        if child.is_text() {
            let text = child.text().unwrap_or("");
            if !text.trim().is_empty() {
                inner.push(Block::Paragraph(vec![Inline::plain(text)]));
            }
            continue;
        }
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            "title" => {
                let content = title_content(ctx, child, depth)?;
                inner.push(Block::Paragraph(content));
            }
            "stanza" => inner.extend(convert_stanza(ctx, child, depth)?),
            _ => inner.extend(convert_block(ctx, child, level, depth)?),
        }
    }
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(id) = element_id(node) {
        attach_anchor(&mut inner, id);
    }
    Ok(vec![Block::BlockQuote(inner)])
}

fn convert_stanza<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    let mut lines = Vec::new();
    if let Some(id) = element_id(node) {
        lines.push(Inline::Anchor(id.to_string()));
    }
    let mut count = 0usize;
    for child in node.children() {
        if child.is_text() {
            let text = child.text().unwrap_or("");
            if text.trim().is_empty() {
                continue;
            }
            if count > 0 {
                lines.push(Inline::LineBreak);
            }
            push_text(&mut lines, text, Style::PLAIN);
            count += 1;
            continue;
        }
        if !child.is_element() {
            continue;
        }
        if child.tag_name().name() == "empty-line" {
            continue;
        }
        if count > 0 {
            lines.push(Inline::LineBreak);
        }
        if let Some(id) = element_id(child) {
            lines.push(Inline::Anchor(id.to_string()));
        }
        collect_inlines(ctx, child, Style::PLAIN, &mut lines, depth)?;
        count += 1;
    }
    Ok(if count == 0 {
        Vec::new()
    } else {
        vec![Block::Paragraph(lines)]
    })
}

/* ---------- 标题、段落、表格 ---------- */

fn title_heading<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    level: u8,
    depth: usize,
) -> Result<Block, Error> {
    let content = title_content(ctx, node, depth)?;
    Ok(Block::Heading {
        level,
        anchor: None,
        content,
    })
}

/// `<title>` 的内联内容: 多个 `<p>` 之间用一个 LineBreak; title 自身 id
/// 保留为内联锚点 (section id 另行落到 heading.anchor).
fn title_content<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Vec<Inline>, Error> {
    let depth = check_depth(depth)?;
    let mut content = Vec::new();
    if let Some(id) = element_id(node) {
        content.push(Inline::Anchor(id.to_string()));
    }
    let mut paragraphs = 0usize;
    for child in node.children() {
        if child.is_text() {
            let text = child.text().unwrap_or("");
            if !text.trim().is_empty() {
                push_text(&mut content, text, Style::PLAIN);
            }
        } else if child.is_element() {
            match child.tag_name().name() {
                "empty-line" => {}
                "p" | "v" => {
                    if paragraphs > 0 {
                        content.push(Inline::LineBreak);
                    }
                    if let Some(id) = element_id(child) {
                        content.push(Inline::Anchor(id.to_string()));
                    }
                    collect_inlines(ctx, child, Style::PLAIN, &mut content, depth)?;
                    paragraphs += 1;
                }
                _ => inline_element(ctx, child, Style::PLAIN, &mut content, depth)?,
            }
        }
    }
    Ok(content)
}

fn paragraph<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Block, Error> {
    let depth = check_depth(depth)?;
    let mut inlines = Vec::new();
    if let Some(id) = element_id(node) {
        inlines.push(Inline::Anchor(id.to_string()));
    }
    collect_inlines(ctx, node, Style::PLAIN, &mut inlines, depth)?;
    Ok(Block::Paragraph(inlines))
}

fn convert_table<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Option<Table>, Error> {
    let depth = check_depth(depth)?;
    let mut grid = Grid::new();
    let mut header_rows = 0usize;
    let mut header_run = true;
    for row in node
        .children()
        .filter(|child| child.is_element() && child.tag_name().name() == "tr")
    {
        grid.next_row();
        let mut any_cell = false;
        let mut all_header = true;
        for cell_node in row.children().filter(Node::is_element) {
            let name = cell_node.tag_name().name();
            if name != "td" && name != "th" {
                continue;
            }
            any_cell = true;
            if name != "th" {
                all_header = false;
            }
            grid.place(convert_cell(ctx, cell_node, depth)?, &ctx.table_positions)?;
        }
        if any_cell && header_run && all_header {
            header_rows += 1;
        } else {
            header_run = false;
        }
    }
    let mut table = grid.finish(&ctx.table_positions)?;
    if table.grid.is_empty() {
        return Ok(None);
    }
    table.header_rows = header_rows.min(table.grid.len());
    if let Some(id) = element_id(node) {
        attach_table_anchor(&mut table, id);
    }
    Ok(Some(table))
}

fn convert_cell<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Cell, Error> {
    let depth = check_depth(depth)?;
    let mut blocks = convert_container(ctx, node, 1, depth)?;
    if let Some(id) = element_id(node) {
        attach_anchor(&mut blocks, id);
    }
    Ok(Cell::spanning(
        blocks,
        span_attribute(node, "colspan"),
        span_attribute(node, "rowspan"),
    ))
}

fn span_attribute<'a, 'input>(node: Node<'a, 'input>, name: &str) -> u32 {
    node.attribute(name)
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(1)
}

/// 表格网格: 在分配之前按 area 收费, span 重叠时收缩到先占位置, 保持
/// 每个 covered 位置恰好由所属 origin 覆盖的 model 不变量.
struct Grid {
    rows: Vec<Vec<CellSlot>>,
    pending: HashMap<(usize, usize), (usize, usize)>,
}

impl Grid {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            pending: HashMap::new(),
        }
    }

    fn next_row(&mut self) {
        self.rows.push(Vec::new());
    }

    fn place(&mut self, cell: Cell, budget: &BudgetCell<u64>) -> Result<(), Error> {
        let area = u64::from(cell.col_span.max(1)) * u64::from(cell.row_span.max(1));
        let used = budget.get().saturating_add(area);
        if used > MAX_TABLE_POSITIONS {
            return Err(Error::Limit("FB2 table expansion"));
        }
        budget.set(used);
        if self.rows.is_empty() {
            self.rows.push(Vec::new());
        }
        let row = self.rows.len() - 1;
        loop {
            let col = self.rows[row].len();
            let Some((origin_row, origin_col)) = self.pending.remove(&(row, col)) else {
                break;
            };
            self.rows[row].push(CellSlot::Covered {
                origin_row,
                origin_col,
            });
        }
        let col = self.rows[row].len();
        let mut col_span = cell.col_span.max(1) as usize;
        let mut row_span = cell.row_span.max(1) as usize;
        for offset in 1..col_span {
            if self.pending.contains_key(&(row, col + offset)) {
                col_span = offset;
                break;
            }
        }
        'shrink: for row_offset in 1..row_span {
            for col_offset in 0..col_span {
                if self
                    .pending
                    .contains_key(&(row + row_offset, col + col_offset))
                {
                    row_span = row_offset;
                    break 'shrink;
                }
            }
        }
        self.rows[row].push(CellSlot::Origin(Cell {
            blocks: cell.blocks,
            col_span: col_span as u32,
            row_span: row_span as u32,
        }));
        for row_offset in 0..row_span {
            for col_offset in 0..col_span {
                if row_offset == 0 && col_offset == 0 {
                    continue;
                }
                self.pending
                    .insert((row + row_offset, col + col_offset), (row, col));
            }
        }
        Ok(())
    }

    fn finish(self, budget: &BudgetCell<u64>) -> Result<Table, Error> {
        let Grid { mut rows, pending } = self;
        let mut by_row: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(row, col) in pending.keys() {
            if row < rows.len() {
                by_row.entry(row).or_default().push(col);
            }
        }
        let mut groups: Vec<(usize, Vec<usize>)> = by_row.into_iter().collect();
        groups.sort_unstable_by_key(|(row, _)| *row);
        for (row, mut cols) in groups {
            cols.sort_unstable();
            for col in cols {
                let gaps = col.saturating_sub(rows[row].len());
                if gaps > 0 {
                    let used = budget.get().saturating_add(gaps as u64);
                    if used > MAX_TABLE_POSITIONS {
                        return Err(Error::Limit("FB2 table expansion"));
                    }
                    budget.set(used);
                    for _ in 0..gaps {
                        rows[row].push(CellSlot::Origin(Cell::default()));
                    }
                }
                if let Some(&(origin_row, origin_col)) = pending.get(&(row, col)) {
                    rows[row].push(CellSlot::Covered {
                        origin_row,
                        origin_col,
                    });
                }
            }
        }
        while rows.last().is_some_and(|row| {
            row.iter().all(|slot| match slot {
                CellSlot::Origin(cell) => cell.is_empty(),
                CellSlot::Covered { .. } => true,
            })
        }) {
            rows.pop();
        }
        let row_count = rows.len();
        for (row_index, row) in rows.iter_mut().enumerate() {
            let width = row.len();
            for (col_index, slot) in row.iter_mut().enumerate() {
                if let CellSlot::Origin(cell) = slot {
                    cell.row_span = cell.row_span.min((row_count - row_index) as u32);
                    cell.col_span = cell.col_span.min((width - col_index) as u32);
                }
            }
        }
        Ok(Table {
            grid: rows,
            header_rows: 0,
            kind: TableKind::Data,
        })
    }
}

/* ---------- 内联转换 ---------- */

fn collect_inlines<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    style: Style,
    out: &mut Vec<Inline>,
    depth: usize,
) -> Result<(), Error> {
    for child in node.children() {
        if child.is_text() {
            push_text(out, child.text().unwrap_or(""), style);
        } else if child.is_element() {
            inline_element(ctx, child, style, out, depth)?;
        }
    }
    Ok(())
}

fn inline_element<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    style: Style,
    out: &mut Vec<Inline>,
    depth: usize,
) -> Result<(), Error> {
    let depth = check_depth(depth)?;
    if let Some(id) = element_id(node) {
        out.push(Inline::Anchor(id.to_string()));
    }
    match node.tag_name().name() {
        "strong" => collect_inlines(
            ctx,
            node,
            Style {
                bold: true,
                ..style
            },
            out,
            depth,
        ),
        "emphasis" => collect_inlines(
            ctx,
            node,
            Style {
                italic: true,
                ..style
            },
            out,
            depth,
        ),
        "strikethrough" => collect_inlines(
            ctx,
            node,
            Style {
                strike: true,
                ..style
            },
            out,
            depth,
        ),
        "code" => collect_inlines(
            ctx,
            node,
            Style {
                code: true,
                ..style
            },
            out,
            depth,
        ),
        "a" => {
            let mut content = Vec::new();
            collect_inlines(ctx, node, style, &mut content, depth)?;
            if content.is_empty() {
                return Ok(());
            }
            match link_target(node) {
                Some(target) => out.push(Inline::Link { content, target }),
                None => out.extend(content),
            }
            Ok(())
        }
        "image" => {
            out.push(Inline::Image {
                alt: node
                    .attribute("alt")
                    .or_else(|| node.attribute("title"))
                    .unwrap_or_default()
                    .to_string(),
                source: image_source(ctx, node),
            });
            Ok(())
        }
        "empty-line" => {
            out.push(Inline::LineBreak);
            Ok(())
        }
        // sub/sup/style 及未建模的内联包装: 不执行 markup, 文本与既有样式保留.
        _ => collect_inlines(ctx, node, style, out, depth),
    }
}

fn push_text(out: &mut Vec<Inline>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text {
        text: previous,
        style: previous_style,
    }) = out.last_mut()
        && *previous_style == style
    {
        previous.push_str(text);
        return;
    }
    out.push(Inline::Text {
        text: text.to_string(),
        style,
    });
}

fn push_container_text(pending: &mut Vec<Inline>, text: &str) {
    if pending.is_empty() && text.trim().is_empty() {
        return;
    }
    push_text(pending, text, Style::PLAIN);
}

fn flush_inlines(pending: &mut Vec<Inline>, out: &mut Vec<Block>) {
    if !pending.is_empty() {
        out.push(Block::Paragraph(std::mem::take(pending)));
    }
}

/* ---------- 锚点 ---------- */

fn attach_heading_anchor(block: &mut Block, id: Option<&str>) {
    let Some(id) = id else {
        return;
    };
    if let Block::Heading {
        anchor, content, ..
    } = block
    {
        if anchor.is_none() {
            *anchor = Some(id.to_string());
        } else {
            content.insert(0, Inline::Anchor(id.to_string()));
        }
    }
}

/// 把源 id 落到既有块上: 段落/标题用内联锚点, 引用块递归, 其它块型
/// (表格/列表等没有内联槽位) 前面补一个纯锚点段落.
fn attach_anchor(blocks: &mut Vec<Block>, id: &str) {
    if let Some(first) = blocks.first_mut() {
        match first {
            Block::Paragraph(inlines) => {
                inlines.insert(0, Inline::Anchor(id.to_string()));
                return;
            }
            Block::Heading {
                anchor, content, ..
            } => {
                if anchor.is_none() {
                    *anchor = Some(id.to_string());
                } else {
                    content.insert(0, Inline::Anchor(id.to_string()));
                }
                return;
            }
            Block::BlockQuote(inner) => {
                attach_anchor(inner, id);
                return;
            }
            _ => {}
        }
    }
    blocks.insert(0, Block::Paragraph(vec![Inline::Anchor(id.to_string())]));
}

fn attach_table_anchor(table: &mut Table, id: &str) {
    for row in &mut table.grid {
        for slot in row {
            if let CellSlot::Origin(cell) = slot {
                attach_anchor(&mut cell.blocks, id);
                return;
            }
        }
    }
}

/* ---------- 链接与图片 ---------- */

/// href 只认 xlink 命名空间与非限定属性两种来源; 不按前缀文本匹配.
fn href<'a, 'input>(node: Node<'a, 'input>) -> Option<&'a str> {
    if let Some(value) = node.attribute((XLINK_NAMESPACE, "href")) {
        return Some(value);
    }
    node.attributes()
        .find(|attribute| attribute.namespace().is_none() && attribute.name() == "href")
        .map(|attribute| attribute.value())
}

fn fragment_id(href: &str) -> Option<Cow<'_, str>> {
    let fragment = href
        .strip_prefix('#')
        .filter(|fragment| !fragment.is_empty())?;
    percent_encoding::percent_decode_str(fragment)
        .decode_utf8()
        .ok()
}

fn link_target<'a, 'input>(node: Node<'a, 'input>) -> Option<LinkTarget> {
    let href = href(node)?;
    if href.is_empty() {
        return None;
    }
    if href.starts_with('#') {
        return fragment_id(href).map(|fragment| LinkTarget::Anchor(fragment.into_owned()));
    }
    if has_uri_scheme(href) {
        Some(LinkTarget::External(href.to_string()))
    } else {
        Some(LinkTarget::Relative(href.to_string()))
    }
}

fn image_source<'a, 'input>(ctx: &Ctx<'a>, node: Node<'a, 'input>) -> ImageSource {
    let Some(href) = href(node) else {
        return ImageSource::Unavailable;
    };
    if let Some(fragment) = fragment_id(href) {
        return ctx
            .assets
            .get(fragment.as_ref())
            .map_or(ImageSource::Unavailable, |id| ImageSource::Asset(*id));
    }
    if is_http_url(href) {
        ImageSource::External(href.to_string())
    } else {
        ImageSource::Unavailable
    }
}

/// RFC 3986 scheme 前缀 (字母开头, 后接字母/数字/`+`/`-`/`.`, 以 `:` 结束).
fn has_uri_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    for ch in chars {
        if ch == ':' {
            return true;
        }
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')) {
            return false;
        }
    }
    false
}

fn is_http_url(value: &str) -> bool {
    value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

/* ---------- 元素文本与工具 ---------- */

fn direct_child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

fn element_id<'a, 'input>(node: Node<'a, 'input>) -> Option<&'a str> {
    node.attribute("id").filter(|id| !id.is_empty())
}

fn is_notes_body(node: Node<'_, '_>) -> bool {
    node.attribute("name")
        .is_some_and(|name| name.eq_ignore_ascii_case("notes"))
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "title"
            | "subtitle"
            | "p"
            | "v"
            | "text-author"
            | "empty-line"
            | "section"
            | "epigraph"
            | "cite"
            | "poem"
            | "stanza"
            | "image"
            | "table"
            | "annotation"
            | "body"
    )
}

/// 元素的全部文本子节点 (注释/PI 不算内容); 只有一个文本子节点时不复制.
fn text_content<'a, 'input>(node: Node<'a, 'input>) -> Cow<'a, str> {
    let mut single: Option<&'a str> = None;
    let mut joined = String::new();
    let mut multiple = false;
    for child in node.children().filter(Node::is_text) {
        let text = child.text().unwrap_or("");
        if multiple {
            joined.push_str(text);
        } else if let Some(first) = single {
            joined.push_str(first);
            joined.push_str(text);
            multiple = true;
        } else {
            single = Some(text);
        }
    }
    if multiple {
        Cow::Owned(joined)
    } else {
        Cow::Borrowed(single.unwrap_or(""))
    }
}

fn check_depth(depth: usize) -> Result<usize, Error> {
    let depth = depth + 1;
    if depth > MAX_DEPTH {
        return Err(Error::Limit("FB2 nesting depth"));
    }
    Ok(depth)
}

/* ---------- 注释 (尾注) ---------- */

/// 注释段仍在原 body 位置, 保留源 id 与标题文本, 不强制移至整本末尾或重编号.
/// section 标题降为段落, 避免每条注释都成为 TOC 章节.
fn convert_note_blocks<'a, 'input>(
    ctx: &Ctx<'a>,
    node: Node<'a, 'input>,
    depth: usize,
) -> Result<Vec<Block>, Error> {
    let depth = check_depth(depth)?;
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for child in node.children() {
        if child.is_text() {
            push_container_text(&mut pending, child.text().unwrap_or(""));
        } else if child.is_element() {
            match child.tag_name().name() {
                "title" => {
                    flush_inlines(&mut pending, &mut out);
                    out.push(Block::Paragraph(title_content(ctx, child, depth)?));
                }
                "section" => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_note_blocks(ctx, child, depth)?);
                }
                name if is_block(name) => {
                    flush_inlines(&mut pending, &mut out);
                    out.extend(convert_block(ctx, child, 2, depth)?);
                }
                _ => inline_element(ctx, child, Style::PLAIN, &mut pending, depth)?,
            }
        }
    }
    flush_inlines(&mut pending, &mut out);
    if let Some(id) = element_id(node) {
        attach_anchor(&mut out, id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anydoc::model::inlines_to_plain_text;

    /// 70 字节、结构完整 (IHDR/IDAT/IEND) 的 1x1 PNG.
    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    fn book(inner: &str) -> Vec<u8> {
        format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
                "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\"",
                " xmlns:l=\"http://www.w3.org/1999/xlink\">",
                "{inner}",
                "</FictionBook>"
            ),
            inner = inner
        )
        .into_bytes()
    }

    fn all_links(blocks: &[Block]) -> Vec<&Inline> {
        fn walk_inlines<'a>(inlines: &'a [Inline], out: &mut Vec<&'a Inline>) {
            for inline in inlines {
                if let Inline::Link { content, .. } = inline {
                    out.push(inline);
                    walk_inlines(content, out);
                }
            }
        }
        fn walk_blocks<'a>(blocks: &'a [Block], out: &mut Vec<&'a Inline>) {
            for block in blocks {
                match block {
                    Block::Paragraph(inlines)
                    | Block::Heading {
                        content: inlines, ..
                    } => {
                        walk_inlines(inlines, out);
                    }
                    Block::BlockQuote(inner) => walk_blocks(inner, out),
                    Block::List(list) => {
                        for item in &list.items {
                            walk_blocks(&item.blocks, out);
                        }
                    }
                    Block::Table(table) => {
                        for row in &table.grid {
                            for slot in row {
                                if let CellSlot::Origin(cell) = slot {
                                    walk_blocks(&cell.blocks, out);
                                }
                            }
                        }
                    }
                    Block::CodeBlock { .. } | Block::Rule => {}
                }
            }
        }
        let mut out = Vec::new();
        walk_blocks(blocks, &mut out);
        out
    }

    fn all_text(blocks: &[Block]) -> String {
        let mut text = String::new();
        for block in blocks {
            match block {
                Block::Paragraph(inlines)
                | Block::Heading {
                    content: inlines, ..
                } => {
                    text.push_str(&inlines_to_plain_text(inlines));
                }
                Block::BlockQuote(inner) => text.push_str(&all_text(inner)),
                Block::List(list) => {
                    for item in &list.items {
                        text.push_str(&all_text(&item.blocks));
                    }
                }
                Block::Table(table) => {
                    for row in &table.grid {
                        for slot in row {
                            if let CellSlot::Origin(cell) = slot {
                                text.push_str(&all_text(&cell.blocks));
                            }
                        }
                    }
                }
                Block::CodeBlock { text: code, .. } => text.push_str(code),
                Block::Rule => {}
            }
        }
        text
    }

    fn first_paragraph(blocks: &[Block]) -> &[Inline] {
        blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(inlines) => Some(inlines.as_slice()),
                _ => None,
            })
            .expect("document has a paragraph")
    }

    #[test]
    fn mixed_inline_runs_keep_text_and_styles() {
        let xml = book(concat!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>",
            "<p id=\"p1\">A<strong>B</strong><emphasis>C</emphasis>",
            "<strikethrough>D</strikethrough><code>E</code><sub>F</sub>G</p>",
            "</section></body>"
        ));
        let doc = parse(&xml).expect("parse mixed inline content");
        let paragraph = first_paragraph(&doc.blocks);
        assert_eq!(inlines_to_plain_text(paragraph), "ABCDEFG");
        let styled: Vec<(&str, Style)> = paragraph
            .iter()
            .filter_map(|inline| match inline {
                Inline::Text { text, style } => Some((text.as_str(), *style)),
                _ => None,
            })
            .collect();
        assert_eq!(styled[0], ("A", Style::PLAIN));
        assert!(
            styled
                .iter()
                .any(|(text, style)| *text == "B" && style.bold)
        );
        assert!(
            styled
                .iter()
                .any(|(text, style)| *text == "C" && style.italic)
        );
        assert!(
            styled
                .iter()
                .any(|(text, style)| *text == "D" && style.strike)
        );
        assert!(
            styled
                .iter()
                .any(|(text, style)| *text == "E" && style.code)
        );
        assert!(
            matches!(paragraph.first(), Some(Inline::Anchor(id)) if id.as_str() == "p1"),
            "paragraph source anchor is preserved before the text"
        );
    }

    #[test]
    fn duplicate_and_nested_source_ids_are_preserved() {
        let xml = book(concat!(
            "<body><section id=\"dup\"><title><p>One</p></title>",
            "<p id=\"dup\">text</p></section></body>"
        ));
        let doc = parse(&xml).expect("parse duplicate ids");
        let heading = doc.blocks.iter().find_map(|block| match block {
            Block::Heading { level, anchor, .. } => Some((*level, anchor.clone())),
            _ => None,
        });
        assert_eq!(heading, Some((2, Some("dup".to_string()))));
        let paragraph = first_paragraph(&doc.blocks);
        assert!(matches!(paragraph.first(), Some(Inline::Anchor(id)) if id.as_str() == "dup"));
    }

    #[test]
    fn xlink_and_unqualified_hrefs_resolve_images_and_links() {
        let png = PNG_1X1;
        let xml = book(&format!(
            concat!(
                "<description><title-info><book-title>T</book-title>",
                "<coverpage><image l:href=\"#pic\"/></coverpage></title-info></description>",
                "<body><section id=\"s1\"><title><p>Глава</p></title>",
                "<p><image l:href=\"#pic\"/><a href=\"#s1\">up</a>",
                "<a l:href=\"#s1\">again</a></p></section></body>",
                "<binary id=\"pic\" content-type=\"image/png\">{png}</binary>"
            ),
            png = png
        ));
        let doc = parse(&xml).expect("parse xlink content");
        assert_eq!(doc.assets.len(), 1);
        assert_eq!(doc.assets[0].media_type, "image/png");
        assert!(!doc.assets[0].bytes.is_empty());

        let has_embedded_asset = doc.blocks.iter().any(|block| match block {
            Block::Paragraph(inlines) => inlines.iter().any(|inline| {
                matches!(
                    inline,
                    Inline::Image { source: ImageSource::Asset(id), .. } if *id == AssetId(0)
                )
            }),
            _ => false,
        });
        assert!(
            has_embedded_asset,
            "referenced binary becomes an embedded asset"
        );

        let targets: Vec<String> = all_links(&doc.blocks)
            .into_iter()
            .filter_map(|inline| match inline {
                Inline::Link {
                    target: LinkTarget::Anchor(id),
                    ..
                } => Some(id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(targets, vec!["s1".to_string(), "s1".to_string()]);
    }

    #[test]
    fn missing_binary_is_visible_and_unreferenced_binary_is_not_decoded() {
        let xml = book(concat!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>",
            "<p><image l:href=\"#missing\"/></p></section></body>",
            "<binary id=\"unused\">!!! not base64 !!!</binary>"
        ));
        let doc = parse(&xml).expect("unused binaries must not be decoded");
        assert!(doc.assets.is_empty());
        let unavailable = doc.blocks.iter().any(|block| match block {
            Block::Paragraph(inlines) => inlines.iter().any(|inline| {
                matches!(
                    inline,
                    Inline::Image {
                        source: ImageSource::Unavailable,
                        ..
                    }
                )
            }),
            _ => false,
        });
        assert!(unavailable, "missing image keeps a visible placeholder");

        let broken = book(concat!(
            "<body><p><image l:href=\"#bad\"/></p></body>",
            "<binary id=\"bad\">!!!!</binary>"
        ));
        assert!(matches!(parse(&broken), Err(Error::Invalid(_))));
    }

    #[test]
    fn duplicate_binary_ids_first_one_wins() {
        let png = PNG_1X1;
        let xml = book(&format!(
            concat!(
                "<body><p><image l:href=\"#pic\"/></p></body>",
                "<binary id=\"pic\">{png}</binary>",
                "<binary id=\"pic\">!!!!</binary>"
            ),
            png = png
        ));
        let doc = parse(&xml).expect("first binary id wins");
        assert_eq!(doc.assets.len(), 1);
        assert_eq!(doc.assets[0].media_type, "image/png");
    }

    #[test]
    fn table_spans_build_a_consistent_grid() {
        let xml = book(concat!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>",
            "<table><tr><th>H1</th><th>H2</th></tr>",
            "<tr><td colspan=\"2\">wide</td></tr>",
            "<tr><td>a</td><td rowspan=\"2\">b</td></tr>",
            "<tr><td>c</td></tr></table></section></body>"
        ));
        let doc = parse(&xml).expect("parse table");
        let table = doc
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Table(table) => Some(table),
                _ => None,
            })
            .expect("table block");
        assert_eq!(table.header_rows, 1);
        assert_eq!(table.grid.len(), 4);
        assert!(matches!(
            table.grid[1][0],
            CellSlot::Origin(Cell {
                col_span: 2,
                row_span: 1,
                ..
            })
        ));
        assert!(matches!(
            table.grid[1][1],
            CellSlot::Covered {
                origin_row: 1,
                origin_col: 0
            }
        ));
        assert!(matches!(
            table.grid[3][1],
            CellSlot::Covered {
                origin_row: 2,
                origin_col: 1
            }
        ));
    }

    #[test]
    fn poem_stanza_and_cite_keep_text_without_extra_headings() {
        let xml = book(concat!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>",
            "<subtitle>Подзаголовок</subtitle>",
            "<cite><p>Цитата</p><text-author>Автор</text-author></cite>",
            "<poem><title><p>Стих</p></title>",
            "<stanza><v>первая</v><v>вторая</v></stanza>",
            "<text-author>Поэт</text-author></poem>",
            "</section></body>"
        ));
        let doc = parse(&xml).expect("parse poem and cite");
        let text = all_text(&doc.blocks);
        assert!(text.contains("Подзаголовок"));
        assert!(text.contains("Цитата"));
        assert!(text.contains("Автор"));
        assert!(text.contains("Стих"));
        assert!(text.contains("Поэт"));
        assert!(
            text.contains("первая\nвторая"),
            "stanza lines are separated by an inline line break"
        );
        let quotes = doc
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::BlockQuote(_)))
            .count();
        assert_eq!(quotes, 2, "cite and poem are quoted blocks");
        let headings = doc
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::Heading { .. }))
            .count();
        assert_eq!(
            headings, 1,
            "subtitle and poem title do not become TOC headings"
        );
    }

    #[test]
    fn oversized_table_span_is_rejected_before_expansion() {
        let xml = book(concat!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>",
            "<table><tr><td colspan=\"1000000\">x</td></tr></table>",
            "</section></body>"
        ));
        assert!(matches!(parse(&xml), Err(Error::Limit(_))));
    }

    #[test]
    fn table_expansion_budget_is_document_wide() {
        let xml = book(concat!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>",
            "<table><tr><td colspan=\"120000\">x</td></tr></table>",
            "<table><tr><td colspan=\"90000\">y</td></tr></table>",
            "</section></body>"
        ));
        assert!(
            matches!(parse(&xml), Err(Error::Limit(_))),
            "many tables cannot each spend the whole per-table budget"
        );
    }

    #[test]
    fn sparse_row_span_gaps_are_budgeted() {
        const COLUMNS: usize = 100;
        let budget = usize::try_from(MAX_TABLE_POSITIONS).expect("table budget fits usize");
        let rows = budget / COLUMNS + 100;
        let mut table = String::from("<table><tr>");
        table.push_str(&"<td>x</td>".repeat(COLUMNS));
        table.push_str(&format!("<td rowspan=\"{rows}\">tall</td></tr>"));
        table.push_str(&"<tr></tr>".repeat(rows - 1));
        table.push_str("</table>");
        let xml = book(&format!(
            "<body><section id=\"s1\"><title><p>Глава</p></title>{table}</section></body>"
        ));
        assert!(
            matches!(parse(&xml), Err(Error::Limit(_))),
            "gap cells behind a row span are charged before allocation"
        );
    }

    #[test]
    fn deep_inline_nesting_hits_the_depth_limit() {
        let mut nested = String::from("x");
        for _ in 0..(MAX_DEPTH * 2) {
            nested = format!("<strong>{nested}</strong>");
        }
        let xml = book(&format!("<body><p>{nested}</p></body>"));
        assert!(matches!(parse(&xml), Err(Error::Limit(_))));
    }

    #[test]
    fn windows_1251_and_utf16_bom_inputs_decode() {
        let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice(&book("<body><p>Привет</p></body>"));
        let doc = parse(&utf8_bom).expect("utf-8 BOM document");
        assert_eq!(all_text(&doc.blocks), "Привет");

        let cp1251_text = concat!(
            "<?xml version=\"1.0\" encoding=\"windows-1251\"?>",
            "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">",
            "<body><p>Привет</p></body></FictionBook>"
        );
        let (bytes, _, had_errors) = encoding_rs::WINDOWS_1251.encode(cp1251_text);
        assert!(!had_errors);
        let doc = parse(&bytes).expect("windows-1251 document");
        assert_eq!(all_text(&doc.blocks), "Привет");

        let utf16_text = concat!(
            "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">",
            "<body><p>Привет</p></body></FictionBook>"
        );
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in utf16_text.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let doc = parse(&utf16).expect("utf-16 BOM document");
        assert_eq!(all_text(&doc.blocks), "Привет");
    }

    #[test]
    fn body_title_is_used_once_and_book_title_is_the_fallback() {
        let with_body_title = book(concat!(
            "<description><title-info><book-title>Заголовок</book-title>",
            "<annotation><p>Аннотация</p></annotation></title-info></description>",
            "<body><title><p>Заголовок</p></title>",
            "<section id=\"s1\"><title><p>Глава</p></title></section></body>"
        ));
        let doc = parse(&with_body_title).expect("parse titled body");
        let h1: Vec<String> = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Heading {
                    level: 1, content, ..
                } => Some(inlines_to_plain_text(content)),
                _ => None,
            })
            .collect();
        assert_eq!(h1, vec!["Заголовок".to_string()]);
        assert!(matches!(doc.blocks[1], Block::BlockQuote(_)));

        let without_body_title = book(concat!(
            "<description><title-info><book-title>Заголовок</book-title>",
            "</title-info></description>",
            "<body><section id=\"s1\"><title><p>Глава</p></title></section></body>"
        ));
        let doc = parse(&without_body_title).expect("parse fallback title");
        assert_eq!(
            all_text(&doc.blocks[..1]),
            "Заголовок",
            "description book-title becomes the heading when the body has none"
        );
        assert!(matches!(doc.blocks[0], Block::Heading { level: 1, .. }));

        let distinct_part_title = book(concat!(
            "<description><title-info><book-title>Book</book-title>",
            "</title-info></description>",
            "<body><title><p>Part One</p></title>",
            "<section id=\"s1\"><title><p>Глава</p></title></section></body>"
        ));
        let doc = parse(&distinct_part_title).expect("parse part title");
        assert_eq!(
            all_text(&doc.blocks[..1]),
            "Book",
            "the book title is shown before a differently named body title"
        );
        assert!(
            all_text(&doc.blocks).contains("Part One"),
            "a distinct body title is not dropped"
        );
    }

    #[test]
    fn unsupported_root_or_namespace_is_typed_failure() {
        let wrong_root = b"<Book><body/></Book>";
        assert!(matches!(
            parse(wrong_root),
            Err(Error::Unsupported("not a FictionBook document"))
        ));
        let wrong_namespace = br#"<FictionBook xmlns="urn:other"><body/></FictionBook>"#;
        assert!(matches!(parse(wrong_namespace), Err(Error::Unsupported(_))));
        assert!(matches!(parse(b"<FictionBook>"), Err(Error::Invalid(_))));
    }
}
