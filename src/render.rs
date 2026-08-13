//! HTML 生成 (maud).

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use maud::{DOCTYPE, Markup, html};

use crate::listing::SearchResult;
use crate::meta::{
    FileMeta, display_path_markup, display_path_text, display_segment, encode_relative_path,
    escape_os_name, format_time,
};
use crate::sort::{SortBy, SortColumn, SortOrder};

/// 目录列表页面 (可信 UI): 共享页面骨架 + 面包屑 + 辅助技术可读的隐藏 h1 +
/// 6 列 striped 表格 + 网格 + 预览面板. native_token 为 Some 时 head 嵌入 token meta
/// 并渲染 native-open 控制; None 时无 token meta 也无 native 按钮.
pub fn dir_page(
    root: &Path,
    path: &Path,
    files: &[FileMeta],
    sort_by: SortBy,
    native_token: Option<&str>,
) -> Markup {
    let title = dir_title(root, path);
    let content = html! {
        h1 class="cb-visually-hidden" { "目录 " (title) " 中的文件" }
        (dir_table(root, path, files, sort_by, native_token))
        (entry_grid(files))
    };
    page_shell(
        &title,
        native_token,
        breadcrumb_markup(root, path),
        content,
        "",
        None,
    )
}

/// 目录完整页 <title> / heading: root-relative 路径逐段经显示 codec 转义 join.
fn dir_title(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(Path::new(""));
    format!("/{}", display_path_text(rel))
}

/// 搜索页 (可信 UI): 同一共享骨架 + 面包屑 (root 链接 + 纯文本 搜索) + 4 列结果表
/// (名称/位置/大小/修改时间) + 网格 + 截断/空结果提示. 行序即 SearchResult 的
/// 确定性 BFS 顺序.
pub fn search_page(
    root: &Path,
    query: &str,
    result: &SearchResult,
    native_token: Option<&str>,
) -> Markup {
    let content = html! {
        h6.cb-heading { "搜索：" (query) }
        @if result.truncated {
            p.cb-notice { "结果超过 500 条，已截断显示前 500 条。" }
        }
        @if result.entries.is_empty() {
            p.cb-empty { "没有找到匹配的文件。" }
        } @else {
            (search_table(result, native_token))
            (entry_grid(&result.entries))
        }
    };
    let breadcrumb = html! {
        nav.cb-breadcrumb aria-label="面包屑" {
            a href="/" { (display_segment(root.file_name().unwrap_or_else(|| OsStr::new("/")))) }
            span.cb-crumb-sep { "/" }
            span.cb-current { "搜索" }
        }
    };
    page_shell(
        &format!("搜索：{query}"),
        native_token,
        breadcrumb,
        content,
        query,
        Some("/"),
    )
}

/// 目录 fragment: 恰好一个外层 wrapper (`<div class="cb-doc cb-dir-fragment">`) 携带
/// 当前目录的 ASCII encoded path (root 为合法空串); 无 doctype/head/style.
/// 迷你列表含名称+大小; 可操作条目渲染 anchor + 自身 encoded identity (供面板下钻);
/// 非 UTF-8 条目 display-only.
pub fn dir_fragment(root: &Path, path: &Path, files: &[FileMeta]) -> String {
    let rel = path.strip_prefix(root).unwrap_or(Path::new(""));
    let encoded = encode_relative_path(rel);
    let title = display_path_text(rel);
    let title_markup = display_path_markup(rel);
    let body = html! {
        ul class="cb-dir-mini" {
            @for f in files {
                li {
                    // 可操作条目: anchor + 自身 encoded identity (供面板内下钻);
                    // href 恒为 "/" + encoded (同一编码器), 非 UTF-8 条目 display-only
                    @if let Some(enc) = f.browser_path_encoded() {
                        a href=(format!("/{enc}")) data-native-path-encoded=(enc) {
                            (entry_label(f))
                        }
                    } @else {
                        span.cb-mini-label {
                            (entry_label(f))
                            span.cb-non-utf8 { "非 UTF-8 名称（仅显示）" }
                        }
                    }
                    span.cb-dir-size { (f.human_size()) }
                }
            }
        }
    };
    // wrapper 携带当前目录 encoded path (root 为合法空串); maud 不支持 Option 属性,
    // 用分支显式省略
    let wrapper = match encoded {
        Some(enc) => html! {
            div class="cb-doc cb-dir-fragment" data-native-path-encoded=(enc) title=(title) {
                span.cb-frag-title hidden { (title_markup) }
                (body)
            }
        },
        None => html! {
            div class="cb-doc cb-dir-fragment" title=(title) {
                span.cb-frag-title hidden { (title_markup) }
                (body)
            }
        },
    };
    wrapper.into_string()
}

/// 共享页面骨架 (目录页/搜索页共用): head (Materialize CSS / theme / chapbook-doc.css /
/// browser JS defer / title / viewport / 可选 token meta), 常驻 GET 搜索表单,
fn page_shell(
    title: &str,
    native_token: Option<&str>,
    breadcrumb: Markup,
    content: Markup,
    q: &str,
    parent_href: Option<&str>,
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no, maximum-scale=1, minimum-scale=1";
                title { (title) }
                link rel="stylesheet" href="/__/static/css/materialize.min.css";
                link rel="stylesheet" href="/__/static/css/chapbook-theme.css";
                link rel="stylesheet" href="/__/static/css/chapbook-doc.css";
                script src="/__/static/js/chapbook-browser.js" defer {}
                @if let Some(token) = native_token {
                    meta name="cb-native-open-token" content=(token);
                }
            }
            body class="cb-browser-page" {
                header.cb-topbar {
                    form.cb-search action="/__/search" method="get" role="search" {
                        input type="search" name="q" id="cb-search-q" placeholder="搜索文件" aria-label="搜索文件" value=(q) autocomplete="off";
                    }
                    div.cb-view-toggle role="group" aria-label="视图切换" {
                        button type="button" data-cb-view="list" aria-pressed="true" { "列表" }
                        button type="button" data-cb-view="grid" aria-pressed="false" { "网格" }
                    }
                }
                (breadcrumb)
                // 搜索页 Backspace 目标: root; 目录页由 ../ 行链接承担同一职责.
                @if let Some(parent) = parent_href {
                    main.cb-main data-cb-parent=(parent) {
                        (content)
                    }
                } @else {
                    main.cb-main {
                        (content)
                    }
                }
                aside.cb-preview id="cb-preview" hidden {
                    div.cb-preview-toolbar {
                        span.cb-preview-title id="cb-preview-title" { bdi dir="auto" {} }
                        span.cb-preview-actions {
                            button type="button" data-cb-action="full" title="当前 tab 打开" { "打开" }
                            button type="button" data-cb-action="new" title="新 tab 打开" { "新标签" }
                            button type="button" data-cb-action="download" title="下载" { "下载" }
                            @if native_token.is_some() {
                                button type="button" data-cb-action="native" title="本机默认应用打开" { "本机打开" }
                            }
                            button type="button" data-cb-action="close" title="关闭" { "✕" }
                            span.cb-native-feedback aria-live="polite" {}
                        }
                    }
                    div.cb-preview-content id="cb-preview-content" {}
                }
            }
        }
    }
}

