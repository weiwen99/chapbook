//! HTML 生成 (maud).

use std::collections::HashSet;
use std::path::Path;

use maud::{DOCTYPE, Markup, html};

use crate::meta::{FileMeta, format_time};
use crate::sort::{SortBy, SortColumn, SortOrder};

/// 目录列表页面
pub fn dir_page(root: &Path, path: &Path, files: &[FileMeta], sort_by: SortBy) -> Markup {
    let simple_dir_name = format!(
        "/{}",
        path.strip_prefix(root).unwrap_or(Path::new("")).display()
    );
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no, maximum-scale=1, minimum-scale=1";
                link rel="stylesheet" href="/__/static/css/materialize.min.css";
                link rel="stylesheet" href="/__/static/css/chapbook-theme.css";
                script src="/__/static/js/materialize.min.js" {}
                title { (simple_dir_name) }
            }
            body style="padding: 0 1em 0 1em;" {
                h6 { (format!("Index of {simple_dir_name}")) }
                table class="striped" {
                    // 表头放进 thead: 浏览器会把 table 下的裸 tr 包进隐式 tbody,
                    // 导致表头被条纹染色 (v2 的灰色带就是这么来的)
                    thead { (table_header(sort_by)) }
                    tbody {
                        @if path != root {
                            // colspan 覆盖全行: 单 td 行的条纹/背景只会染到单元格区域
                            tr { td colspan="6" { a href="../" { "../" } } }
                        }
                        @for f in files {
                            tr {
                                td { a href=(f.href()) { (f.name) } }
                                td { (f.type_str()) }
                                td { (f.human_size()) }
                                td { (format_time(f.last_modified_time)) }
                                td { (format_time(f.last_access_time)) }
                                td { (format_time(f.creation_time)) }
                            }
                        }
                    }
                }
            }
        }
    }
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

/// 文档/代码页面骨架: 自组 `<!DOCTYPE>` / head / title / body.
/// org 路径的页面骨架由这里接管 (pandoc -s 不再参与); body 已由调用方转义,
/// 以 PreEscaped 原样嵌入. 样式仍由 `inject_doc_style` 在 </head> 前注入.
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
            body { (maud::PreEscaped(body)) }
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
