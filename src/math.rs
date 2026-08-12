//! LaTeX 数学公式渲染 (katex crate: 官方 KaTeX 0.16.7 经 QuickJS 执行, 纯服务端, 无前端 JS).
//!
//! 三个入口, md 与 org 共用同一渲染函数:
//! - `replace_comrak_math`: comrak 开启 math_dollars/math_latex 扩展后输出
//!   `<span data-math-style="inline|display">…</span>`, 后处理替换为 KaTeX HTML
//! - `org_text_html`: orgize 不解析 LaTeX (Element 枚举无数学节点), 在 Text 元素上
//!   tokenize `\(..\)` `\[..\]` `$..$` `$$..$$` `\begin{env}..\end{env}`, 数学段渲染,
//!   普通文本照常 HTML 转义
//!
//! 降级策略 (org 侧关键): 任何启发式或 KaTeX 解析失败都回退原文显示 — `$ column: $`、
//! `$2 == 'patch'$` 这类伪数学 (shell/Haskell 语境) 不会被误渲染; src/example/code 块
//! 是 orgize 独立元素, 不经过 Text, 天然隔离.
//!
//! 每次渲染走 katex crate 的 thread_local 引擎 (每线程首次调用初始化 + 求值
//! katex.min.js, 之后复用), tokio worker 线程有限, 冷启动次数有界, 无需自建缓存.

use katex::Opts;

/// 单条数学内容的最大长度 (字符). 超过视为非数学文本, 保持原文 —
/// 防 `$` 跨段误配对把大段正文吞进 KaTeX (解析失败也要白等).
const MAX_MATH_LEN: usize = 2000;

/// 渲染一段 LaTeX 为 KaTeX HTML; 解析失败或超长返回 None (调用方回退原文).
pub fn render_math(latex: &str, display: bool) -> Option<String> {
    if latex.is_empty() || latex.len() > MAX_MATH_LEN {
        return None;
    }
    let opts = Opts::builder().display_mode(display).build().ok()?;
    katex::render_with_opts(latex, opts).ok()
}

/// 把 org 文本 (单个 Text 元素的值) 转为 HTML: 数学段渲染, 其余转义.
/// 与 DefaultHtmlHandler 对 Text 的转义语义一致 (HtmlEscape), 数学输出是
/// KaTeX 生成的受信 HTML, 原样嵌入.
pub fn org_text_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 64);
    for part in tokenize(value) {
        match part {
            OrgPart::Text(s) => push_html_escape(&mut out, s),
            OrgPart::Math { latex, display } => {
                match render_math(latex, display) {
                    Some(html) => out.push_str(&html),
                    // 解析失败: 恢复原文, 保证内容不丢
                    None => push_html_escape(&mut out, latex),
                }
            }
        }
    }
    out
}

/// org 文本的数学分段.
enum OrgPart<'a> {
    /// 普通文本 (需转义)
    Text(&'a str),
    /// 数学片段 (原始 LaTeX, 含定界符内的内容)
    Math { latex: &'a str, display: bool },
}

/// org 数学 tokenize: 按优先级扫描 `\(`/`\[` 配对、`\begin{env}..\end{env}`
/// (同环境名, 含 `*` 后缀)、`$$..$$`、`$..$`.
/// 行内 `$..$` 套用 org-mode 启发式: 内容非空、首尾非空白; 跨段风险由
/// MAX_MATH_LEN 封顶; 最终防线是 KaTeX 解析失败回退原文.
fn tokenize(value: &str) -> Vec<OrgPart<'_>> {
    let mut parts = Vec::new();
    let mut rest = value;
    while !rest.is_empty() {
        if let Some((head, latex, tail)) = take_pair(rest, "\\(", "\\)") {
            parts.push(OrgPart::Text(head));
            parts.push(OrgPart::Math {
                latex,
                display: false,
            });
            rest = tail;
            continue;
        }
        if let Some((head, latex, tail)) = take_pair(rest, "\\[", "\\]") {
            parts.push(OrgPart::Text(head));
            parts.push(OrgPart::Math {
                latex,
                display: true,
            });
            rest = tail;
            continue;
        }
        if let Some((head, latex, tail)) = take_environment(rest) {
            parts.push(OrgPart::Text(head));
            parts.push(OrgPart::Math {
                latex,
                display: true,
            });
            rest = tail;
            continue;
        }
        if let Some((head, latex, tail)) = take_dollar(rest, true) {
            parts.push(OrgPart::Text(head));
            parts.push(OrgPart::Math {
                latex,
                display: true,
            });
            rest = tail;
            continue;
        }
        if let Some((head, latex, tail)) = take_dollar(rest, false) {
            parts.push(OrgPart::Text(head));
            parts.push(OrgPart::Math {
                latex,
                display: false,
            });
            rest = tail;
            continue;
        }
        // 无定界符命中: 整段都是普通文本
        parts.push(OrgPart::Text(rest));
        break;
    }
    parts
}

