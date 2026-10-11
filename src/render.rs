//! HTML 生成 (maud).
//!
//! - 目录/搜索完整页共用 [`page_shell`]: 同一套顶部工具区 ([`topbar`])、面包屑、
//!   预览面板与 native token meta; 独立阅读页由 [`doc_page`] 输出同一工具区.
//! - 列表/网格/fragment 的全部导航链接与搜索表单统一保留有效显示规则
//!   (`sort=Column:Order&hidden=0|1`; 搜索页带 q), 实际离开搜索才移除 q.
//! - 非 UTF-8 名称一律 display-only (无 anchor / 无 action identity), 仅显示
//!   codec 转义文本; 规范 filesystem identity 只用 `data-native-path-encoded`.
//! - 工具区不含 inline handler: 视图图标组/隐藏项开关/排序入口在无 JS 时以
//!   真实链接或原生 details/summary 工作 (树/网格按真实能力渐进增强).

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use maud::{DOCTYPE, Markup, html};

use crate::listing::{ListingOptions, SearchResult};
use crate::meta::{
    FileMeta, display_path_markup, display_path_text, display_segment, encode_relative_path,
    escape_os_name, format_time, percent_encode_segment,
};
use crate::sort::{SortBy, SortColumn, SortOrder};

/// 有效显示规则的 query 片段: `sort=Column:Order&hidden=0|1` (sort 在前, 精确大小写).
fn listing_query(options: ListingOptions) -> String {
    format!(
        "sort={}:{}&hidden={}",
        options.sort_by.column,
        options.sort_by.order,
        hidden_value(options)
    )
}

/// 排序 query 值 (`Column:Order`): 隐藏字段与 `data-cb-sort` 同一格式.
fn sort_value(options: ListingOptions) -> String {
    format!("{}:{}", options.sort_by.column, options.sort_by.order)
}

/// 布尔 → `1`/`0` (hidden query 与 `data-cb-hidden` 同一编码).
fn hidden_value(options: ListingOptions) -> &'static str {
    if options.show_hidden { "1" } else { "0" }
}

/// canonical 绝对路径链接 + 有效显示规则: root 为 `/`, 其余 `/{encoded}`.
fn path_href(encoded: &str, options: ListingOptions) -> String {
    if encoded.is_empty() {
        format!("/?{}", listing_query(options))
    } else {
        format!("/{encoded}?{}", listing_query(options))
    }
}

/// 当前路径不变的相对 query 链接: `?sort=…&hidden=…`, 搜索页带 q (`?q=…&…`).
/// 覆盖视图/隐藏项开关与完整页的列表入口 (离开搜索的条目链接不调用本函数).
fn browse_href(q: Option<&str>, options: ListingOptions) -> String {
    match q {
        Some(q) => format!(
            "?q={}&{}",
            percent_encode_segment(q),
            listing_query(options)
        ),
        None => format!("?{}", listing_query(options)),
    }
}

/// 排序入口/表头快捷入口的目标链接: 目标排序 + 保留有效 hidden (与 q).
fn sort_href(q: Option<&str>, options: ListingOptions, target: SortBy) -> String {
    let next = ListingOptions {
        sort_by: target,
        show_hidden: options.show_hidden,
    };
    match q {
        Some(q) => format!("?q={}&{}", percent_encode_segment(q), listing_query(next)),
        None => format!("?{}", listing_query(next)),
    }
}

/// 排序菜单的十二个 (列, 方向) 选项.
/// `enabled` 为 false (独立阅读表面) 时省略 href: 无 JS 不可导航 (禁用),
/// 客户端启用时再补 href.
struct SortChoice {
    /// `Column:Order` (data-cb-sort 与 query 值).
    value: String,
    href: Option<String>,
    label: String,
    active: bool,
}

fn sort_choices(q: Option<&str>, options: ListingOptions, enabled: bool) -> Vec<SortChoice> {
    let mut choices = Vec::with_capacity(12);
    for column in SortColumn::ALL {
        for order in [SortOrder::Asc, SortOrder::Desc] {
            let target = SortBy { column, order };
            choices.push(SortChoice {
                value: format!("{column}:{order}"),
                href: enabled.then(|| sort_href(q, options, target)),
                label: sort_option_label(column, order),
                active: target == options.sort_by,
            });
        }
    }
    choices
}

/// 排序菜单/摘要的列名 (query key 之外的显示文本).
fn sort_column_label(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Name => "名称",
        SortColumn::Type => "类型",
        SortColumn::Size => "大小",
        SortColumn::LastModified => "修改时间",
        SortColumn::LastAccess => "访问时间",
        SortColumn::Creation => "创建时间",
    }
}