/// 面包屑: 根节点 (root basename label, href "/"), 中间节点为逐级累计的 exact
/// UTF-8 路径 (共享编码器); 当前节点纯文本. 祖先路径非 UTF-8 时该段及其后
/// remainder display-only (不生成链接). 每 segment 经显示 codec + bdi 隔离.
fn breadcrumb_markup(root: &Path, path: &Path) -> Markup {
    let crumbs = breadcrumb_crumbs(root, path);
    let last = crumbs.len().saturating_sub(1);
    html! {
        nav.cb-breadcrumb aria-label="面包屑" {
            a href="/" { (display_segment(root.file_name().unwrap_or_else(|| OsStr::new("/")))) }
            @for (i, (seg, href)) in crumbs.iter().enumerate() {
                span.cb-crumb-sep { "/" }
                @if i == last {
                    span.cb-current { (display_segment(seg)) }
                } @else if let Some(h) = href {
                    a href=(h.clone()) { (display_segment(seg)) }
                } @else {
                    span.cb-current { (display_segment(seg)) }
                }
            }
        }
    }
}

/// 面包屑数据: (segment, 累计路径的 encoded href); 累计路径非 UTF-8 时 href 为 None.
fn breadcrumb_crumbs(root: &Path, path: &Path) -> Vec<(OsString, Option<String>)> {
    let mut crumbs = Vec::new();
    if let Ok(rel) = path.strip_prefix(root) {
        let mut acc = PathBuf::new();
        for seg in rel.iter() {
            acc.push(seg);
            let href = encode_relative_path(&acc).map(|encoded| format!("/{encoded}"));
            crumbs.push((seg.to_os_string(), href));
        }
    }
    crumbs
}

/// 目录表格: 6 列保持既有结构 (thead 排序链接 + tbody 条纹行 + 父行 colspan=6);
/// 行尾 action 放在最后单元格内, 不新增第 7 列.
fn dir_table(
    root: &Path,
    path: &Path,
    files: &[FileMeta],
    sort_by: SortBy,
    native_token: Option<&str>,
) -> Markup {
    let parent_href = path
        .parent()
        .and_then(|parent| parent.strip_prefix(root).ok())
        .and_then(encode_relative_path)
        .map(|encoded| {
            if encoded.is_empty() {
                "/".to_string()
            } else {
                format!("/{encoded}")
            }
        });
    html! {
        section.cb-view.cb-view-list id="cb-view-list" {
            table class="striped" {
                // 表头放进 thead: 浏览器会把 table 下的裸 tr 包进隐式 tbody,
                // 导致表头被条纹染色 (v2 的灰色带就是这么来的)
                thead { (table_header(sort_by)) }
                tbody {
                    @if path != root {
                        // colspan 覆盖全行: 单 td 行的条纹/背景只会染到单元格区域;
                        // 绝对 root-relative URL 同供原生 ../ 链接与 JS Backspace,
                        // 避免 slashless 当前 URL 把相对 "../" 多解析一级。
                        tr {
                            td colspan="6" {
                                @if let Some(href) = &parent_href {
                                    a href=(href) data-cb-parent { "../" }
                                } @else {
                                    "../"
                                }
                            }
                        }
                    }
                    @for f in files {
                        (entry_row(
                            f,
                            vec![
                                html! { (f.type_str()) },
                                html! { (f.human_size()) },
                                html! { (format_time(f.last_modified_time)) },
                                html! { (format_time(f.last_access_time)) },
                            ],
                            html! { (format_time(f.creation_time)) },
                            native_token,
                        ))
                    }
                }
            }
        }
    }
}

/// 搜索结果表: 4 列 (名称/位置/大小/修改时间). 行尾 action 与目录表同一簇
/// (最后单元格内). 位置列: 父目录整条 UTF-8 时链接, 否则逐段显示 codec.
fn search_table(result: &SearchResult, native_token: Option<&str>) -> Markup {
    html! {
        section.cb-view.cb-view-list id="cb-view-list" {
            table class="striped" {
                thead {
                    tr { th { "名称" } th { "位置" } th { "大小" } th { "修改时间" } }
                }
                tbody {
                    @for f in &result.entries {
                        (entry_row(
                            f,
                            vec![search_location_cell(f), html! { (f.human_size()) }],
                            html! { (format_time(f.last_modified_time)) },
                            native_token,
                        ))
                    }
                }
            }
        }
    }
}

/// 数据行 (目录表/搜索表共用): 行级 identity (data-browser-entry /
/// ASCII-only encoded path / 安全 label attributes), 名称单元格 (可操作 anchor
/// 或 display-only + 提示), 中间单元格, 以及放在最后单元格内的行尾 action 簇.
/// href() 为 None 的路径 (非 UTF-8) 是 display-only 行: 只渲染名称, 不产生锚点,
/// 更不能 emit href="" (空 href 点击会导航回当前目录).
fn entry_row(
    f: &FileMeta,
    middle: Vec<Markup>,
    last: Markup,
    native_token: Option<&str>,
) -> Markup {
    let label = entry_label_text(f);
    let encoded = f.browser_path_encoded();
    let body = html! {
        @if let Some(href) = f.href() {
            td { a href=(href) { (entry_label(f)) } }
        } @else {
            td {
                (entry_label(f))
                span.cb-non-utf8 { "非 UTF-8 名称（仅显示）" }
            }
        }
        @for cell in middle {
            td { (cell) }
        }
        td { (last) (entry_actions(f, native_token)) }
    };
    // 只有可操作路径携带 data-native-path-encoded (maud 不支持 Option 属性, 显式分支)
    match encoded {
        Some(enc) => html! {
            tr class="cb-row" data-browser-entry data-native-path-encoded=(enc) title=(label) aria-label=(label) {
                (body)
            }
        },
        None => html! {
            tr class="cb-row" data-browser-entry title=(label) aria-label=(label) {
                (body)
            }
        },
    }
}

