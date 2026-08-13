//! 内嵌静态资源 (Materialize v2.3.3, materializecss/materialize 社区维护分支;
//! KaTeX v0.16.7, 与 katex crate 内嵌的 JS 版本一致, 见 THIRD_PARTY_NOTICES).

pub const MATERIALIZE_CSS: &str = include_str!("../assets/materialize.min.css");
pub const MATERIALIZE_JS: &str = include_str!("../assets/materialize.min.js");
pub const THEME_CSS: &str = include_str!("../assets/chapbook-theme.css");

/// KaTeX 样式表: 自官方 dist 裁剪 (仅保留 woff2 字体引用, 见 THIRD_PARTY_NOTICES),
/// 配套 woff2 字体走二进制内嵌 (KATEX_FONTS).
pub const KATEX_CSS: &str = include_str!("../assets/katex/katex.min.css");

/// KaTeX 字体 (woff2): 每个条目是 (文件名, 内容), 由 routes 按名服务.
pub const KATEX_FONTS: &[(&str, &[u8])] = &[
    (
        "KaTeX_AMS-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_AMS-Regular.woff2"),
    ),
    (
        "KaTeX_Caligraphic-Bold.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Caligraphic-Bold.woff2"),
    ),
    (
        "KaTeX_Caligraphic-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Caligraphic-Regular.woff2"),
    ),
    (
        "KaTeX_Fraktur-Bold.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Fraktur-Bold.woff2"),
    ),
    (
        "KaTeX_Fraktur-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Fraktur-Regular.woff2"),
    ),
    (
        "KaTeX_Main-Bold.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Main-Bold.woff2"),
    ),
    (
        "KaTeX_Main-BoldItalic.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Main-BoldItalic.woff2"),
    ),
    (
        "KaTeX_Main-Italic.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Main-Italic.woff2"),
    ),
    (
        "KaTeX_Main-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Main-Regular.woff2"),
    ),
    (
        "KaTeX_Math-BoldItalic.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Math-BoldItalic.woff2"),
    ),
    (
        "KaTeX_Math-Italic.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Math-Italic.woff2"),
    ),
    (
        "KaTeX_SansSerif-Bold.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_SansSerif-Bold.woff2"),
    ),
    (
        "KaTeX_SansSerif-Italic.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_SansSerif-Italic.woff2"),
    ),
    (
        "KaTeX_SansSerif-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_SansSerif-Regular.woff2"),
    ),
    (
        "KaTeX_Script-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Script-Regular.woff2"),
    ),
    (
        "KaTeX_Size1-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Size1-Regular.woff2"),
    ),
    (
        "KaTeX_Size2-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Size2-Regular.woff2"),
    ),
    (
        "KaTeX_Size3-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Size3-Regular.woff2"),
    ),
    (
        "KaTeX_Size4-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Size4-Regular.woff2"),
    ),
    (
        "KaTeX_Typewriter-Regular.woff2",
        include_bytes!("../assets/katex/fonts/KaTeX_Typewriter-Regular.woff2"),
    ),
];

/// 浏览器渐进增强 JS (目录页/搜索页): 编译期内嵌, 由共享页面 head 引用.
pub const BROWSER_JS: &str = include_str!("../assets/chapbook-browser.js");

#[cfg(test)]
mod tests {
    use super::*;

    /// 浏览器 JS 是编译期内嵌的 served constant: 常量与资产文件逐字节一致.
    #[test]
    fn browser_js_is_the_served_asset_constant() {
        assert_eq!(BROWSER_JS, include_str!("../assets/chapbook-browser.js"));
        assert!(!BROWSER_JS.is_empty());
    }

    /// 行为导向的 JS 安全不变量 (非脆弱的子串-only 断言):
    /// - decodeURIComponent 恰出现一次, 且位于 native-open seam (nativeOpenParams) 内、
    ///   早于 DOM 段 — URL actions 全程不 decode;
    /// - 不出现 encodeURIComponent (form 编码只经 URLSearchParams);
    /// - fragment/download 模式只经 searchParams.set 添加;
    /// - 无 location.search 字符串拼接;
    /// - 测试钩子经 CommonJS 守卫暴露, 不污染浏览器全局.
    #[test]
    fn browser_js_security_seam_invariants() {
        let js = BROWSER_JS;
        let decode = "decodeURIComponent(";
        assert_eq!(
            js.matches(decode).count(),
            1,
            "decode 必须恰好一次 (native-open 独占 seam)"
        );
        let decode_at = js.find(decode).expect("decode 调用存在");
        let fn_at = js
            .find("function nativeOpenParams(")
            .expect("nativeOpenParams 函数存在");
        let dom_at = js.find("/* ---------- DOM").expect("DOM 段标记存在");
        assert!(
            fn_at < decode_at && decode_at < dom_at,
            "decode 必须位于 nativeOpenParams 内且早于 DOM 段"
        );
        assert_eq!(js.matches("encodeURIComponent(").count(), 0);
        assert_eq!(js.matches("searchParams.set('fragment'").count(), 1);
        assert_eq!(js.matches("searchParams.set('download'").count(), 1);
        assert!(!js.contains("location.search"), "禁止字符串拼接 query");
        assert!(
            js.contains("typeof module !== 'undefined' && module.exports"),
            "测试钩子必须经 CommonJS 守卫, 浏览器无全局污染"
        );
        assert!(
            js.contains("isValidEncodedPath"),
            "钩子: isValidEncodedPath"
        );
        assert!(js.contains("actionUrl"), "钩子: actionUrl");
        assert!(js.contains("nativeOpenParams"), "钩子: nativeOpenParams");
    }