fn sort_order_arrow(order: SortOrder) -> &'static str {
    match order {
        SortOrder::Asc => "▲",
        SortOrder::Desc => "▼",
    }
}

fn sort_option_label(column: SortColumn, order: SortOrder) -> String {
    format!("{} {}", sort_column_label(column), sort_order_arrow(order))
}

fn sort_state_label(sort_by: SortBy) -> String {
    sort_option_label(sort_by.column, sort_by.order)
}

/// 自包含图标: 无外部字体/CDN, 颜色继承 currentColor, 由 aria-label 提供名称.
fn icon(glyph: Markup) -> Markup {
    html! {
        svg class="cb-icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"
            focusable="false" fill="none" stroke="currentColor" stroke-width="1.5"
            stroke-linecap="round" stroke-linejoin="round" {
            (glyph)
        }
    }
}

fn icon_tree() -> Markup {
    icon(html! {
        path d="M2.5 2.5v11" {}
        path d="M2.5 5.5h3.5" {}
        path d="M2.5 10.5h3.5" {}
        rect x="6" y="3.5" width="7.5" height="4" {}
        rect x="6" y="8.5" width="7.5" height="4" {}
    })
}

fn icon_list() -> Markup {
    icon(html! {
        path d="M5.5 4h8" {}
        path d="M5.5 8h8" {}
        path d="M5.5 12h8" {}
        circle cx="2.6" cy="4" r="1.1" fill="currentColor" stroke="none" {}
        circle cx="2.6" cy="8" r="1.1" fill="currentColor" stroke="none" {}
        circle cx="2.6" cy="12" r="1.1" fill="currentColor" stroke="none" {}
    })
}

fn icon_grid() -> Markup {
    icon(html! {
        rect x="2.5" y="2.5" width="4.5" height="4.5" {}
        rect x="9" y="2.5" width="4.5" height="4.5" {}
        rect x="2.5" y="9" width="4.5" height="4.5" {}
        rect x="9" y="9" width="4.5" height="4.5" {}
    })
}

fn icon_eye() -> Markup {
    icon(html! {
        path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8s-2.4 4.5-6.5 4.5S1.5 8 1.5 8z" {}
        circle cx="8" cy="8" r="2" {}
    })
}