/// 找 `open`..`close` 配对; 返回 (定界符前文本, 内容, 定界符后剩余).
/// `\(..\)`/`\[..\]` 不套用首尾空白检查 (org 文档里 `\( \sqrt{2} \)` 常见);
/// `$..$` 的启发式由 take_dollar 负责.
fn take_pair<'a>(value: &'a str, open: &str, close: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let start = value.find(open)?;
    let content_start = start + open.len();
    let end = value[content_start..].find(close)?;
    let content_end = content_start + end;
    let latex = &value[content_start..content_end];
    if latex.is_empty() || latex.len() > MAX_MATH_LEN {
        return None;
    }
    Some((&value[..start], latex, &value[content_end + close.len()..]))
}

/// `\begin{NAME}..\end{NAME}` (NAME 可为 `align*` 等, 原样匹配).
/// 返回的 latex 含 `\begin{..}\end{..}` 包裹 — KaTeX 的 align 等环境必须带包裹解析.
/// 环境名限定常见数学环境, 避免误吞任意 `\begin{...}` 文本.
fn take_environment(value: &str) -> Option<(&str, &str, &str)> {
    const ENVS: &[&str] = &[
        "align",
        "align*",
        "aligned",
        "equation",
        "equation*",
        "gather",
        "gather*",
        "gathered",
        "split",
        "multline",
        "multline*",
        "matrix",
        "pmatrix",
        "bmatrix",
        "vmatrix",
        "Vmatrix",
        "Bmatrix",
        "cases",
        "array",
        "subarray",
        "smallmatrix",
        "eqnarray",
        "eqnarray*",
        "displaymath",
        "math",
        "verbatim",
    ];
    const OPEN: &str = "\\begin{";
    const CLOSE: &str = "\\end{";

    let start = value.find(OPEN)?;
    let head = &value[..start];
    let body_start = start + OPEN.len();
    let name_end = value[body_start..].find('}')? + body_start;
    let name = &value[body_start..name_end];
    if !ENVS.contains(&name) {
        return None;
    }
    let close_pat = format!("{CLOSE}{name}}}");
    let after_begin = &value[name_end + 1..];
    let end = after_begin.find(&close_pat)?;
    let content_end = name_end + 1 + end;
    let latex = &value[start..content_end + close_pat.len()];
    if latex.len() > MAX_MATH_LEN {
        return None;
    }
    let tail = &after_begin[end + close_pat.len()..];
    Some((head, latex, tail))
}

/// `$$..$$` (display) 或 `$..$` (inline).
fn take_dollar(value: &str, double: bool) -> Option<(&str, &str, &str)> {
    let delim = if double { "$$" } else { "$" };
    let start = value.find(delim)?;
    let content_start = start + delim.len();
    // 内容里再出现的 `$$`/`$` 优先作为闭合 (不做嵌套扫描 — LaTeX 里 `$` 不嵌套)
    let end = value[content_start..].find(delim)?;
    let content_end = content_start + end;
    let latex = &value[content_start..content_end];
    if latex.len() > MAX_MATH_LEN {
        return None;
    }
    if !double && (latex.starts_with(char::is_whitespace) || latex.ends_with(char::is_whitespace)) {
        return None;
    }
    if latex.is_empty() {
        return None;
    }
    Some((&value[..start], latex, &value[content_end + delim.len()..]))
}

/// 替换 comrak 输出的数学 span `<span data-math-style="inline|display">…</span>`
/// 为 KaTeX HTML. 内容经 comrak 转义, 先反转义再渲染; 渲染失败保留原 span 原文
/// (comrak 判定为数学的内容至少定界符合法, 失败场景是 KaTeX 不认识的宏).
pub fn replace_comrak_math(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        let Some((span_start, content_off, style)) = find_math_span(rest) else {
            out.push_str(rest);
            break;
        };
        let content = &rest[content_off..];
        let Some(end) = content.find("</span>") else {
            out.push_str(rest);
            break;
        };
        let span_end = content_off + end + "</span>".len();
        out.push_str(&rest[..span_start]);
        let unescaped = unescape_html(&content[..end]);
        match render_math(&unescaped, style == "display") {
            Some(html) => out.push_str(&html),
            None => out.push_str(&rest[span_start..span_end]),
        }
        rest = &rest[span_end..];
    }
    out
}

