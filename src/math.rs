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

/// org 数学 tokenize: 按**位置**扫描 (而非类型优先级), 每次处理最早出现的定界符.
/// 五种定界符: `\(..\)` `\[..\]` `\begin{env}..\end{env}` `$$..$$` `$..$`.
/// 关键: `\begin{...}` 常出现在段落中段, 若把其前文本整体当普通文本, 前面的
/// `$..$` 数学会丢失 (真实文档复现); 启发式不合格 (行内 `$` 首尾空白) 时把
/// 开定界符当普通文本从其后面继续扫描, 不丢弃段落其余合法数学.
fn tokenize(value: &str) -> Vec<OrgPart<'_>> {
    let mut parts = Vec::new();
    let mut rest = value;
    while !rest.is_empty() {
        // 全部定界符候选, 取 start 最小者
        let mut best: Option<(usize, usize, &str, &str, bool)> = None;
        for c in [
            find_pair(rest, "\\(", "\\)", false),
            find_pair(rest, "\\[", "\\]", true),
            find_environment(rest),
            find_dollar(rest, true),
            find_dollar(rest, false),
        ]
        .into_iter()
        .flatten()
        {
            if best.is_none_or(|(s, ..)| c.0 < s) {
                best = Some(c);
            }
        }
        let Some((start, delim, latex, tail, display)) = best else {
            parts.push(OrgPart::Text(rest));
            break;
        };
        parts.push(OrgPart::Text(&rest[..start]));
        // 行内 `$..$` 启发式: 首尾非空白 (org-mode 同款规则), 防 shell/Haskell 伪数学
        let inline_dollar = !display && delim == 1; // 单个 `$` (区别于 `$$`/`\(`/`\[`/`\begin{`)
        let ok = !latex.is_empty()
            && latex.len() <= MAX_MATH_LEN
            && (!inline_dollar
                || (!latex.starts_with(char::is_whitespace)
                    && !latex.ends_with(char::is_whitespace)));
        if ok {
            parts.push(OrgPart::Math { latex, display });
            rest = tail;
        } else {
            // 定界符按文本处理 (head 已输出): 只补开定界符本身, 继续扫描
            parts.push(OrgPart::Text(&rest[start..start + delim]));
            rest = &rest[start + delim..];
        }
    }
    parts
}

/// 找 `open`..`close` 配对; 返回 (开定界符位置, 定界符长, 内容, 剩余, display).
/// `\(..\)`/`\[..\]` 不套用首尾空白检查 (org 文档里 `\( \sqrt{2} \)` 常见).
fn find_pair<'a>(
    value: &'a str,
    open: &str,
    close: &str,
    display: bool,
) -> Option<(usize, usize, &'a str, &'a str, bool)> {
    let start = value.find(open)?;
    let content_start = start + open.len();
    let end = value[content_start..].find(close)?;
    let latex = &value[content_start..content_start + end];
    if latex.is_empty() || latex.len() > MAX_MATH_LEN {
        return None;
    }
    Some((
        start,
        open.len(),
        latex,
        &value[content_start + end + close.len()..],
        display,
    ))
}

/// `\begin{NAME}..\end{NAME}` (NAME 可为 `align*` 等, 原样匹配).
/// latex 含 `\begin{..}\end{..}` 包裹 — KaTeX 的 align 等环境必须带包裹解析.
/// 环境名限定常见数学环境, 避免误吞任意 `\begin{...}` 文本.
fn find_environment(value: &str) -> Option<(usize, usize, &str, &str, bool)> {
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
    Some((
        start,
        OPEN.len(),
        latex,
        &after_begin[end + close_pat.len()..],
        true,
    ))
}

/// `$$..$$` (display) 或 `$..$` (inline); 返回 (开定界符位置, 定界符长, 内容, 剩余, display).
/// 内容非空且不超限; 行内 `$` 的首尾空白检查由主循环统一处理 (需支持"跳过继续").
fn find_dollar(value: &str, double: bool) -> Option<(usize, usize, &str, &str, bool)> {
    let delim = if double { "$$" } else { "$" };
    let start = value.find(delim)?;
    let content_start = start + delim.len();
    // 内容里再出现的 `$$`/`$` 优先作为闭合 (不做嵌套扫描 — LaTeX 里 `$` 不嵌套)
    let end = value[content_start..].find(delim)?;
    let latex = &value[content_start..content_start + end];
    if latex.is_empty() || latex.len() > MAX_MATH_LEN {
        return None;
    }
    Some((
        start,
        delim.len(),
        latex,
        &value[content_start + end + delim.len()..],
        double,
    ))
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

    #[test]
    fn deep_nested_cfrac_renders() {
        // 回归: QuickJS 默认 256KB JS 栈下, 8 层 \cfrac 嵌套 (pell 连分数文档
        // 的真实公式) 会 InternalError: stack overflow → 回退原文.
        // vendor/quick-js patch 把栈提到 8MB, 此公式必须渲染.
        let latex = "c=a_0+\\cfrac{1}{a_1+\\cfrac{1}{a_2+\\cfrac{1}{a_3+\\cfrac{1}{a_4+\\cfrac{1}{a_5+\\cfrac{1}{a_6+\\cfrac{1}{a_7+\\ldots}}}}}}}\\tag{1}";
        let html = replace_comrak_math(&format!(
            r#"<span data-math-style="display">{latex}</span>"#
        ));
        assert!(html.contains("class=\"katex\""), "{html}");
        assert!(!html.contains("data-math-style"), "{html}");
    }

    #[test]
    fn org_math_before_mid_paragraph_environment() {
        // 回归: `\begin{align}` 出现在段落中段时, 旧实现把其前整段当普通文本,
        // 前面的 `$..$` 数学全部丢失 (内接三角形文档真实场景).
        let html = org_text_html(
            "命题 $1$ : 锐角三角形 $\\triangle{A_1B_1C_1}$ 的最小面积\n\\begin{align*}\n\\Delta_{min} = \\dfrac{2\\Delta^2}{4\\Delta} \\tag{3}\n\\end{align*}",
        );
        // `$1$` + `$\triangle{...}$` + align 环境 = 3 个渲染
        assert_eq!(html.matches("class=\"katex\"").count(), 3, "{html}");
        assert_eq!(html.matches('$').count(), 0, "{html}");
    }
}