/// 统一顶部工具区 (目录/搜索/独立阅读共用): 左搜索, 右树/列表/网格图标组 +
/// 隐藏项开关 + 排序入口.
///
/// - 树/网格图标服务端 `aria-disabled` (无 JS 不伪造树/网格), 由客户端启用;
///   列表入口是真实 anchor (阅读页指向父目录), 无 JS 可用.
/// - 隐藏项开关在浏览表面是 anchor (href = 切换后的 hidden, 保留 sort/q),
///   阅读表面是 `aria-disabled` button (href 缺失, 保留 hover/focus 提示).
/// - 排序入口是原生 details/summary (键盘可打开, 无 JS 可用), 十二个链接
///   保留 q/hidden; 阅读表面 `aria-disabled` 且链接不带 href (客户端启用时补).
/// - hover 与键盘焦点提示统一由 `data-cb-tip` + CSS ::after 呈现.
fn topbar(
    surface: &str,
    q: Option<&str>,
    options: ListingOptions,
    list_href: Option<&str>,
) -> Markup {
    let reading = surface == "reading";
    let hidden_pressed = if options.show_hidden { "true" } else { "false" };
    let list_pressed = if reading { "false" } else { "true" };
    let toggled = ListingOptions {
        sort_by: options.sort_by,
        show_hidden: !options.show_hidden,
    };
    let hidden_toggle_href = (!reading).then(|| browse_href(q, toggled));
    let choices = sort_choices(q, options, !reading);
    html! {
        header.cb-topbar {
            form.cb-search action="/__/search" method="get" role="search" {
                input type="search" name="q" id="cb-search-q" placeholder="搜索文件"
                    aria-label="搜索文件" autocomplete="off" value=(q.unwrap_or(""));
                input type="hidden" name="sort" value=(sort_value(options));
                input type="hidden" name="hidden" value=(hidden_value(options));
            }
            div.cb-view-group role="group" aria-label="浏览视图" {
                button type="button" class="cb-icon-btn cb-view-btn" data-cb-view="tree"
                    aria-pressed="false" aria-disabled="true" aria-label="树视图"
                    data-cb-tip="树视图（需启用 JavaScript）" {
                    (icon_tree())
                }
                @if let Some(href) = list_href {
                    a class="cb-icon-btn cb-view-btn" data-cb-view="list" href=(href)
                        aria-pressed=(list_pressed) aria-label="列表视图" data-cb-tip="列表视图" {
                        (icon_list())
                    }
                } @else {
                    button type="button" class="cb-icon-btn cb-view-btn" data-cb-view="list"
                        aria-pressed="false" aria-disabled="true" aria-label="列表视图"
                        data-cb-tip="列表视图（需启用 JavaScript）" {
                        (icon_list())
                    }
                }
                button type="button" class="cb-icon-btn cb-view-btn" data-cb-view="grid"
                    aria-pressed="false" aria-disabled="true" aria-label="网格视图"
                    data-cb-tip="网格视图（需启用 JavaScript）" {
                    (icon_grid())
                }
            }
            @if let Some(href) = hidden_toggle_href {
                a class="cb-icon-btn cb-hidden-toggle" data-cb-hidden-toggle href=(href)
                    aria-pressed=(hidden_pressed) aria-label="显示隐藏项"
                    data-cb-tip="显示隐藏项" {
                    (icon_eye())
                }
            } @else {
                button type="button" class="cb-icon-btn cb-hidden-toggle" data-cb-hidden-toggle
                    aria-pressed=(hidden_pressed) aria-disabled="true" aria-label="显示隐藏项"
                    data-cb-tip="阅读模式不适用：请先在目录或搜索中使用" {
                    (icon_eye())
                }
            }
            details class="cb-sort" aria-disabled=[reading.then_some("true")] {
                summary data-cb-sort-summary
                    data-cb-tip=(if reading { "阅读模式不适用：请先在目录或搜索中使用" } else { "排序" }) {
                    "排序"
                    span class="cb-sort-state" data-cb-sort-state { (sort_state_label(options.sort_by)) }
                }
                div class="cb-sort-menu" role="group" aria-label="排序方式" {
                    @for choice in choices {
                        a href=[choice.href.as_deref()] data-cb-sort=(&choice.value)
                            aria-current=[choice.active.then_some("true")] {
                            (&choice.label)
                        }
                    }
                }
            }
        }
    }
}

/// 目录完整页 (可信 UI): 共享页面骨架 + 面包屑 + 隐藏 h1 + 6 列 striped 表格 +
/// 网格 + 预览面板; native_token 为 Some 时 head 嵌入 token meta 并渲染
/// native-open 控制. 列表入口是当前目录 + 有效显示规则.
pub fn dir_page(
    root: &Path,
    path: &Path,
    files: &[FileMeta],
    options: ListingOptions,
    native_token: Option<&str>,
) -> Markup {
    let title = dir_title(root, path);
    let identity = encode_relative_path(path.strip_prefix(root).unwrap_or(Path::new("")));
    let content = html! {
        h1 class="cb-visually-hidden" { "目录 " (title) " 中的文件" }
        (dir_table(root, path, files, options, native_token))
        (entry_grid(files, options, true))
    };
    page_shell(PageShell {
        title,
        surface: "directory",
        q: None,
        options,
        identity,
        kind: Some("dir"),
        native_token: native_token.map(str::to_string),
        breadcrumb: breadcrumb_markup(root, path, options),
        content,
        list_href: Some(browse_href(None, options)),
        parent_href: None,
    })
}

/// 目录完整页 <title> / heading: root-relative 路径逐段经显示 codec 转义 join.
fn dir_title(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(Path::new(""));
    format!("/{}", display_path_text(rel))
}

/// 搜索完整页 (可信 UI): 同一共享骨架 + 面包屑 (root 链接 + 纯文本 搜索) +
/// 4 列结果表 + 网格 + 截断/空结果提示; 行序即 SearchResult 的确定性顺序.
pub fn search_page(
    root: &Path,
    query: &str,
    result: &SearchResult,
    options: ListingOptions,
    native_token: Option<&str>,
) -> Markup {
    let content = html! {
        h6.cb-heading { "搜索：" (query) }
        (search_results(result, options, true, native_token))
    };
    let breadcrumb = html! {
        nav.cb-breadcrumb aria-label="面包屑" {
            a href=(path_href("", options)) {
                (display_segment(root.file_name().unwrap_or_else(|| OsStr::new("/"))))
            }
            span.cb-crumb-sep { "/" }
            span.cb-current { "搜索" }
        }
    };
    page_shell(PageShell {
        title: format!("搜索：{query}"),
        surface: "search",
        q: Some(query.to_string()),
        options,
        identity: Some(String::new()),
        kind: None,
        native_token: native_token.map(str::to_string),
        breadcrumb,
        content,
        list_href: Some(browse_href(Some(query), options)),
        parent_href: Some(path_href("", options)),
    })
}