/// 行尾 action 簇 (仅 actionable 行): 预览 / 新 tab (target=_blank rel=noopener) /
/// 下载 (?download=1, 仅文件 — 目录忽略 download 模式) / native-open (仅 token
/// 存在时). 只渲染于最后单元格内.
fn entry_actions(f: &FileMeta, native_token: Option<&str>) -> Markup {
    let Some(encoded) = f.browser_path_encoded() else {
        return html! {};
    };
    let label = entry_label_text(f);
    html! {
        span.cb-row-actions {
            button type="button" class="cb-action" data-cb-action="preview" title="预览" aria-label=(format!("预览 {label}")) { "◈" }
            a href=(format!("/{encoded}")) target="_blank" rel="noopener" title="新 tab 打开" aria-label=(format!("新 tab 打开 {label}")) { "↗" }
            @if !f.is_directory {
                a href=(format!("/{encoded}?download=1")) title="下载" aria-label=(format!("下载 {label}")) { "⤓" }
            }
            @if native_token.is_some() {
                button type="button" class="cb-action" data-cb-action="native" title="本机打开" aria-label=(format!("本机打开 {label}")) { "▣" }
            }
        }
    }
}

/// 网格视图: 与表格同一 entry 数据 (data-browser-entry / kind / encoded / label);
/// 图片用 lazy img + 精确 raw URL (无模式 query), 非图片/目录用扩展名 badge.
/// 默认 hidden, 由 JS 切换 (列表/网格都渲染).
fn entry_grid(files: &[FileMeta]) -> Markup {
    html! {
        section.cb-view.cb-view-grid id="cb-view-grid" hidden {
            div.cb-grid {
                @for f in files {
                    (grid_item(f))
                }
            }
        }
    }
}

fn grid_item(f: &FileMeta) -> Markup {
    let label = entry_label_text(f);
    let encoded = f.browser_path_encoded();
    let img_src = is_image_entry(f).then(|| f.href()).flatten();
    let body = html! {
        @if let Some(src) = img_src {
            div.cb-grid-thumb { img loading="lazy" decoding="async" src=(src) alt=""; }
        } @else {
            (grid_badge(f))
        }
        div.cb-grid-label {
            @if let Some(href) = f.href() {
                a href=(href) { (entry_label(f)) }
            } @else {
                (entry_label(f))
                span.cb-non-utf8 { "非 UTF-8 名称（仅显示）" }
            }
        }
        div.cb-grid-size { (f.human_size()) }
    };
    match encoded {
        Some(enc) => html! {
            div class="cb-grid-item" data-browser-entry data-native-path-encoded=(enc) title=(label) aria-label=(label) {
                (body)
            }
        },
        None => html! {
            div class="cb-grid-item" data-browser-entry title=(label) aria-label=(label) {
                (body)
            }
        },
    }
}

/// 网格 badge: 目录 "DIR"; 文件显示扩展名 (显示 codec), 无扩展名 "FILE".
fn grid_badge(f: &FileMeta) -> Markup {
    if f.is_directory {
        return html! { div.cb-grid-badge { "DIR" } };
    }
    let ext = f
        .relative_to_root
        .extension()
        .map(escape_os_name)
        .unwrap_or_default();
    let text = if ext.is_empty() {
        "FILE".to_string()
    } else {
        ext
    };
    html! { div.cb-grid-badge { (text) } }
}

/// 位置单元格 (搜索表): 父目录整条路径 UTF-8 时渲染链接 (顶层条目 -> "/"),
/// 否则逐段显示 codec 显示.
fn search_location_cell(f: &FileMeta) -> Markup {
    match f.relative_to_root.parent() {
        None => html! { "/" },
        Some(parent) if parent.as_os_str().is_empty() => html! { a href="/" { "/" } },
        Some(parent) => match encode_relative_path(parent) {
            Some(encoded) => html! {
                a href=(format!("/{encoded}")) { (path_label_markup(parent)) }
            },
            None => html! { (path_label_markup(parent)) },
        },
    }
}

/// 多 segment 路径的显示 markup: 逐段 display_segment (bdi 隔离), 分隔符在 bdi 外.
fn path_label_markup(path: &Path) -> Markup {
    html! {
        @for (i, seg) in path.iter().enumerate() {
            @if i > 0 { "/" }
            (display_segment(seg))
        }
    }
}

/// 行内文件名的 bdi 标记: 从 identity (relative_to_root) 取原始 basename,
/// 经显示 codec 转义并隔离方向性, 路径分隔符保持在 bdi 之外.
fn entry_label(f: &FileMeta) -> Markup {
    display_segment(
        f.relative_to_root
            .file_name()
            .unwrap_or_else(|| OsStr::new("")),
    )
}

/// 纯文本 label (title/aria-label 等 attribute): basename 经显示 codec,
/// 绝不出现 raw OsStr / Path::display / to_string_lossy.
fn entry_label_text(f: &FileMeta) -> String {
    escape_os_name(
        f.relative_to_root
            .file_name()
            .unwrap_or_else(|| OsStr::new("")),
    )
}

/// 网格缩略图判定: 仅图片扩展名 (png jpg jpeg gif webp svg avif ico,
/// 大小写不敏感, 集合与 routes::media_kind_for_path 的 image 分支一致).
/// 目录与非图片一律走扩展名 badge.
fn is_image_entry(f: &FileMeta) -> bool {
    if f.is_directory {
        return false;
    }
    matches!(
        f.relative_to_root
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "ico")
    )
}

