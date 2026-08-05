//! 代码高亮 (syntect 5, 纯 Rust, 无子进程): org src 块 / markdown 代码块 / 代码文件共用.
//!
//! 输出 classed HTML fragment: `ClassStyle::Spaced` 把完整 scope atom 作为类名
//! (`keyword` / `string` / `comment` …), 颜色由 chapbook-doc.css 调色板控制
//! (GitHub 亮/暗双模式). 代码文件额外带行号 (`<span class="ln">`).
//!
//! 提案: docs/2026-08-05-proposal-syntax-highlight-code-files.org (方案 A).

use std::path::Path;
use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// 超过该大小的代码文件不做渲染, 直接走 ServeFile (避免内存放大).
pub const MAX_RENDER_BYTES: u64 = 1024 * 1024;

/// 扩展名 -> 高亮 token. 未识别的扩展名维持裸文本现状.
/// 注意: html/htm 不在此映射 — 网页文件应直接交给浏览器渲染, 而不是显示源码.
pub fn language_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "scala" => "scala",
        "java" => "java",
        "py" => "python",
        "js" | "mjs" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "css" => "css",
        "json" => "json",
        "rb" => "ruby",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "php" => "php",
        "swift" => "swift",
        _ => return None,
    })
}

/// 内嵌语法集 (syntect defaults, 含 ~50 种语言), 进程内只构建一次.
/// 用 `load_defaults_newlines` 变体: 跨行字符串等上下文依赖行尾换行符参与匹配.
fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// 高亮代码为 classed HTML fragment (不含 `<pre>`/`<code>` 包裹, 内容已转义).
/// lang 为 None 或未识别时退化为纯文本 (仅转义, 无 span).
/// numbered 时每行前缀 `<span class="ln">N</span>` (代码文件用; 文档代码块不带行号).
///
/// 用 `ClassedHTMLGenerator` (syntect 官方路径, 输出有效 HTML): span 跨行保持打开
/// 以保留上下文高亮, 行号 span 可能嵌套在前一行上下文 span 内 — 视觉无影响
/// (`.ln` 的 color/user-select 规则定义在调色板之后, 优先级同级别胜出).
pub fn highlight(code: &str, lang: Option<&str>, numbered: bool) -> String {
    let ss = syntax_set();
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, ss, ClassStyle::Spaced);
    for line in LinesWithEndings::from(code) {
        if let Err(e) = generator.parse_html_for_line_which_includes_newline(line) {
            tracing::warn!(error = %e, lang = ?lang, "syntect failed to parse line, falling back to escaped text");
            return escape(code);
        }
    }
    let html = generator.finalize();
    if numbered {
        number_lines(&html)
    } else {
        html
    }
}

/// 给每行前缀行号 span. 每行输出都以 '\n' 结尾, 按 '\n' 切分插入行号;
/// 跨行保持打开的 span 会包住后续行的行号, 由 CSS `.ln` 规则接管样式.
fn number_lines(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    for (i, line) in html.split_terminator('\n').enumerate() {
        out.push_str("<span class=\"ln\">");
        out.push_str(&(i + 1).to_string());
        out.push_str("</span>");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// HTML 转义 (syntect 失败兜底; 正常路径由 line_tokens_to_classed_spans 内部转义).
fn escape(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    for c in code.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// 代码文件页面: 完整 `<pre class="sourceCode"><code>…</code></pre>` 块 (带行号).
pub fn code_block(code: &str, lang: Option<&str>) -> String {
    format!(
        "<pre class=\"sourceCode\"><code>{}</code></pre>",
        highlight(code, lang, true)
    )
}
