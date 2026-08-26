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
            "createGenerationGuard",
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
    /// Task 1 行为导向源不变量: canonical transport key seam 必须锁定。
    /// - 纯函数 canonicalTransportKey 存在, 且位于 actionUrl 之前;
    /// - 请求路径与 wrapper path 都在 commitPane 中经该 seam 绑定;
    /// - transport key 构造段不 decode (no-decode action 契约保留);
    /// - 编码斜杠 `%2F` 必须拒绝 (byte === 0x2f);
    /// - 经 CommonJS 钩子暴露 (浏览器无全局污染)。
    #[test]
    fn browser_js_canonical_transport_binding_seam_is_locked() {
        let js = BROWSER_JS;
        for seam in [
            "function canonicalTransportKey(",
            "canonicalTransportKey(encodedPath)",
            "canonicalTransportKey(wrapperPath)",
        ] {
            assert!(js.contains(seam), "missing transport seam: {seam}");
        }
        let start = js
            .find("function canonicalTransportKey(")
            .expect("transport-key function");
        let end = js[start..]
            .find("function actionUrl(")
            .map(|n| start + n)
            .expect("actionUrl after transport key");
        let body = &js[start..end];
        assert!(
            !body.contains("decodeURIComponent("),
            "action transport must never decode"
        );
        assert!(
            body.contains("byte === 0x2f"),
            "encoded slash must be rejected"
        );
        assert!(js.contains("canonicalTransportKey: canonicalTransportKey"));
        // 绑定强度: commitPane 内 equality 守卫必须先于 wrapper action URL 构造,
        // 且 DOM/history 提交必须发生在守卫之后; requested/wrapper key 绝不直接
        // 参与 action URL 构造.
        let cp_start = js.find("function commitPane(").expect("commitPane 存在");
        let cp_end = js
            .find("function clearTreeCurrent(")
            .expect("clearTreeCurrent 位于 commitPane 之后");
        let cp_body = &js[cp_start..cp_end];
        let eq = cp_body
            .find("requestedKey === wrapperKey")
            .expect("equality 守卫存在");
        let action = cp_body
            .find("actionUrl(wrapperPath, 'full')")
            .expect("wrapper action URL 构造存在");
        assert!(eq < action, "equality 守卫必须先于 action URL 构造");
        for mutation in [
            "replaceChildren(wrapper)",
            "history.replaceState",
            "history.pushState",
        ] {
            let at = cp_body
                .find(mutation)
                .unwrap_or_else(|| panic!("commitPane 必须包含 {mutation}"));
            assert!(action < at, "{mutation} 必须发生在 binding 守卫之后");
        }
        assert!(
            !cp_body.contains("actionUrl(requestedKey"),
            "requested key 绝不直接用于 action URL"
        );
        assert!(
            !cp_body.contains("actionUrl(wrapperKey"),
            "wrapper key 绝不直接用于 action URL"
        );
    }
    /// 可执行行为矩阵: 在 QuickJS 中运行真实的 canonicalTransportKey, 覆盖
    /// 非规范 %HH / 大小写混合 / 字面 Unicode / 服务端保留 ASCII /
    /// 字面 + % 与反斜杠 / 编码斜杠拒绝 / dot segment 拒绝 / 非法 % /
    /// 空串 root / 孤立代理项拒绝 / 合法 emoji 与 U+FFFD 的 UTF-8 大写百分号编码。
    /// 与 Node 矩阵同构 (quick-js 核心无 TextEncoder, 测试内供 WHATWG 同语义
    /// 单标量 UTF-8 polyfill; BROWSER_JS 本身仅把 encode 结果逐字节 percent 化)。
    #[test]
    fn browser_js_canonical_transport_key_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        // CommonJS 钩子 shim: 与 Node 的 module.exports 语义一致 (浏览器无 module).
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        // QuickJS 核心无 TextEncoder (WHATWG encode 语义: 孤立代理项替换为 U+FFFD,
        // 单标量 UTF-8); BROWSER_JS 仅逐字节 percent 化 encode 结果, 由测试供 polyfill.
        ctx.eval(
            r#"
            globalThis.TextEncoder = function TextEncoder() {
              this.encode = function (str) {
                var bytes = [];
                for (var i = 0; i < str.length;) {
                  var cp = str.codePointAt(i);
                  if (cp >= 0xD800 && cp < 0xE000) cp = 0xFFFD;
                  if (cp < 0x80) {
                    bytes.push(cp);
                  } else if (cp < 0x800) {
                    bytes.push(0xC0 | (cp >> 6), 0x80 | (cp & 0x3F));
                  } else if (cp < 0x10000) {
                    bytes.push(0xE0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3F), 0x80 | (cp & 0x3F));
                  } else {
                    bytes.push(
                      0xF0 | (cp >> 18),
                      0x80 | ((cp >> 12) & 0x3F),
                      0x80 | ((cp >> 6) & 0x3F),
                      0x80 | (cp & 0x3F)
                    );
                  }
                  i += cp > 0xFFFF ? 2 : 1;
                }
                return bytes;
              };
            };
            "#,
        )
        .expect("test harness setup");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        let cases: &[(&str, Option<&str>)] = &[
            ("", Some("")),
            ("%41GENTS.md", Some("AGENTS.md")),
            ("%2b.txt", Some("%2B.txt")),
            ("中.md", Some("%E4%B8%AD.md")),
            ("%E4%b8%AD.md", Some("%E4%B8%AD.md")),
            ("!%24()*", Some("!$()*")),
            ("~!$()*", Some("~!$()*")),
            ("a+b.md", Some("a%2Bb.md")),
            ("a%25b.md", Some("a%25b.md")),
            ("a\\b.md", Some("a%5Cb.md")),
            ("a%5cb.md", Some("a%5Cb.md")),
            ("a%2Fb.md", None),
            (".", None),
            ("%2e", None),
            ("..", None),
            ("%2e%2e", None),
            (".%2e", None),
            ("%2e.", None),
            ("bad%", None),
            ("😀.md", Some("%F0%9F%98%80.md")),
            ("\u{FFFD}.md", Some("%EF%BF%BD.md")),
        ];
        for (input, expected) in cases {
            let literal = serde_json::to_string(input).expect("json string literal");
            let got = ctx
                .eval(&format!("module.exports.canonicalTransportKey({literal})"))
                .expect("canonicalTransportKey");
            match expected {
                Some(exp) => assert_eq!(
                    got.as_str(),
                    Some(*exp),
                    "input {input:?}: expected {exp:?}, got {got:?}"
                ),
                None => assert!(
                    matches!(got, quick_js::JsValue::Null),
                    "input {input:?}: expected null, got {got:?}"
                ),
            }
        }
        // 孤立代理项无法在 Rust str 中表示 (非合法标量), 经 JS 转义构造输入;
        // 现行实现 String.fromCodePoint 会抛 RangeError → RED, 修复后返回 null.
        for literal in ["'\\uD800'", "'\\uDC00'"] {
            let js = format!("module.exports.canonicalTransportKey({literal})");
            let got = ctx.eval(&js).expect("canonicalTransportKey must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Null),
                "lone surrogate {literal}: expected null, got {got:?}"
            );
        }
    }
}