/// 目录 fragment: 恰好一个外层 wrapper (`<div class="cb-doc cb-dir-fragment">`)
/// 携带当前目录的 ASCII encoded path (root 为合法空串) 与 page_shell 相同的
/// surface/选项 metadata; 无 doctype/head/style/工具栏. 迷你列表含名称+大小;
/// 可操作条目渲染 anchor + 自身 encoded identity (供面板下钻) 与有效显示规则,
/// 非 UTF-8 条目 display-only.
pub fn dir_fragment(
    root: &Path,
    path: &Path,
    files: &[FileMeta],
    options: ListingOptions,
) -> String {
    let rel = path.strip_prefix(root).unwrap_or(Path::new(""));
    let encoded = encode_relative_path(rel);
    let title = display_path_text(rel);
    let title_markup = display_path_markup(rel);
    let body = html! {
        ul class="cb-dir-mini" {
            @for f in files {
                li data-kind=(if f.is_directory { "dir" } else { "file" }) {
                    // 可操作条目: anchor + 自身 encoded identity (供面板下钻);
                    // href 用共享编码器 + 有效显示规则, 非 UTF-8 条目 display-only
                    @if let Some(enc) = f.browser_path_encoded() {
                        a href=(format!("/{enc}?{}", listing_query(options)))
                            data-native-path-encoded=(enc) {
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
    html! {
        div class="cb-doc cb-dir-fragment"
            data-cb-surface="directory"
            data-cb-sort=(sort_value(options))
            data-cb-hidden=(hidden_value(options))
            data-native-path-encoded=[encoded.as_deref()]
            title=(title) {
            span.cb-frag-title hidden { (title_markup) }
            (body)
        }
    }
    .into_string()
}

/// 搜索 fragment: 与完整搜索页同一 `SearchResult`、同一过滤/排序规则与同一
/// 截断/空结果提示; wrapper 携带 search 语义 + q + root 空 identity.
/// 无 doctype/head/token/工具栏/重复 id (id 只属于完整页).
pub fn search_fragment(query: &str, result: &SearchResult, options: ListingOptions) -> String {
    html! {
        div class="cb-doc cb-search-fragment"
            data-cb-surface="search"
            data-cb-sort=(sort_value(options))
            data-cb-hidden=(hidden_value(options))
            data-cb-query=(query)
            data-native-path-encoded="" {
            (search_results(result, options, false, None))
        }
    }
    .into_string()
}

/// 搜索/目录完整页共用的结果区: 截断与空结果提示 + 列表/网格 sections.
/// 完整页带稳定 id (客户端切换视图), fragment 不带 id (可能嵌入已有页面);
/// native_token 只在完整搜索页 (受门禁) 传入, fragment 恒为 None.
fn search_results(
    result: &SearchResult,
    options: ListingOptions,
    with_ids: bool,
    native_token: Option<&str>,
) -> Markup {
    html! {
        @if result.truncated {
            p.cb-notice { "结果超过 500 条，已截断显示前 500 条。" }
        }
        @if result.entries.is_empty() {
            p.cb-empty { "没有找到匹配的文件。" }
        } @else {
            (search_table(result, options, with_ids, native_token))
            (entry_grid(&result.entries, options, with_ids))
        }
    }
}

/// 共享页面骨架 (目录页/搜索页共用): head (Materialize CSS / theme /
/// chapbook-doc.css / browser JS defer / title / viewport / 可选 token meta),
/// 顶部工具区, 面包屑, main (搜索页带 `data-cb-parent`) 与预览面板.
struct PageShell {
    title: String,
    surface: &'static str,
    q: Option<String>,
    options: ListingOptions,
    identity: Option<String>,
    kind: Option<&'static str>,
    native_token: Option<String>,
    breadcrumb: Markup,
    content: Markup,
    list_href: Option<String>,
    parent_href: Option<String>,
}

fn page_shell(shell: PageShell) -> Markup {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no, maximum-scale=1, minimum-scale=1";
                title { (shell.title) }
                link rel="stylesheet" href="/__/static/css/materialize.min.css";
                link rel="stylesheet" href="/__/static/css/chapbook-theme.css";
                link rel="stylesheet" href="/__/static/css/chapbook-doc.css";
                // KaTeX: 目录页预览面板与树模式内容区是文档片段的宿主,
                // 预加载样式表 (字体按需加载); 与 doc_page 同一资产.
                link rel="stylesheet" href="/__/static/katex/katex.min.css";
                script src="/__/static/js/chapbook-browser.js" defer {}
                @if let Some(token) = &shell.native_token {
                    meta name="cb-native-open-token" content=(token);
                }
            }
            body class="cb-browser-page"
                data-cb-surface=(shell.surface)
                data-cb-sort=(sort_value(shell.options))
                data-cb-hidden=(hidden_value(shell.options))
                data-native-path-encoded=[shell.identity.as_deref()]
                data-cb-query=[shell.q.as_deref()]
                data-kind=[shell.kind] {
                (topbar(shell.surface, shell.q.as_deref(), shell.options, shell.list_href.as_deref()))
                (shell.breadcrumb)
                // 搜索页 Backspace 目标: root (保留有效显示规则); 目录页由 ../ 行
                // 链接承担同一职责.
                @if let Some(parent) = &shell.parent_href {
                    main.cb-main data-cb-parent=(parent) {
                        (shell.content)
                    }
                } @else {
                    main.cb-main {
                        (shell.content)
                    }
                }
                aside.cb-preview id="cb-preview" hidden {
                    div.cb-preview-toolbar {
                        span.cb-preview-title id="cb-preview-title" { bdi dir="auto" {} }
                        span.cb-preview-actions {
                            button type="button" data-cb-action="full" title="当前 tab 打开" { "打开" }
                            button type="button" data-cb-action="new" title="新 tab 打开" { "新标签" }
                            button type="button" data-cb-action="download" title="下载" { "下载" }
                            @if shell.native_token.is_some() {
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

/// 面包屑: 根节点 (root basename label, href "/" + 有效显示规则), 中间节点为
/// 逐级累计的 exact UTF-8 路径 (共享编码器 + 有效显示规则); 当前节点纯文本.
/// 祖先路径非 UTF-8 时该段及其后 remainder display-only (不生成链接).
/// 每 segment 经显示 codec + bdi 隔离.
fn breadcrumb_markup(root: &Path, path: &Path, options: ListingOptions) -> Markup {
    let crumbs = breadcrumb_crumbs(root, path);
    let last = crumbs.len().saturating_sub(1);
    html! {
        nav.cb-breadcrumb aria-label="面包屑" {
            a href=(path_href("", options)) {
                (display_segment(root.file_name().unwrap_or_else(|| OsStr::new("/"))))
            }
            @for (i, (seg, href)) in crumbs.iter().enumerate() {
                span.cb-crumb-sep { "/" }
                @if i == last {
                    span.cb-current { (display_segment(seg)) }
                } @else if let Some(h) = href {
                    a href=(path_href(h, options)) { (display_segment(seg)) }
                } @else {
                    span.cb-current { (display_segment(seg)) }
                }
            }
        }
    }
}

/// 面包屑数据: (segment, 累计路径的 encoded href 片段); 累计路径非 UTF-8 时
/// href 为 None.
fn breadcrumb_crumbs(root: &Path, path: &Path) -> Vec<(OsString, Option<String>)> {
    let mut crumbs = Vec::new();
    if let Ok(rel) = path.strip_prefix(root) {
        let mut acc = PathBuf::new();
        for seg in rel.iter() {
            acc.push(seg);
            let href = encode_relative_path(&acc);
            crumbs.push((seg.to_os_string(), href));
        }
    }
    crumbs
}

/// 目录表格: 6 列保持既有结构 (thead 排序链接 + tbody 条纹行 + 父行 colspan=6);
/// 行尾 action 放在最后单元格内, 不新增第 7 列. 所有链接保留有效显示规则.
fn dir_table(
    root: &Path,
    path: &Path,
    files: &[FileMeta],
    options: ListingOptions,
    native_token: Option<&str>,
) -> Markup {
    let parent_href = path
        .parent()
        .and_then(|parent| parent.strip_prefix(root).ok())
        .and_then(encode_relative_path)
        .map(|encoded| path_href(&encoded, options));
    html! {
        section class="cb-view cb-view-list" id="cb-view-list" {
            table class="striped" {
                // 表头放进 thead: 浏览器会把 table 下的裸 tr 包进隐式 tbody,
                // 导致表头被条纹染色 (v2 的灰色带就是这么来的)
                thead { (table_header(options.sort_by, options, None)) }
                tbody {
                    @if path != root {
                        // colspan 覆盖全行: 单 td 行的条纹/背景只会染到单元格区域;
                        // 绝对 root-relative URL 同供原生 ../ 链接与 JS Backspace,
                        // 避免 slashless 当前 URL 把相对 "../" 多解析一级.
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
                                html! { (f.type_label()) },
                                html! { (f.human_size()) },
                                html! { (format_time(f.last_modified_time)) },
                                html! { (format_time(f.last_access_time)) },
                            ],
                            html! { (format_time(f.creation_time)) },
                            options,
                            native_token,
                        ))
                    }
                }
            }
        }
    }
}

/// 搜索结果表: 4 列 (名称/位置/大小/修改时间). 行尾 action 与目录表同一簇
/// (最后单元格内); native-open 只在完整搜索页有 token 时出现. 位置列: 父目录
/// 整条 UTF-8 时链接 (保留 hidden, 不带 q), 否则逐段显示 codec.
fn search_table(
    result: &SearchResult,
    options: ListingOptions,
    with_ids: bool,
    native_token: Option<&str>,
) -> Markup {
    html! {
        section class="cb-view cb-view-list" id=[with_ids.then_some("cb-view-list")] {
            table class="striped" {
                thead {
                    tr { th { "名称" } th { "位置" } th { "大小" } th { "修改时间" } }
                }
                tbody {
                    @for f in &result.entries {
                        (entry_row(
                            f,
                            vec![search_location_cell(f, options), html! { (f.human_size()) }],
                            html! { (format_time(f.last_modified_time)) },
                            options,
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
/// 名称 anchor 的 href 保留有效显示规则 (进入子目录/打开文件不丢规则);
/// href() 为 None 的路径 (非 UTF-8) 是 display-only 行: 只渲染名称, 不产生锚点,
/// 更不能 emit href="" (空 href 点击会导航回当前目录).
fn entry_row(
    f: &FileMeta,
    middle: Vec<Markup>,
    last: Markup,
    options: ListingOptions,
    native_token: Option<&str>,
) -> Markup {
    let label = entry_label_text(f);
    let encoded = f.browser_path_encoded();
    let kind = if f.is_directory { "dir" } else { "file" };
    let name_href = f
        .href()
        .map(|href| format!("{href}?{}", listing_query(options)));
    let body = html! {
        @if let Some(href) = name_href {
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
    // data-native-path-encoded 只在可操作路径出现 (None 时属性整个省略);
    // data-kind (dir|file) 恒携带 — JS 的 PDF 判定是 kind + 后缀双条件
    html! {
        tr class="cb-row" data-browser-entry data-kind=(kind)
            data-native-path-encoded=[encoded.as_deref()] title=(label) aria-label=(label) {
            (body)
        }
    }
}

/// 行尾 action 簇 (仅 actionable 行): 预览 / 新 tab (target=_blank rel=noopener) /
/// 下载 (?download=1, 仅文件 — 目录忽略 download 模式) / native-open (仅 token
/// 存在时). 只渲染于最后单元格内; action 不携带显示规则 (响应模式语义不变).
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
/// 图片用 lazy img + 精确 raw URL (无模式 query), 非图片/目录用扩展名 badge;
/// 可操作 label 是原生 anchor (href 保留有效显示规则). 默认 hidden, 由 JS 切换
/// (列表/网格都渲染). id 只出现在完整页 (fragment 可嵌入已有页面).
fn entry_grid(files: &[FileMeta], options: ListingOptions, with_ids: bool) -> Markup {
    html! {
        section class="cb-view cb-view-grid" id=[with_ids.then_some("cb-view-grid")] hidden {
            div.cb-grid {
                @for f in files {
                    (grid_item(f, options))
                }
            }
        }
    }
}

fn grid_item(f: &FileMeta, options: ListingOptions) -> Markup {
    let label = entry_label_text(f);
    let encoded = f.browser_path_encoded();
    let kind = if f.is_directory { "dir" } else { "file" };
    let img_src = is_image_entry(f).then(|| f.href()).flatten();
    let name_href = f
        .href()
        .map(|href| format!("{href}?{}", listing_query(options)));
    let body = html! {
        @if let Some(src) = img_src {
            div.cb-grid-thumb { img loading="lazy" decoding="async" src=(src) alt=""; }
        } @else {
            (grid_badge(f))
        }
        div.cb-grid-label {
            @if let Some(href) = name_href {
                a href=(href) { (entry_label(f)) }
            } @else {
                (entry_label(f))
                span.cb-non-utf8 { "非 UTF-8 名称（仅显示）" }
            }
        }
        div.cb-grid-size { (f.human_size()) }
    };
    html! {
        div class="cb-grid-item" data-browser-entry data-kind=(kind)
            data-native-path-encoded=[encoded.as_deref()] title=(label) aria-label=(label) {
            (body)
        }
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
/// 否则逐段显示 codec. 位置导航延续有效显示规则但移除 q (实际离开搜索).
fn search_location_cell(f: &FileMeta, options: ListingOptions) -> Markup {
    match f.relative_to_root.parent() {
        None => html! { "/" },
        Some(parent) if parent.as_os_str().is_empty() => {
            html! { a href=(path_href("", options)) { "/" } }
        }
        Some(parent) => match encode_relative_path(parent) {
            Some(encoded) => html! {
                a href=(path_href(&encoded, options)) { (path_label_markup(parent)) }
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

/// 表头: 每列一个排序链接 (完整 `?sort=…&hidden=…` query), 当前列带 ▲/▼ 箭头
/// 与 aria-sort, 再次点击切换方向; 链接与工具区排序入口共用同一状态流程.
fn table_header(sort_by: SortBy, options: ListingOptions, q: Option<&str>) -> Markup {
    html! {
        tr {
            @for column in SortColumn::ALL {
                @let next_order = if column == sort_by.column && sort_by.order == SortOrder::Asc {
                    SortOrder::Desc
                } else {
                    SortOrder::Asc
                };
                @let active = column == sort_by.column;
                th aria-sort=[active.then_some(match sort_by.order {
                    SortOrder::Asc => "ascending",
                    SortOrder::Desc => "descending",
                })] {
                    a href=(sort_href(q, options, SortBy { column, order: next_order }))
                        data-cb-sort=(format!("{column}:{next_order}")) {
                        (format!("{column}{}", if active { sort_order_arrow(sort_by.order) } else { "" }))
                    }
                }
            }
        }
    }
}

/// 文档内容片段: 恰好一个外层 `<div class="cb-doc">…</div>`, 无 doctype/head/style.
/// 内容样式全部作用域在 `.cb-doc` 下 (chapbook-doc.css), 可嵌入宿主页面;
/// 完整页与 Fragment 路由共用此包装.
pub fn doc_fragment(body: &str) -> String {
    html! {
        div class="cb-doc" {
            (maud::PreEscaped(body))
        }
    }
    .into_string()
}

/// 文档/代码页面骨架 (独立阅读表面): 自组 `<!DOCTYPE>` / head / title / body.
/// body 已由调用方转义, 以 PreEscaped 原样嵌入, 并经 `doc_fragment` 包装
/// (恰好一次). 工具区随 body 输出统一 surface/选项 metadata: 三个视图图标
/// 均无选中态, 隐藏/排序禁用; 列表入口指向父目录 (identity 由 routes 提供,
/// 绝不从显示标题重建). no native token.
pub fn doc_page(
    title: &str,
    body: &str,
    encoded_path: Option<&str>,
    options: ListingOptions,
) -> String {
    let list_href = encoded_path.map(|encoded| parent_dir_href(encoded, options));
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) }
                // KaTeX 数学排版 (服务端渲染输出的 .katex 结构依赖此样式表与字体)
                link rel="stylesheet" href="/__/static/katex/katex.min.css";
                // 文档/代码/Office 页是树模式外壳宿主 (渐进增强; defer)
                script src="/__/static/js/chapbook-browser.js" defer {}
            }
            body class="cb-doc-page"
                data-cb-surface="reading"
                data-cb-sort=(sort_value(options))
                data-cb-hidden=(hidden_value(options))
                data-native-path-encoded=[encoded_path]
                data-kind="file" {
                (topbar("reading", None, options, list_href.as_deref()))
                (maud::PreEscaped(doc_fragment(body)))
            }
        }
    }
    .into_string()
}

/// 阅读页列表入口目标: encoded 文件 identity 去掉最后一段的父目录 + 有效显示规则;
/// 顶层文件指向 root (`/`).
fn parent_dir_href(encoded: &str, options: ListingOptions) -> String {
    let parent = encoded
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    path_href(parent, options)
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
/// 无条目时返回空串 (页面无 TOC, 浏览器侧不产生任何目录侧栏).
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

    fn meta(rel: PathBuf, is_directory: bool) -> FileMeta {
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

    /// 安全契约: 非 UTF-8 名称在所有条目出口都是 display-only — 名称经显示
    /// codec 出现在 bdi 中, 但不产生 anchor (更没有空 href) 与 action identity.
    #[cfg(unix)]
    #[test]
    fn non_utf8_entries_are_display_only_without_identity() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let bad = PathBuf::from(OsString::from_vec(b"\xFFbad.txt".to_vec()));
        let files = [meta(bad, false)];

        // 目录页: 行 display-only, 无空 href, 无行 identity / 行尾控制.
        // body 自身携带 root 的合法空 identity, 条目行不得再出现 identity.
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &files,
            ListingOptions::default(),
            None,
        )
        .into_string();
        assert!(
            page.contains("<bdi dir=\"auto\">\\xFFbad.txt</bdi>"),
            "page: {page}"
        );
        assert!(page.contains("非 UTF-8 名称（仅显示）"), "page: {page}");
        assert!(!page.contains(r#"href="""#), "page: {page}");
        assert_eq!(
            page.matches("data-native-path-encoded").count(),
            1,
            "only the body identity remains: {page}"
        );
        assert!(
            !page.contains(r#"data-cb-action="preview""#),
            "page: {page}"
        );
        assert!(!page.contains("?download=1"), "page: {page}");

        // 目录 fragment: wrapper identity 保留, 迷你条目无 anchor.
        let frag = dir_fragment(
            Path::new("/srv"),
            Path::new("/srv"),
            &files,
            ListingOptions::default(),
        );
        assert!(
            frag.contains("<bdi dir=\"auto\">\\xFFbad.txt</bdi>"),
            "frag: {frag}"
        );
        assert!(!frag.contains(r#"<a href="/"#), "frag: {frag}");
        assert_eq!(
            frag.matches("data-native-path-encoded").count(),
            1,
            "frag: {frag}"
        );

        // 搜索页: 名称 display-only, 位置链接仍保留有效显示规则.
        let result = SearchResult {
            entries: vec![meta(
                PathBuf::from(OsString::from_vec(b"dir/\xFFleaf.txt".to_vec())),
                false,
            )],
            truncated: false,
        };
        let page = search_page(
            Path::new("/srv"),
            "leaf",
            &result,
            ListingOptions {
                sort_by: SortBy {
                    column: SortColumn::Size,
                    order: SortOrder::Desc,
                },
                show_hidden: true,
            },
            None,
        )
        .into_string();
        assert!(
            page.contains("<bdi dir=\"auto\">\\xFFleaf.txt</bdi>"),
            "page: {page}"
        );
        assert!(
            page.contains(r#"<a href="/dir?sort=Size:Desc&amp;hidden=1">"#),
            "page: {page}"
        );
        assert!(!page.contains(r#"href="""#), "page: {page}");
    }

    /// codec + query 边界: RFC 3986 sub-delims 原样保留在 identity/href 中,
    /// 且行/网格 label 的 href 附加有效显示规则 (无 JS 导航不丢 hidden/sort).
    #[test]
    fn entry_links_preserve_encoded_identity_and_listing_options() {
        let options = ListingOptions {
            sort_by: SortBy {
                column: SortColumn::Size,
                order: SortOrder::Desc,
            },
            show_hidden: true,
        };
        let page = dir_page(
            Path::new("/srv"),
            Path::new("/srv"),
            &[meta(PathBuf::from("a!b$c(d)e*f.txt"), false)],
            options,
            None,
        )
        .into_string();
        assert!(
            page.contains(r#"data-native-path-encoded="a!b$c(d)e*f.txt""#),
            "page: {page}"
        );
        assert!(
            page.contains(r#"<a href="/a!b$c(d)e*f.txt?sort=Size:Desc&amp;hidden=1">"#),
            "page: {page}"
        );
        // 表头快捷入口保留同一 hidden 并携带 data-cb-sort.
        assert!(page.contains(r#"data-cb-sort="Size:Asc""#), "page: {page}");
        assert!(
            page.contains(r#"href="?sort=Size:Asc&amp;hidden=1""#),
            "page: {page}"
        );
    }

    /// root 的 fragment wrapper: encodedPath 是合法空字符串而不是属性缺失.
    #[test]
    fn dir_fragment_root_wrapper_empty_encoded_path() {
        let frag = dir_fragment(
            Path::new("/srv"),
            Path::new("/srv"),
            &[],
            ListingOptions::default(),
        );
        assert!(
            frag.contains(r#"data-native-path-encoded="""#),
            "frag: {frag}"
        );
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
                is_image_entry(&meta(PathBuf::from(name), *is_dir)),
                *expect_image,
                "{name}"
            );
        }
    }
}