    /// 评审六项修复的行为导向源不变量 (每项对应一个可观察行为):
    /// - P1: 校验器 charset 允许服务端原样保留的 RFC 3986 sub-delims `!$()*`;
    /// - P2#2: Escape 分支先于通用 editable-target 守卫 (否则聚焦搜索框时被吞掉);
    /// - P2#3: dblclick 守卫排除 action/button/control/link;
    /// - P2#5: 预览并发用 generation 守卫, 关闭面板作废一切在途响应/错误;
    /// - P2#4: 选择决策 seam 被 DOM 使用;
    /// - 三个新纯函数 seam 经 CommonJS 钩子暴露 (浏览器无全局污染).
    #[test]
    fn browser_js_review_fix_seam_invariants() {
        let js = BROWSER_JS;
        assert!(js.contains("!$()*"), "校验器 charset 必须允许 !$()*");
        let esc = js.find("event.key === 'Escape'").expect("Escape 分支存在");
        let guard = js
            .find("isEditableTarget(event.target)")
            .expect("editable 守卫存在");
        assert!(esc < guard, "Escape 必须早于 editable-target 守卫");
        let dblclick_fn = js
            .find("function onDocumentDblclick(")
            .expect("dblclick 处理器存在");
        let dblclick_body = &js[dblclick_fn..];
        assert!(
            dblclick_body.contains("'a, button, input, select, textarea, [data-cb-action]'"),
            "dblclick 守卫必须排除 action/button/control/link"
        );
        assert!(js.contains("previewGuard.begin()"), "新请求作废旧请求");
        assert!(
            js.contains("previewGuard.isCurrent(gen)"),
            "过期响应/错误必须丢弃"
        );
        assert!(
            js.contains("previewGuard.invalidate()"),
            "面板关闭必须作废在途请求"
        );
        assert!(
            js.contains("panelTargetForSelection("),
            "选择决策 seam 必须被 DOM 使用"
        );
        for seam in [
            "escapeAction",
            "panelTargetForSelection",
            "createPreviewGuard",
        ] {
            assert!(js.contains(seam), "seam 缺失: {seam}");
        }
    }

    /// 复评审 P1 的行为导向源不变量: 校验器纯函数段 (DOM 段之前) 必须携带
    /// WHATWG 单/双 dot segment 全部拼写的整段拒绝正则 (ASCII case-insensitive:
    /// `.`, `..`, `%2e`, `%2e%2e`, 含混合拼写 `.%2e` / `%2e.` — URL 解析器会把
    /// 全部六种拼写折叠并改写目标)。字面 `%252e%252e` (服务端把 `%` 编码为
    /// `%25`) 不在拒绝集; 行为矩阵见 Node 复评审脚本 (cb_rereview_matrix_test.js)。
    #[test]
    fn browser_js_rereview_dot_segment_regex_invariants() {
        let js = BROWSER_JS;
        let vfn = js
            .find("function isValidEncodedPath(")
            .expect("校验器函数存在");
        let dom_at = js.find("/* ---------- DOM").expect("DOM 段标记存在");
        let validator_body = &js[vfn..dom_at];
        assert!(
            validator_body.contains(r"^(?:\.|\.\.|%2e|%2e%2e|\.%2e|%2e\.)$/i"),
            "校验器必须整段拒绝 WHATWG dot segment 全部六种拼写 (含混合拼写 .%2e / %2e.)"
        );
        assert!(
            !validator_body.contains("%252e%252e"),
            "字面 %252e%252e 不在拒绝集 (服务端对 % 编码后的字面文件名)"
        );
    }

    /// 复评审 P2 的行为导向源不变量: Escape 分支内 (早于通用 editable-target
    /// 守卫), handled 路径 (blur-search / close-panel) 必须在返回前恰好调用一次
    /// `event.preventDefault()`, 且先于 blur/close 动作; 未处理 (action === null)
    /// 必须先返回, 不取消默认行为。
    #[test]
    fn browser_js_rereview_escape_prevent_default_invariants() {
        let js = BROWSER_JS;
        let esc = js.find("event.key === 'Escape'").expect("Escape 分支存在");
        let guard = js
            .find("isEditableTarget(event.target)")
            .expect("editable 守卫存在");
        let esc_block = &js[esc..guard];
        assert_eq!(
            esc_block.matches("event.preventDefault()").count(),
            1,
            "handled Escape 必须在返回前恰好调用一次 preventDefault"
        );
        let pd = esc_block.find("event.preventDefault()").unwrap();
        let null_return = esc_block
            .find("action === null")
            .expect("未处理分支存在 (先返回, 不取消)");
        assert!(
            null_return < pd,
            "未处理 Escape 必须先返回, 不调用 preventDefault"
        );
        assert!(
            pd < esc_block
                .find("input.blur()")
                .expect("blur-search 分支存在"),
            "preventDefault 必须先于失焦动作"
        );
        assert!(
            pd < esc_block
                .find("closePanel()")
                .expect("close-panel 分支存在"),
            "preventDefault 必须先于关闭面板动作"
        );
    }
}