/// 表头: 每列一个排序链接, 当前列带 ▲/▼ 箭头, 再次点击切换方向.
fn table_header(sort_by: SortBy) -> Markup {
    html! {
        tr {
            @for column in SortColumn::ALL {
                @let next_order = if column == sort_by.column && sort_by.order == SortOrder::Asc {
                    SortOrder::Desc
                } else {
                    SortOrder::Asc
                };
                @let arrow = if column == sort_by.column {
                    match sort_by.order {
                        SortOrder::Asc => "▲",
                        SortOrder::Desc => "▼",
                    }
                } else {
                    ""
                };
                th { a href=(format!("?sort={column}:{next_order}")) { (format!("{column}{arrow}")) } }
            }
        }
    }
}

/// 文档内容片段: 恰好一个外层 `<div class="cb-doc">…</div>`, 无 doctype/head/style.
/// 内容样式全部作用域在 `.cb-doc` 下 (chapbook-doc.css), 可嵌入宿主页面;
/// 完整页与未来 Fragment 路由共用此包装.
pub fn doc_fragment(body: &str) -> String {
    html! {
        div class="cb-doc" {
            (maud::PreEscaped(body))
        }
    }
    .into_string()
}

/// 文档/代码页面骨架: 自组 `<!DOCTYPE>` / head / title / body.
/// org 路径的页面骨架由这里接管 (pandoc -s 不再参与); body 已由调用方转义,
/// 以 PreEscaped 原样嵌入, 并经 `doc_fragment` 包装 (恰好一次).
/// 样式仍由 `inject_doc_style` 在 </head> 前注入.
pub fn doc_page(title: &str, body: &str) -> String {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) }
                // KaTeX 数学排版 (服务端渲染输出的 .katex 结构依赖此样式表与字体)
                link rel="stylesheet" href="/__/static/katex/katex.min.css";
            }
            body class="cb-doc-page" {
                (maud::PreEscaped(doc_fragment(body)))
            }
        }
    }
    .into_string()
}

/// pandoc 行为对齐: 文档标题块 `<header id="title-block-header"><h1 class="title">…</h1></header>`.
/// org 由渲染 handler 内联输出 (含 author/date), markdown 走此 helper.
pub fn title_header(title: &str) -> String {
    html! {
        header id="title-block-header" {
            h1 class="title" { (title) }
        }
    }
    .into_string()
}

/// TOC 条目: 标题级别 + 纯文本 + 锚点 slug.
pub struct TocEntry {
    pub level: usize,
    pub text: String,
    pub slug: String,
}

/// slug 规则: 小写; 非字母数字 -> '-'; 重复 slug 追加 -1/-2 后缀保证唯一.
/// 字母数字包含 CJK, 中文标题锚点原样保留.
/// org 与 markdown 共用此函数 — TOC 链接与正文锚点必须一一对应 (测试锁定).
pub fn slugify(raw: &str, used: &mut HashSet<String>) -> String {
    let base: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if used.insert(base.clone()) {
        return base;
    }
    let mut n = 1;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// TOC 树的节点 (arena 式索引), 避免手工嵌套栈的边界错误.
struct TocNode<'a> {
    entry: &'a TocEntry,
    children: Vec<usize>,
}

/// 由标题列表生成嵌套 `<nav id="TOC"><ul>…</ul></nav>`, 按标题级别嵌套.
/// level > 6 按 h6 处理 (与正文标题渲染一致); max_depth 限制 TOC 深度.
/// 无条目时返回空串 (页面无 TOC, 宽屏侧栏守卫 `:has(#TOC)` 不生效).
pub fn toc_html(entries: &[TocEntry], max_depth: Option<usize>) -> String {
    // 先建树 (arena 式), 再递归渲染, 避免手工嵌套栈的边界错误
    let mut nodes: Vec<TocNode> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for entry in entries {
        if let Some(depth) = max_depth
            && entry.level > depth
        {
            continue;
        }
        let level = entry.level.min(6);
        while let Some(&top) = stack.last() {
            if nodes[top].entry.level.min(6) < level {
                break;
            }
            stack.pop();
        }
        let idx = nodes.len();
        nodes.push(TocNode {
            entry,
            children: Vec::new(),
        });
        match stack.last() {
            Some(&parent) => nodes[parent].children.push(idx),
            None => roots.push(idx),
        }
        stack.push(idx);
    }

    if roots.is_empty() {
        return String::new();
    }
    html! {
        nav id="TOC" {
            ul {
                @for &root in &roots {
                    (toc_node(&nodes, root))
                }
            }
        }
    }
    .into_string()
}

fn toc_node(nodes: &[TocNode], idx: usize) -> Markup {
    let node = &nodes[idx];
    html! {
        li {
            a href=(format!("#{}", node.entry.slug)) { (node.entry.text) }
            @if !node.children.is_empty() {
                ul {
                    @for &child in &node.children {
                        (toc_node(nodes, child))
                    }
                }
            }
        }
    }
}

/// 将 DOC_STYLE 注入页面 </head> 之前, 覆盖各渲染器的默认样式.
pub fn inject_doc_style(html: &str) -> String {
    match html.find("</head>") {
        Some(head_end) => {
            format!(
                "{}<style>\n{DOC_STYLE}\n</style>\n{}",
                &html[..head_end],
                &html[head_end..]
            )
        }
        None => html.to_string(),
    }
}

