//! 代码高亮 (syntect 5, 纯 Rust, 无子进程): org src 块 / markdown 代码块 / 代码文件共用.
//!
//! 输出 classed HTML fragment: `ClassStyle::Spaced` 把完整 scope atom 作为类名
//! (`keyword` / `string` / `comment` …), 颜色由 chapbook-doc.css 调色板控制
//! (GitHub 亮/暗双模式). 代码文件额外带行号 (`<span class="ln">`).
//!
//! 语法集 = syntect 默认集 (~50 语言) + `assets/syntaxes/` 内嵌补充
//! (TypeScript/TOML/Kotlin/Swift 等默认集缺失的常见语言, 详见 THIRD_PARTY_NOTICES).
//!
//! 提案: docs/2026-08-05-proposal-syntax-highlight-code-files.org (方案 A).

use std::path::Path;
use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};
use syntect::util::LinesWithEndings;

/// 超过该大小的代码文件不做渲染, 直接走 ServeFile (避免内存放大).
pub const MAX_RENDER_BYTES: u64 = 1024 * 1024;

/// 扩展名 -> 高亮 token. 未识别的扩展名维持裸文本现状.
/// 注意: html/htm 不在此映射 — 网页文件应直接交给浏览器渲染, 而不是显示源码.
pub fn language_for_path(path: &Path) -> Option<&'static str> {
    // 无扩展名文件按文件名匹配 (常见构建/容器文件)
    let file_name = path.file_name()?.to_str()?;
    match file_name {
        "Dockerfile" | "Containerfile" => return Some("dockerfile"),
        "Makefile" | "GNUmakefile" | "makefile" => return Some("makefile"),
        _ => {}
    }
    let ext = path.extension()?.to_str()?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "scala" | "sbt" => "scala",
        "java" => "java",
        "py" => "python",
        "js" | "mjs" => "javascript",
        // JSX/TSX 共用 TypeScript 语法 (JSX 是其子集; 内嵌 TS 语法已加 tsx/jsx 扩展)
        "ts" | "tsx" | "mts" | "cts" | "jsx" => "typescript",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "yaml" | "yml" => "yaml",
        "toml" | "tml" => "toml",
        "xml" => "xml",
        "css" => "css",
        "json" => "json",
        "rb" => "ruby",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "php" => "php",
        "swift" => "swift",
        "graphql" | "gql" | "graphqls" => "graphql",
        "dart" => "dart",
        "ex" | "exs" => "elixir",
        "cmake" => "cmake",
        "proto" | "protobuf" => "protobuf",
        "zig" | "zon" => "zig",
        // conf/cfg 由内嵌 INI 语法处理 (构建时已加扩展)
        "ini" | "conf" | "cfg" | "reg" | "inf" => "ini",
        "nix" => "nix",
        "svelte" => "svelte",
        "properties" => "properties",
        "bat" | "cmd" => "bat",
        "hs" | "lhs" => "haskell",
        "pl" | "pm" => "perl",
        "r" => "r",
        "tex" | "ltx" => "latex",
        "diff" | "patch" => "diff",
        _ => return None,
    })
}

/// 内嵌语法集 (syntect 默认集 + 补充语法), 进程内只构建一次.
/// 用 `load_defaults_newlines` 变体: 跨行字符串等上下文依赖行尾换行符参与匹配.
fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(build_syntax_set)
}

/// 内嵌补充语法 (syntect 默认集缺失的常见语言).
/// 来源: sublimehq/Packages (TOML) 与 sharkdp/bat assets/syntaxes/02_Extra (其余),
/// 许可证见 THIRD_PARTY_NOTICES. 部分语法补充扩展名 (如 TS 语法认 tsx/jsx).
fn build_syntax_set() -> SyntaxSet {
    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/TypeScript.sublime-syntax"),
        &["tsx", "jsx"],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/TOML.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Kotlin.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Swift.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Dockerfile.sublime-syntax"),
        &["Containerfile"],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/GraphQL.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Dart.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Elixir.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/CMake.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Protobuf.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Zig.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/INI.sublime-syntax"),
        &["conf", "cfg"],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Nix.sublime-syntax"),
        &[],
    );
    add_extra(
        &mut builder,
        include_str!("../assets/syntaxes/Svelte.sublime-syntax"),
        &[],
    );
    builder.build()
}

fn add_extra(builder: &mut SyntaxSetBuilder, src: &str, extra_extensions: &[&str]) {
    let mut def = SyntaxDefinition::load_from_str(src, true, None)
        .expect("embedded .sublime-syntax must parse");
    def.file_extensions
        .extend(extra_extensions.iter().map(|s| s.to_string()));
    builder.add(def);
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