/// 找下一个数学 span; 返回 (开标签起点, 内容起点, inline|display).
fn find_math_span(html: &str) -> Option<(usize, usize, &'static str)> {
    const INLINE: &str = r#"<span data-math-style="inline">"#;
    const DISPLAY: &str = r#"<span data-math-style="display">"#;
    let i = html.find(INLINE);
    let d = html.find(DISPLAY);
    match (i, d) {
        (Some(a), Some(b)) => {
            if a < b {
                Some((a, a + INLINE.len(), "inline"))
            } else {
                Some((b, b + DISPLAY.len(), "display"))
            }
        }
        (Some(a), None) => Some((a, a + INLINE.len(), "inline")),
        (None, Some(b)) => Some((b, b + DISPLAY.len(), "display")),
        (None, None) => None,
    }
}

/// comrak/HTML 实体反转义 (数学内容里可能出现的五个标准实体).
fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let (entity, len) = if tail.starts_with("&lt;") {
            ("<", 4)
        } else if tail.starts_with("&gt;") {
            (">", 4)
        } else if tail.starts_with("&quot;") {
            ("\"", 6)
        } else if tail.starts_with("&#39;") {
            ("'", 5)
        } else if tail.starts_with("&amp;") {
            ("&", 5)
        } else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        out.push_str(entity);
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

/// 与 orgize DefaultHtmlHandler 一致的 HTML 转义 (org 普通文本).
fn push_html_escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_inline_and_display_math() {
        // 数学段渲染为 KaTeX HTML (class="katex"), 普通文本保留
        let html = org_text_html(r"设其第 \( \sqrt{2} \) 项的渐进分数为: $a_{i-1}$ 与 $x_1 + y_2$");
        assert!(html.contains("class=\"katex\""), "{html}");
        assert!(html.contains("设其第"), "{html}");
        assert!(html.contains("项的渐进分数为"), "{html}");
    }

    #[test]
    fn org_dollar_heuristics_keep_fake_math() {
        // 首尾空白 / KaTeX 解析失败 → 原文 (orgize 同款转义, `'` -> &apos;)
        let html = org_text_html(r"价格 $ column: $ 与 $2 == 'patch' || $ 不变");
        assert!(!html.contains("katex"), "{html}");
        assert!(html.contains("$ column: $"), "{html}");
        assert!(html.contains("$2 == &apos;patch&apos; || $"), "{html}");
    }

    #[test]
    fn org_display_env_and_dollar_wrap() {
        let html = org_text_html(
            "\\begin{align}\nx &= 1 \\\\\ny &= 2\n\\end{align}\n\n$$G^2_{i-1} = Q_i$$\n",
        );
        assert_eq!(html.matches(r#"class="katex""#).count(), 2, "{html}");
        // KaTeX 渲染 align 输出 mtable
        assert!(html.contains("<mtable"), "{html}");
    }

    #[test]
    fn org_plain_text_escaped() {
        let html = org_text_html("<tag> & \"quoted\"");
        assert_eq!(html, "&lt;tag&gt; &amp; &quot;quoted&quot;");
    }

    #[test]
    fn comrak_spans_replaced() {
        // 内容经 comrak 转义 (`&lt;`), 反转义后渲染
        let html = replace_comrak_math(
            r#"<p><span data-math-style="inline">\frac{1}{2} &lt; 1</span> and <span data-math-style="display">\frac{a}{b}</span></p>"#,
        );
        assert_eq!(html.matches("class=\"katex\"").count(), 2, "{html}");
        assert!(!html.contains("data-math-style"), "{html}");
    }

    #[test]
    fn comrak_span_fallback_on_parse_error() {
        // `\badmacro` KaTeX 解析失败 → 保留原 span (内容不丢)
        let html = replace_comrak_math(r#"<span data-math-style="inline">\badmacro{x}</span>"#);
        assert!(html.contains(r#"data-math-style="inline""#), "{html}");
        assert!(html.contains(r"\badmacro{x}"), "{html}");
    }
}