/// 美化 pandoc/Emacs 导出的文档 HTML 的自定义样式. 亮色/暗色自适应.
/// 独立存放于 assets/chapbook-doc.css (编辑器高亮/lint 友好), 编译期内嵌进二进制.
pub const DOC_STYLE: &str = include_str!("../assets/chapbook-doc.css");

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use super::*;

    fn meta(rel: PathBuf) -> FileMeta {
        FileMeta {
            relative_to_root: rel,
            display_name: String::new(),
            is_directory: false,
            size: 0,
            last_modified_time: SystemTime::UNIX_EPOCH,
            last_access_time: SystemTime::UNIX_EPOCH,
            creation_time: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn dir_page_renders_bdi_label_inside_anchor_for_utf8() {
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[meta(PathBuf::from("My File.txt"))],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(
            page.contains(r#"<a href="/My%20File.txt"><bdi dir="auto">My File.txt</bdi></a>"#),
            "page: {page}"
        );
    }

    /// 非 UTF-8 basename 的 display-only 行: 名称必须出现 (bdi 隔离),
    /// 但不得有 `<a href>` 锚点, 更不得有空 href (空 href 点击会导航回当前目录).
    #[cfg(unix)]
    #[test]
    fn dir_page_non_utf8_row_has_no_anchor() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let bad = PathBuf::from(OsString::from_vec(b"\xFFbad.txt".to_vec()));
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[meta(bad)],
            SortBy::default(),
            None,
        )
        .into_string();
        // 名称经显示 codec 转义后出现在 bdi 中, 带 display-only 提示, 且没有被 <a> 包裹
        assert!(
            page.contains(r#"<td><bdi dir="auto">\xFFbad.txt</bdi><span class="cb-non-utf8">非 UTF-8 名称（仅显示）</span></td>"#),
            "page: {page}"
        );
        assert!(!page.contains(r#"href="""#), "page: {page}");
        // 无 action identity 与行尾控制
        assert!(!page.contains("data-native-path-encoded"), "page: {page}");
        assert!(
            !page.contains(r#"data-cb-action="preview""#),
            "page: {page}"
        );
        assert!(!page.contains("rel=\"noopener\""), "page: {page}");
        assert!(!page.contains("?download=1"), "page: {page}");
    }

    /// 片段 helper: 恰好一个外层 `.cb-doc` 包装, 无 doctype/head/style/body 骨架.
    #[test]
    fn doc_fragment_is_single_cb_doc_div_without_page_shell() {
        let frag = doc_fragment("<p>hi</p>");
        assert!(frag.starts_with("<div class=\"cb-doc\">"), "frag: {frag}");
        assert!(frag.ends_with("</div>"), "frag: {frag}");
        assert_eq!(
            frag.matches("<div class=\"cb-doc\">").count(),
            1,
            "frag: {frag}"
        );
        assert!(frag.contains("<p>hi</p>"), "frag: {frag}");
        assert!(!frag.contains("<!DOCTYPE"), "frag: {frag}");
        assert!(!frag.contains("<head"), "frag: {frag}");
        assert!(!frag.contains("<style"), "frag: {frag}");
        assert!(!frag.contains("<body"), "frag: {frag}");
    }

    /// 完整页: body 带 cb-doc-page 类, 正文经同一 doc_fragment 恰好包装一次,
    /// KaTeX 链接保留, inject_doc_style 样式标记保留.
    #[test]
    fn doc_page_wraps_body_once_with_page_class() {
        let page = doc_page("T", "<p>hi</p>");
        assert!(page.starts_with("<!DOCTYPE html>"), "page: {page}");
        assert!(page.contains("<title>T</title>"), "page: {page}");
        assert!(
            page.contains(r#"<body class="cb-doc-page">"#),
            "page: {page}"
        );
        assert_eq!(
            page.matches("<div class=\"cb-doc\">").count(),
            1,
            "page: {page}"
        );
        assert!(
            page.contains(r#"href="/__/static/katex/katex.min.css""#),
            "page: {page}"
        );
        let styled = inject_doc_style(&page);
        assert!(
            styled.contains("/* chapbook-doc-style */"),
            "styled: {styled}"
        );
    }

    /// CSS 重构静态守卫: 页面布局挂在 .cb-doc-page, 内容排版选择器全部作用域化到
    /// .cb-doc, 盒模型作用域化, :root 变量保持全局, .cb-doc 不设置 transform/filter/perspective.
    #[test]
    fn doc_css_scopes_layout_and_content() {
        let css = DOC_STYLE;
        assert!(css.contains(":root"), "css: {css}");
        assert!(
            css.contains(
                ".cb-doc *, .cb-doc *::before, .cb-doc *::after { box-sizing: border-box; }"
            ),
            "css: {css}"
        );
        assert!(css.contains(".cb-doc-page"), "css: {css}");
        assert!(css.contains(".cb-doc-page:has(#TOC)"), "css: {css}");
        assert!(css.contains(".cb-doc-page #TOC"), "css: {css}");
        assert!(!css.contains("body:has(#TOC)"), "css: {css}");
        assert!(!css.contains("\nhtml {"), "css: {css}");
        assert!(!css.contains("\nbody {"), "css: {css}");
        // 视口滚动声明在根元素上且限定完整文档页 (body 上的 scroll-* 不会传播到视口),
        // 且是移动而非复制 (全表仅各出现一次)
        assert!(css.contains("html:has(> body.cb-doc-page)"), "css: {css}");
        assert!(
            css.contains(
                "html:has(> body.cb-doc-page) {\n  scroll-behavior: smooth;\n  scroll-padding-top: 1.5rem;\n}"
            ),
            "css: {css}"
        );
        assert_eq!(
            css.matches("scroll-behavior: smooth").count(),
            1,
            "css: {css}"
        );
        assert_eq!(
            css.matches("scroll-padding-top: 1.5rem").count(),
            1,
            "css: {css}"
        );
        let page_rule = css
            .split(".cb-doc-page {")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .expect("css has a `.cb-doc-page {` rule");
        let doc_rule = css
            .split("\n.cb-doc {")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .expect("css has a `.cb-doc {` rule");
        // 页面规则: border-box + 背景/栏宽/外边距/内边距, 不携带滚动或排版声明
        assert!(
            page_rule.contains("box-sizing: border-box"),
            "page rule: {page_rule}"
        );
        assert!(
            page_rule.contains("background-color: var(--cb-bg)"),
            "page rule: {page_rule}"
        );
        assert!(
            page_rule.contains("max-width: 100rem"),
            "page rule: {page_rule}"
        );
        assert!(
            page_rule.contains("margin: 0 auto"),
            "page rule: {page_rule}"
        );
        assert!(
            page_rule.contains("padding: 1.2rem 1.5rem 5rem"),
            "page rule: {page_rule}"
        );
        assert!(!page_rule.contains("scroll"), "page rule: {page_rule}");
        assert!(!page_rule.contains("\n  color:"), "page rule: {page_rule}");
        assert!(!page_rule.contains("font-"), "page rule: {page_rule}");
        assert!(!page_rule.contains("line-height"), "page rule: {page_rule}");
        // .cb-doc 规则: 基础排版 (颜色/字体/字号/行高) 整页与片段共用, 不携带页面布局
        assert!(
            doc_rule.contains("color: var(--cb-text)"),
            "doc rule: {doc_rule}"
        );
        assert!(
            doc_rule.contains(
                "font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Helvetica Neue\", Arial, \"PingFang SC\", \"Hiragino Sans GB\", \"Microsoft YaHei\", \"Noto Sans CJK SC\", sans-serif"
            ),
            "doc rule: {doc_rule}"
        );
        assert!(
            doc_rule.contains("font-size: 12.8px"),
            "doc rule: {doc_rule}"
        );
        assert!(
            doc_rule.contains("line-height: 1.75"),
            "doc rule: {doc_rule}"
        );
        assert!(!doc_rule.contains("background-"), "doc rule: {doc_rule}");
        assert!(!doc_rule.contains("max-width"), "doc rule: {doc_rule}");
        assert!(!doc_rule.contains("\n  margin:"), "doc rule: {doc_rule}");
        assert!(!doc_rule.contains("\n  padding:"), "doc rule: {doc_rule}");
        assert!(
            css.contains(".cb-doc h1, .cb-doc h2, .cb-doc h3, .cb-doc h4, .cb-doc h5, .cb-doc h6"),
            "css: {css}"
        );
        assert!(css.contains(".cb-doc pre.src"), "css: {css}");
        assert!(css.contains(".cb-doc pre code span.keyword"), "css: {css}");
        assert!(css.contains(".cb-doc #TOC a:hover"), "css: {css}");
        assert!(css.contains(".cb-doc .katex-display"), "css: {css}");
        assert!(!css.contains("transform"), "css: {css}");
        assert!(!css.contains("filter"), "css: {css}");
        assert!(!css.contains("perspective"), "css: {css}");
    }

    use crate::listing::SearchResult;

    fn meta2(rel: PathBuf, is_directory: bool) -> FileMeta {
        FileMeta {
            relative_to_root: rel,
            display_name: String::new(),
            is_directory,
            size: 0,
            last_modified_time: SystemTime::UNIX_EPOCH,
            last_access_time: SystemTime::UNIX_EPOCH,
            creation_time: SystemTime::UNIX_EPOCH,
        }
    }

    /// 共享页面 head: Materialize CSS / theme / chapbook-doc.css / browser JS (defer) /
    /// title / viewport; token meta 只在 Some 时出现, 无 token 时不渲染 native 控制.
    #[test]
    fn dir_page_shared_head_and_native_token_meta() {
        let with = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[],
            SortBy::default(),
            Some("tok123"),
        )
        .into_string();
        assert!(
            with.contains(r#"<link rel="stylesheet" href="/__/static/css/materialize.min.css">"#),
            "with: {with}"
        );
        assert!(
            with.contains(r#"<link rel="stylesheet" href="/__/static/css/chapbook-theme.css">"#),
            "with: {with}"
        );
        assert!(
            with.contains(r#"<link rel="stylesheet" href="/__/static/css/chapbook-doc.css">"#),
            "with: {with}"
        );
        assert!(
            with.contains(r#"src="/__/static/js/chapbook-browser.js""#),
            "with: {with}"
        );
        assert!(with.contains("defer"), "with: {with}");
        assert!(
            with.contains("<meta name=\"viewport\" content=\"width=device-width"),
            "with: {with}"
        );
        assert!(
            with.contains(r#"<meta name="cb-native-open-token" content="tok123">"#),
            "with: {with}"
        );
        // 常驻搜索表单 / 视图切换 / 预览面板骨架
        assert!(with.contains(r#"action="/__/search""#), "with: {with}");
        assert!(with.contains("cb-search-q"), "with: {with}");
        assert!(with.contains("cb-preview"), "with: {with}");
        assert!(with.contains(r#"data-cb-action="native""#), "with: {with}");

        let without = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(
            !without.contains("cb-native-open-token"),
            "without: {without}"
        );
        assert!(
            !without.contains(r#"data-cb-action="native""#),
            "without: {without}"
        );
    }

    #[test]
    fn dir_page_hides_semantic_heading_instead_of_repeating_visible_path() {
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv/nested"),
            &[],
            SortBy::default(),
            None,
        )
        .into_string();

        assert!(
            page.contains(r#"<h1 class="cb-visually-hidden">目录 /nested 中的文件</h1>"#),
            "page: {page}"
        );
        assert!(!page.contains("Index of"), "page: {page}");
    }

    /// 面包屑: 根节点 href "/" + root basename label; 中间节点逐级累计编码
    /// (空格 %20, + %2B, = %3D, # %23, 中文 percent-encoded), 与 FileMeta::href 同一编码器;
    /// 当前节点纯文本不生成链接.
    #[test]
    fn dir_page_breadcrumb_special_segments_and_current_plain() {
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv/a b/c+d/e=f/g#h/中文"),
            &[],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(page.contains(r#"<a href="/">"#), "page: {page}");
        assert!(page.contains("<bdi dir=\"auto\">srv</bdi>"), "page: {page}");
        assert!(page.contains(r#"<a href="/a%20b">"#), "page: {page}");
        assert!(page.contains(r#"<a href="/a%20b/c%2Bd">"#), "page: {page}");
        assert!(
            page.contains(r#"<a href="/a%20b/c%2Bd/e%3Df">"#),
            "page: {page}"
        );
        assert!(
            page.contains(r#"<a href="/a%20b/c%2Bd/e%3Df/g%23h">"#),
            "page: {page}"
        );
        // 当前节点 (中文): 纯文本, 不生成链接
        assert!(
            page.contains("<bdi dir=\"auto\">中文</bdi>"),
            "page: {page}"
        );
        assert!(
            !page.contains(r#"href="/a%20b/c%2Bd/e%3Df/g%23h/%E4%B8%AD%E6%96%87""#),
            "current segment must not link: {page}"
        );
    }

    /// root 目录的面包屑: 只有根链接, 无中间/当前节点.
    #[test]
    fn dir_page_breadcrumb_at_root_is_root_link_only() {
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(page.contains(r#"<a href="/">"#), "page: {page}");
        assert!(!page.contains("cb-crumb-sep"), "page: {page}");
    }

    #[cfg(unix)]
    #[test]
    fn dir_page_breadcrumb_non_utf8_ancestor_is_display_only() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let p = PathBuf::from(OsString::from_vec(b"/srv/bad\xFF/leaf".to_vec()));
        let page = dir_page(Path::new("/srv"), &p, &[], SortBy::default(), None).into_string();
        // 非 UTF-8 段及其后 remainder 均不产生链接, 只显示 (显示 codec 转义)
        assert!(
            page.contains("<bdi dir=\"auto\">bad\\xFF</bdi>"),
            "page: {page}"
        );
        assert!(
            page.contains("<bdi dir=\"auto\">leaf</bdi>"),
            "page: {page}"
        );
        assert!(!page.contains("href=\"/bad"), "page: {page}");
    }

    /// 行 identity: data-browser-entry + ASCII-only encoded path + 安全 label.
    /// href 与 dataset 是同一 RFC 3986 编码值 (仅前导斜杠不同).
    #[test]
    fn dir_page_row_identity_and_label_attributes() {
        let img = meta2(PathBuf::from("a b/photo.png"), false);
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[img],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(page.contains(r#"data-browser-entry"#), "page: {page}");
        assert!(
            page.contains(r#"data-native-path-encoded="a%20b/photo.png""#),
            "page: {page}"
        );
        assert!(
            page.contains(r#"<a href="/a%20b/photo.png">"#),
            "page: {page}"
        );
        // 安全 label attributes: basename 经显示 codec
        assert!(page.contains(r#"title="photo.png""#), "page: {page}");
        assert!(page.contains(r#"aria-label="photo.png""#), "page: {page}");
    }

    /// 网格缩略图判定: 仅图片扩展名, 大小写不敏感; 目录/音视频/无扩展名一律 false.
    #[test]
    fn image_entry_detection_is_exact() {
        let cases: &[(&str, bool, bool)] = &[
            ("folder", true, false),
            ("a.png", false, true),
            ("b.JPG", false, true),
            ("c.webp", false, true),
            ("d.svg", false, true),
            ("v.mp4", false, false),
            ("w.MOV", false, false),
            ("s.mp3", false, false),
            ("t.flac", false, false),
            ("u.m4a", false, false),
            ("f.txt", false, false),
            ("noext", false, false),
        ];
        for (name, is_dir, expect_image) in cases {
            assert_eq!(
                is_image_entry(&meta2(PathBuf::from(name), *is_dir)),
                *expect_image,
                "{name}"
            );
        }
    }

    /// 保持 6 列 striped 表 + 父行 colspan=6；父链接为精确绝对 root-relative URL，
    /// 行尾 action 放进最后单元格，不新增第 7 列。
    #[test]
    fn dir_page_keeps_six_columns_and_actions_in_last_cell() {
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv/a/b"),
            &[meta2(PathBuf::from("f.txt"), false)],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(page.contains(r#"<td colspan="6">"#), "page: {page}");
        assert!(
            page.contains(r#"<a href="/a" data-cb-parent"#),
            "page: {page}"
        );
        // ../ 行 (colspan) + 1 数据行 * 6 单元格
        assert_eq!(page.matches("<td").count(), 7, "page: {page}");
        // 行尾 action: preview / new tab (target=_blank rel=noopener) / download
        assert!(page.contains(r#"data-cb-action="preview""#), "page: {page}");
        assert!(page.contains(r#"rel="noopener""#), "page: {page}");
        assert!(page.contains("?download=1"), "page: {page}");
        // 无 token -> 无 native 控制
        assert!(!page.contains(r#"data-cb-action="native""#), "page: {page}");
    }

    /// 网格: 图片项用 lazy img + 精确 raw URL (无模式 query); 非图片/目录用 badge;
    /// 列表与网格容器都渲染, 网格默认 hidden.
    #[test]
    fn dir_page_grid_image_attrs_and_badges() {
        let files = [
            meta2(PathBuf::from("photo.png"), false),
            meta2(PathBuf::from("notes.txt"), false),
            meta2(PathBuf::from("folder"), true),
            meta2(PathBuf::from("README"), false),
        ];
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &files,
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(page.contains(r#"id="cb-view-list""#), "page: {page}");
        assert!(page.contains(r#"id="cb-view-grid" hidden"#), "page: {page}");
        assert!(
            page.contains(r#"<img loading="lazy" decoding="async" src="/photo.png" alt="">"#),
            "page: {page}"
        );
        assert!(page.contains(">DIR<"), "page: {page}");
        assert!(page.contains(">txt<"), "page: {page}");
        assert!(page.contains(">FILE<"), "page: {page}");
        assert!(!page.contains("src=\"/notes.txt\""), "page: {page}");
    }

    /// 可操作网格 label 必须是原生 anchor, href 为 exact Full href (无 query):
    /// Ctrl/Meta/中键/新 tab 行为由浏览器原生提供。同时锁定服务端编码器原样
    /// 保留 RFC 3986 sub-delims !$()* (与 JS 校验器接受表一致)。
    #[test]
    fn grid_actionable_label_is_native_anchor_with_exact_href() {
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[meta(PathBuf::from("a!b$c(d)e*f.txt"))],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(
            page.contains(r#"data-native-path-encoded="a!b$c(d)e*f.txt""#),
            "page: {page}"
        );
        assert!(
            page.contains(
                r#"<div class="cb-grid-label"><a href="/a!b$c(d)e*f.txt"><bdi dir="auto">a!b$c(d)e*f.txt</bdi></a>"#
            ),
            "page: {page}"
        );
        assert!(
            !page.contains(r#"<div class="cb-grid-label"><a href="/a!b$c(d)e*f.txt?"#),
            "网格 label 不得携带 fragment/download query: {page}"
        );
    }

    /// 非 actionable 网格 label 保持 span 形态: 无 anchor、无 action identity。
    #[cfg(unix)]
    #[test]
    fn grid_non_utf8_label_is_span_without_anchor() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let bad = PathBuf::from(OsString::from_vec(b"\xFFbad.txt".to_vec()));
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[meta(bad)],
            SortBy::default(),
            None,
        )
        .into_string();
        assert!(
            page.contains(
                r#"<div class="cb-grid-label"><bdi dir="auto">\xFFbad.txt</bdi><span class="cb-non-utf8">非 UTF-8 名称（仅显示）</span></div>"#
            ),
            "page: {page}"
        );
        assert!(
            !page.contains(r#"<div class="cb-grid-label"><a"#),
            "page: {page}"
        );
        assert!(
            !page.contains(r#"data-native-path-encoded"#),
            "page: {page}"
        );
    }

    fn search_result(entries: Vec<FileMeta>, truncated: bool) -> SearchResult {
        SearchResult { entries, truncated }
    }

    /// 搜索页: 面包屑 (root 链接 + 纯文本 搜索), 4 列 (名称/位置/大小/修改时间),
    /// 行序保持结果序, 位置列: 顶层条目链接到 "/", 子目录条目链接到父目录.
    #[test]
    fn search_page_columns_order_and_location_links() {
        let entries = vec![
            meta2(PathBuf::from("sub/a.txt"), false),
            meta2(PathBuf::from("b c.png"), false),
        ];
        let page =
            search_page(Path::new("/srv"), "a", &search_result(entries, false), None).into_string();
        assert!(page.contains(r#"<a href="/">"#), "page: {page}");
        assert!(page.contains(">搜索<"), "page: {page}");
        assert!(page.contains("<th>名称</th>"), "page: {page}");
        assert!(page.contains("<th>位置</th>"), "page: {page}");
        assert!(page.contains("<th>大小</th>"), "page: {page}");
        assert!(page.contains("<th>修改时间</th>"), "page: {page}");
        assert!(page.contains(r#"<a href="/sub">"#), "page: {page}");
        assert!(page.contains(r#"<a href="/">"#), "page: {page}");
        // 结果顺序保持 (a.txt 行先于 b c.png 行)
        let first = page.find("a.txt").expect("first entry");
        let second = page.find("b%20c.png").expect("second entry");
        assert!(first < second, "page: {page}");
    }

    #[test]
    fn search_page_truncated_and_empty_notices() {
        let trunc =
            search_page(Path::new("/srv"), "q", &search_result(vec![], true), None).into_string();
        assert!(trunc.contains("已截断"), "trunc: {trunc}");
        let empty =
            search_page(Path::new("/srv"), "q", &search_result(vec![], false), None).into_string();
        assert!(empty.contains("没有找到匹配的文件"), "empty: {empty}");
        assert!(!empty.contains("<table"), "empty: {empty}");
    }

    #[cfg(unix)]
    #[test]
    fn search_page_non_utf8_entry_display_only_but_location_linked() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let bad = FileMeta {
            relative_to_root: PathBuf::from(OsString::from_vec(b"dir/\xFFleaf.txt".to_vec())),
            display_name: String::new(),
            is_directory: false,
            size: 0,
            last_modified_time: SystemTime::UNIX_EPOCH,
            last_access_time: SystemTime::UNIX_EPOCH,
            creation_time: SystemTime::UNIX_EPOCH,
        };
        let page = search_page(
            Path::new("/srv"),
            "leaf",
            &search_result(vec![bad], false),
            None,
        )
        .into_string();
        // 名称 display-only + 提示; 无 action identity/控制
        assert!(
            page.contains("<bdi dir=\"auto\">\\xFFleaf.txt</bdi>"),
            "page: {page}"
        );
        assert!(page.contains("非 UTF-8 名称（仅显示）"), "page: {page}");
        assert!(!page.contains("data-native-path-encoded"), "page: {page}");
        assert!(
            !page.contains(r#"data-cb-action="preview""#),
            "page: {page}"
        );
        // 父目录是合法 UTF-8: 位置仍链接到 /dir
        assert!(page.contains(r#"<a href="/dir">"#), "page: {page}");
    }

    /// 目录 fragment: 恰好一个外层 wrapper 携带当前目录的 ASCII encoded path,
    /// 无 doctype/head/style/body; 迷你列表含名称/大小与可操作 anchor (带自身 identity).
    #[test]
    fn dir_fragment_wrapper_encoded_path_and_mini_list() {
        let files = vec![
            meta2(PathBuf::from("a.txt"), false),
            meta2(PathBuf::from("sub"), true),
        ];
        let frag = dir_fragment(Path::new("/srv"), Path::new("/srv/sub dir"), &files);
        assert!(
            frag.starts_with(
                r#"<div class="cb-doc cb-dir-fragment" data-native-path-encoded="sub%20dir""#
            ),
            "frag: {frag}"
        );
        assert_eq!(
            frag.matches(r#"<div class="cb-doc cb-dir-fragment""#)
                .count(),
            1,
            "frag: {frag}"
        );
        assert!(!frag.contains("<!DOCTYPE"), "frag: {frag}");
        assert!(!frag.contains("<head"), "frag: {frag}");
        assert!(!frag.contains("<style"), "frag: {frag}");
        assert!(!frag.contains("<body"), "frag: {frag}");
        assert!(
            frag.contains(r#"<a href="/a.txt" data-native-path-encoded="a.txt">"#),
            "frag: {frag}"
        );
        assert!(frag.contains("0 B"), "frag: {frag}");
    }

    /// root 的 fragment wrapper: encodedPath 是合法空字符串而不是属性缺失.
    #[test]
    fn dir_fragment_root_wrapper_empty_encoded_path() {
        let frag = dir_fragment(Path::new("/srv"), Path::new("/srv"), &[]);
        assert!(
            frag.contains(r#"data-native-path-encoded="""#),
            "frag: {frag}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dir_fragment_non_utf8_entry_display_only() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let bad = FileMeta {
            relative_to_root: PathBuf::from(OsString::from_vec(b"\xFFbad.txt".to_vec())),
            display_name: String::new(),
            is_directory: false,
            size: 0,
            last_modified_time: SystemTime::UNIX_EPOCH,
            last_access_time: SystemTime::UNIX_EPOCH,
            creation_time: SystemTime::UNIX_EPOCH,
        };
        let frag = dir_fragment(Path::new("/srv"), Path::new("/srv"), &[bad]);
        assert!(
            frag.contains("<bdi dir=\"auto\">\\xFFbad.txt</bdi>"),
            "frag: {frag}"
        );
        assert!(frag.contains("非 UTF-8 名称（仅显示）"), "frag: {frag}");
        // 只有 wrapper 携带 identity; 迷你条目无 anchor / 无 action identity
        assert_eq!(
            frag.matches("data-native-path-encoded").count(),
            1,
            "frag: {frag}"
        );
        assert!(!frag.contains(r#"<a href="/"#), "frag: {frag}");
    }
}
