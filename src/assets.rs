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
    /// - commitPane 绑定顺序: requestedKey 赋值 < wrapperKey 赋值 < equality
    ///   守卫 < wrapper action URL 构造 < DOM/history 提交;
    /// - transport key 构造段不 decode (no-decode action 契约保留);
    /// - 编码斜杠 `%2F` 必须拒绝 (byte === 0x2f);
    /// - 经 CommonJS 钩子暴露 (浏览器无全局污染)。
    #[test]
    fn browser_js_canonical_transport_binding_seam_is_locked() {
        let js = BROWSER_JS;
        assert!(
            js.contains("function canonicalTransportKey("),
            "transport-key 函数存在"
        );
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
        // 绑定强度: commitPane 内 requestedKey 必须先于 wrapperKey 绑定赋值,
        // equality 守卫先于 wrapper action URL 构造, DOM/history 提交发生在全部
        // 绑定与守卫之后; requested/wrapper key 绝不直接参与 action URL 构造.
        let cp_start = js.find("function commitPane(").expect("commitPane 存在");
        let cp_end = js
            .find("function clearTreeCurrent(")
            .expect("clearTreeCurrent 位于 commitPane 之后");
        let cp_body = &js[cp_start..cp_end];
        let requested_at = cp_body
            .find("const requestedKey = canonicalTransportKey(encodedPath);")
            .expect("requestedKey 绑定赋值存在");
        let wrapper_at = cp_body
            .find("const wrapperKey = canonicalTransportKey(wrapperPath);")
            .expect("wrapperKey 绑定赋值存在");
        assert!(
            requested_at < wrapper_at,
            "requestedKey 赋值必须先于 wrapperKey 赋值"
        );
        let eq = cp_body
            .find("requestedKey === wrapperKey")
            .expect("equality 守卫存在");
        assert!(wrapper_at < eq, "wrapperKey 绑定必须先于 equality 守卫");
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
    /// 多段路径与段内 %HH 保留 / 前导·尾随·空段拒绝 / 非法 %HH hex 拒绝 /
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
            ("a/b%20c.md", Some("a/b%20c.md")),
            ("a%2Fb.md", None),
            ("/a", None),
            ("a/", None),
            ("a//b", None),
            (".", None),
            ("%2e", None),
            ("..", None),
            ("%2e%2e", None),
            (".%2e", None),
            ("%2e.", None),
            ("bad%", None),
            ("%GG", None),
            ("%2x", None),
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
        // 孤立代理项无法在 Rust str 中表示 (非合法标量); 以 JS 转义字面量构造输入,
        // 必须不抛异常并返回 null.
        for literal in ["'\\uD800'", "'\\uDC00'"] {
            let js = format!("module.exports.canonicalTransportKey({literal})");
            let got = ctx.eval(&js).expect("canonicalTransportKey must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Null),
                "lone surrogate {literal}: expected null, got {got:?}"
            );
        }
    }

    /// 任务 2: 文档布局 seam (installDocLayout 移动 TOC 节点, 不克隆).
    /// 行为切片到真实函数体 (区别于全局 contains): 幂等查询/节点移动顺序,
    /// initFullPageTocSpy 与 commitPane 内的生命周期调用顺序.
    #[test]
    fn browser_js_document_layout_seam_is_locked() {
        let js = BROWSER_JS;
        // installDocLayout: 直系子级已有 layout 直接返回, TOC 原节点先移入 toc 列,
        // 其余原节点再进 content, 最后整体替换 .cb-doc 子节点.
        let inst_start = js
            .find("function installDocLayout(")
            .expect("installDocLayout 存在");
        let inst_end = js
            .find("function installTocSpy(")
            .expect("installTocSpy 位于 installDocLayout 之后");
        let inst = &js[inst_start..inst_end];
        for needle in [
            "docEl.querySelector(':scope > .cb-doc-layout')",
            "if (existing) return existing;",
            "Array.from(docEl.childNodes)",
            "tocHost.append(tocNav)",
            "if (node !== tocNav) content.append(node)",
            "layout.append(tocHost, content)",
            "docEl.replaceChildren(layout)",
            "return layout;",
        ] {
            assert!(inst.contains(needle), "installDocLayout 必须包含 {needle}");
        }
        let append_at = inst
            .find("tocHost.append(tocNav)")
            .expect("TOC append 存在");
        let replace_at = inst
            .find("docEl.replaceChildren(layout)")
            .expect("replaceChildren 存在");
        assert!(
            append_at < replace_at,
            "TOC 原节点必须先 append 进 toc 列, 再整体替换 .cb-doc 子节点"
        );
        // 类名/函数存在性 (全局契约, 与上面的行为切片互补).
        for seam in [
            "function installDocLayout(",
            "cb-doc-layout",
            "cb-doc-toc",
            "cb-doc-content",
        ] {
            assert!(js.contains(seam), "missing document-layout seam: {seam}");
        }

        // initFullPageTocSpy: 先装布局 (TOC 移入 toc 列), 再取 TOC 节点, 最后装 spy.
        let spy_start = js
            .find("function initFullPageTocSpy(")
            .expect("initFullPageTocSpy 存在");
        let spy_end = js
            .find("function installPaneTocSpy(")
            .expect("installPaneTocSpy 位于 initFullPageTocSpy 之后");
        let spy = &js[spy_start..spy_end];
        let layout_at = spy
            .find("installDocLayout(docEl)")
            .expect("installDocLayout 调用存在");
        let nav_at = spy.find("tocNavIn(docEl)").expect("tocNavIn 调用存在");
        let sniff_at = spy
            .find("installTocSpy(docEl, tocNav, null)")
            .expect("installTocSpy 调用存在");
        assert!(
            layout_at < nav_at,
            "initFullPageTocSpy: 先装布局再取 TOC 节点"
        );
        assert!(
            nav_at < sniff_at,
            "initFullPageTocSpy: 先取 TOC 节点再装 spy"
        );

        // commitPane: 旧 pane 拆除 < 新 wrapper 就位 < 文档布局 < pane TOC spy.
        let cp_start = js.find("function commitPane(").expect("commitPane 存在");
        let cp_end = js
            .find("function clearTreeCurrent(")
            .expect("clearTreeCurrent 位于 commitPane 之后");
        let cp = &js[cp_start..cp_end];
        let teardown_at = cp
            .rfind("teardownPane();")
            .expect("成功路径 teardownPane 存在");
        let replace_at = cp
            .find("pane.replaceChildren(wrapper)")
            .expect("pane.replaceChildren(wrapper) 存在");
        let layout_call_at = cp
            .find("installDocLayout(")
            .expect("installDocLayout 调用存在");
        let pane_spy_at = cp
            .find("installPaneTocSpy(wrapper)")
            .expect("installPaneTocSpy 调用存在");
        assert!(
            teardown_at < replace_at,
            "commitPane: 先拆除旧 observer/媒体再替换内容"
        );
        assert!(
            replace_at < layout_call_at,
            "commitPane: 新内容就位后才装文档布局"
        );
        assert!(
            layout_call_at < pane_spy_at,
            "commitPane: 布局就位后才装 TOC spy"
        );
    }

    /// 任务 3: 树宽调整 seam. 常量/状态与行为函数名锁定: 拖拽指针捕获、键盘
    /// ±16 步进、ARIA separator 契约, 以及 pointerdown 对未来 .cb-tree-collapse
    /// 子控件的忽略守卫全部就位 (contains 级契约, 行为矩阵见 Chromium 验收).
    #[test]
    fn browser_js_tree_resize_seams_are_locked() {
        let js = BROWSER_JS;
        for needle in [
            "TREE_WIDTH_KEY = 'cb-tree-width'",
            "function clampTreeWidth(",
            "function setTreeWidth(",
            "role', 'separator'",
            "setPointerCapture",
            "aria-valuenow",
        ] {
            assert!(js.contains(needle), "missing tree-resize seam: {needle}");
        }
        // 键盘步进必须阻断冒泡: stopPropagation 位于 resizer keydown 处理器内且
        // 在应用宽度之前 — 分隔器按键不得冒泡成全局树键 (展开/收起/选中).
        let resizer_kd = js
            .find("treeResizer.addEventListener('keydown',")
            .expect("resizer keydown 处理器存在");
        let stop_at = js
            .find("event.stopPropagation();")
            .expect("resizer 键盘步进阻断冒泡");
        let apply_at = js
            .find("setTreeWidth(next, true)")
            .expect("键盘步进应用宽度");
        assert!(
            resizer_kd < stop_at && stop_at < apply_at,
            "resizer 键盘处理必须先阻断冒泡再应用宽度"
        );
        // onTreeKeydown 防御性守卫: 已 preventDefault 或源自分隔器
        // (.cb-tree-resizer) 的按键不得触发任何全局树键.
        assert!(
            js.contains("event.defaultPrevented || event.target.closest('.cb-tree-resizer')"),
            "onTreeKeydown 必须忽略分隔器/已处理按键"
        );
    }

    /// 任务 3 质量收尾: 拖拽手势所有权纯函数在 QuickJS 中真实执行. 行为矩阵:
    /// 无在途手势时主指针左键可开始 (isPrimary 省略视同主指针); 右键/中键/
    /// 副指针 (isPrimary=false)/已有 owner 一律拒绝; move/end 只认持有手势的
    /// pointerId (owner 匹配 true, 其他 pointerId 或无 owner false).
    #[test]
    fn browser_js_tree_resize_pointer_ownership_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        // CommonJS 钩子 shim: 与 Node 的 module.exports 语义一致 (浏览器无 module).
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        // 期望 true: 主指针左键且无 owner 可开始; owner 的 move/end 通行.
        for expr in [
            "canStartTreeResize({button:0,isPrimary:true}, null)",
            "canStartTreeResize({button:0}, null)",
            "ownsTreeResize({pointerId:7}, 7)",
        ] {
            let js = format!("module.exports.{expr}");
            let got = ctx.eval(&js).expect("ownership helper must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(true)),
                "{expr}: expected true, got {got:?}"
            );
        }
        // 期望 false: 右键/中键/副指针/已有 owner 不可开始; 非 owner 指针或
        // 无 owner 的 move/end 不可通行.
        for expr in [
            "canStartTreeResize({button:2,isPrimary:true}, null)",
            "canStartTreeResize({button:1,isPrimary:true}, null)",
            "canStartTreeResize({button:0,isPrimary:false}, null)",
            "canStartTreeResize({button:0,isPrimary:true}, 7)",
            "canStartTreeResize({button:2,isPrimary:true}, 7)",
            "ownsTreeResize({pointerId:7}, null)",
            "ownsTreeResize({pointerId:8}, 7)",
            "ownsTreeResize({pointerId:7}, 8)",
        ] {
            let js = format!("module.exports.{expr}");
            let got = ctx.eval(&js).expect("ownership helper must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(false)),
                "{expr}: expected false, got {got:?}"
            );
        }
    }

    /// 任务 3 质量收尾: 拖拽手势单 owner + 捕获失败干净回退的源码切片 (全部
    /// 切片止于下一条 sibling 监听或各自闭合标记, 不越过函数边界).
    /// pointerdown 顺序: 折叠守卫 → 主指针守卫 → setPointerCapture → 异常分支
    /// → 激活 id 赋值 → class 添加; 捕获失败在 class 添加前 return (不留
    /// class/监听/持久化状态). move/end 均经 ownsTreeResize 过滤 (多指不得
    /// 移动/结束在途手势); 幂等收尾 onEnd 顺序: owns 守卫 → 复位 id → 移除
    /// 全部监听 (含 lostpointercapture) → 移除 class → 释放捕获 → 持久化一次
    /// (id 先复位, releasePointerCapture 触发的 lostpointercapture 重入直接返回).
    #[test]
    fn browser_js_tree_resize_gesture_ownership_is_locked() {
        let js = BROWSER_JS;
        // 模块状态 + 纯函数 seam (行为由 QuickJS 矩阵测试验证).
        for needle in [
            "let treeResizePointerId = null",
            "function canStartTreeResize(",
            "function ownsTreeResize(",
        ] {
            assert!(
                js.contains(needle),
                "missing gesture-ownership seam: {needle}"
            );
        }
        // pointerdown 切片 (止于下一条 sibling 监听, 不越过 handler 边界):
        // 折叠守卫 < 主指针守卫 < 捕获 < 异常分支 < 激活 id < class.
        let pd_start = js
            .find("treeResizer.addEventListener('pointerdown',")
            .expect("resizer pointerdown 处理器存在");
        let pd_end = js
            .find("treeResizer.addEventListener('keydown',")
            .expect("resizer keydown 处理器存在");
        assert!(
            pd_start < pd_end,
            "pointerdown 处理器必须位于 keydown 监听之前"
        );
        let pd = &js[pd_start..pd_end];
        let collapse_at = pd
            .find("closest('.cb-tree-collapse')")
            .expect("pointerdown 折叠按钮守卫存在");
        let can_start_at = pd
            .find("canStartTreeResize(event, treeResizePointerId)")
            .expect("pointerdown 主指针守卫存在");
        let capture_at = pd
            .find("setPointerCapture(event.pointerId)")
            .expect("pointerdown 指针捕获存在");
        let catch_at = pd
            .find("} catch (err) {")
            .expect("pointerdown 捕获异常分支存在");
        let owner_at = pd
            .find("treeResizePointerId = pointerId")
            .expect("pointerdown 激活手势 id 赋值存在");
        let class_at = pd
            .find("classList.add('cb-tree-resizing')")
            .expect("pointerdown 拖拽 class 添加存在");
        assert!(
            collapse_at < can_start_at
                && can_start_at < capture_at
                && capture_at < catch_at
                && catch_at < owner_at
                && owner_at < class_at,
            "pointerdown 顺序: 折叠守卫 → 主指针守卫 → 捕获 → 异常分支 → 激活 id → class"
        );
        // 捕获失败必须干净返回: catch 块 (止于 owner 赋值, 同一坐标系) 内含
        // return, 先于 owner 激活与 class 添加 — 不留 class/监听/持久化状态.
        let catch_block = &pd[catch_at..owner_at];
        assert!(
            catch_block.contains("return;"),
            "setPointerCapture 失败必须在捕获块内 return (先于 owner 激活与 class 添加)"
        );
        // onMove 定义在 onEnd 之前, 且只认 owner: 非 owner 指针的 move 不得改宽.
        // 两个局部函数切片均止于各自闭合 };, 不吞入后续代码.
        let on_move_at = pd
            .find("const onMove = function (moveEvent) {")
            .expect("onMove 函数存在");
        let on_end_at = pd
            .find("const onEnd = function (endEvent) {")
            .expect("onEnd 收尾函数存在");
        assert!(on_move_at < on_end_at, "onMove 必须先于 onEnd 定义");
        let on_move_close = pd[on_move_at..].find("};").expect("onMove 闭合缺失");
        let on_move_body = &pd[on_move_at..on_move_at + on_move_close];
        assert!(
            on_move_body.contains("ownsTreeResize(moveEvent, treeResizePointerId)"),
            "onMove 必须经 ownsTreeResize 过滤"
        );
        // 监听挂载: move/up/cancel/lostpointercapture 齐全且挂载于启动之后.
        for needle in [
            "addEventListener('pointermove', onMove)",
            "addEventListener('pointerup', onEnd)",
            "addEventListener('pointercancel', onEnd)",
            "addEventListener('lostpointercapture', onEnd)",
        ] {
            let at = pd
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle}"));
            assert!(at > class_at, "{needle} 必须晚于手势启动");
        }
        // onEnd 幂等收尾切片 (止于闭合 };): owns 守卫 → 复位 id → 移除全部监听
        // → 移除 class → 释放捕获 → 持久化一次.
        let on_end_close = pd[on_end_at..].find("};").expect("onEnd 闭合缺失");
        let on_end = &pd[on_end_at..on_end_at + on_end_close];
        let owns_at = on_end
            .find("ownsTreeResize(endEvent, treeResizePointerId)")
            .expect("onEnd 必须校验手势所有权");
        let reset_at = on_end
            .find("treeResizePointerId = null")
            .expect("onEnd 必须复位激活 id");
        let rm_move = on_end
            .find("removeEventListener('pointermove', onMove)")
            .expect("onEnd 移除 pointermove");
        let rm_up = on_end
            .find("removeEventListener('pointerup', onEnd)")
            .expect("onEnd 移除 pointerup");
        let rm_cancel = on_end
            .find("removeEventListener('pointercancel', onEnd)")
            .expect("onEnd 移除 pointercancel");
        let rm_lost = on_end
            .find("removeEventListener('lostpointercapture', onEnd)")
            .expect("onEnd 移除 lostpointercapture");
        let class_rm = on_end
            .find("classList.remove('cb-tree-resizing')")
            .expect("onEnd 移除拖拽 class");
        let release_at = on_end
            .find("releasePointerCapture(pointerId)")
            .expect("onEnd 释放指针捕获");
        let persist_at = on_end
            .find("setTreeWidth(dragWidth, true)")
            .expect("onEnd 持久化偏好");
        assert!(
            owns_at < reset_at
                && reset_at < rm_move
                && rm_move < rm_up
                && rm_up < rm_cancel
                && rm_cancel < rm_lost
                && rm_lost < class_rm
                && class_rm < release_at
                && release_at < persist_at,
            "onEnd 顺序: owns 守卫 → 复位 id → 移除全部监听 → 移除 class → 释放 → 持久化一次"
        );
        assert_eq!(
            on_end.matches("setTreeWidth(dragWidth, true)").count(),
            1,
            "onEnd 只持久化一次"
        );
        // exitTreeMode: 在途手势先安全复位 (移除 resizer 前清空 owner 与 class,
        // 元素移除隐式释放捕获触发的 lostpointercapture 重入此刻已无 owner).
        let exit_start = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 存在");
        let exit = &js[exit_start..];
        let exit_reset = exit
            .find("treeResizePointerId = null")
            .expect("exitTreeMode 复位在途手势");
        let exit_class_rm = exit
            .find("classList.remove('cb-tree-resizing')")
            .expect("exitTreeMode 移除拖拽 class");
        let resizer_rm = exit
            .find("treeResizer.remove()")
            .expect("exitTreeMode 移除 resizer");
        assert!(
            exit_reset < resizer_rm && exit_class_rm < resizer_rm,
            "exitTreeMode 必须先在移除 resizer 前复位在途手势"
        );
    }

    /// 任务 3 跟进: 持久化偏好与生效宽分离. 锁定 treePreferredWidth 只受硬边界
    /// [160,480] 约束 (clampTreePreferred), 生效宽 treeWidth 再叠加 50vw 视口
    /// 上限; persist 只写硬边界截断的偏好 (绝不写入窄屏截断值); 窄屏进入不改写
    /// 偏好, 跨回宽屏时 drawer media handler 重放偏好恢复 CSS 变量与 ARIA.
    #[test]
    fn browser_js_tree_preferred_width_survives_drawer_breakpoint() {
        let js = BROWSER_JS;
        for needle in [
            "function clampTreePreferred(",
            "function loadTreePreferredWidth(",
            "let treePreferredWidth = TREE_DEFAULT_WIDTH",
            "const TREE_DRAWER_MQL = '(width < 64rem)'",
        ] {
            assert!(
                js.contains(needle),
                "missing preferred-width seam: {needle}"
            );
        }
        // persist 分支: 偏好按硬边界截断后写入 localStorage, 而非 50vw 截断的生效宽
        assert!(
            js.contains("treePreferredWidth = clampTreePreferred(width)"),
            "persist 必须以硬边界截断更新偏好"
        );
        assert!(
            js.contains("localStorage.setItem(TREE_WIDTH_KEY, String(treePreferredWidth))"),
            "localStorage 只存硬边界截断的偏好"
        );
        // 无效保存值 → 默认 238: 仅有限正数合法 (Infinity 同样非法), 再按硬边界截断
        assert!(
            js.contains("Number.isFinite(saved) && saved > 0 ? clampTreePreferred(saved) : TREE_DEFAULT_WIDTH"),
            "无效保存值 (Infinity/NaN/0/负数) 必须回退默认 238"
        );
        // 跨回宽屏重放: setTreeWidth(treePreferredWidth, false) 必须位于
        // installDrawerMql 与 uninstallDrawerMql 之间的 drawer change handler
        let install_at = js
            .find("function installDrawerMql()")
            .expect("installDrawerMql 存在");
        let uninstall_at = js
            .find("function uninstallDrawerMql()")
            .expect("uninstallDrawerMql 存在");
        let replay_at = js
            .find("setTreeWidth(treePreferredWidth, false)")
            .expect("宽屏重放调用存在");
        assert!(
            install_at < replay_at && replay_at < uninstall_at,
            "偏好重放必须位于 drawer mql handler 内"
        );
        // 窄屏进入不改写偏好: 进入时仅在非窄屏 (mql 不匹配) 下应用偏好
        assert!(
            js.contains("matchMedia(TREE_DRAWER_MQL).matches"),
            "窄屏进入不得应用/改写宽度偏好"
        );
        // 拖拽/键盘持久化走同一 persist 通道 (硬边界), 而不是把生效宽原样存回
        assert!(
            js.contains("setTreeWidth(dragWidth, true)"),
            "拖拽结束时持久化未截断的目标宽"
        );
    }

    /// 任务 3 收尾: 宽屏视口 resize 重钳制生效宽. 宽屏下视口收窄 (仍 ≥64rem)
    /// 时 50vw 上限下降, 生效宽必须跟随重钳制; 持久化偏好原样保留, resize
    /// 绝不写 localStorage. 命名处理器 (install/uninstall seam) 在进入树模式、
    /// 宽度状态就绪后安装一次, 退出树模式卸载 (行为矩阵见 Chromium 验收).
    #[test]
    fn browser_js_tree_viewport_resize_reclamp_seam_is_locked() {
        let js = BROWSER_JS;
        // 命名处理器状态 + install/uninstall seam: add/remove 必须走同一引用
        for needle in [
            "let treeViewportResizeHandler = null;",
            "function installTreeViewportResizeHandler()",
            "function uninstallTreeViewportResizeHandler()",
            "window.addEventListener('resize', treeViewportResizeHandler)",
            "window.removeEventListener('resize', treeViewportResizeHandler)",
        ] {
            assert!(
                js.contains(needle),
                "missing viewport-resize seam: {needle}"
            );
        }
        // handler 体: 窄屏抽屉直接返回 (resizer 隐藏, 宽度不生效); 宽屏按偏好
        // 重钳制, persist=false (绝不写 localStorage). 顺序: 守卫 → 钳制 → 挂载.
        let inst_start = js
            .find("function installTreeViewportResizeHandler()")
            .expect("installTreeViewportResizeHandler 存在");
        let inst_end = js
            .find("function uninstallTreeViewportResizeHandler()")
            .expect("uninstallTreeViewportResizeHandler 位于 install 之后");
        let inst = &js[inst_start..inst_end];
        assert!(
            inst.contains("setTreeWidth(treePreferredWidth, false)"),
            "resize 必须按偏好重钳制且不持久化"
        );
        assert!(
            !inst.contains("localStorage"),
            "resize 处理器不得写入 localStorage"
        );
        let guard_at = inst
            .find("matchMedia(TREE_DRAWER_MQL).matches")
            .expect("resize 窄屏守卫存在");
        let clamp_at = inst
            .find("setTreeWidth(treePreferredWidth, false)")
            .expect("resize 重钳制调用存在");
        let attach_at = inst
            .find("addEventListener('resize', treeViewportResizeHandler)")
            .expect("resize 监听挂载存在");
        assert!(
            guard_at < clamp_at && clamp_at < attach_at,
            "install: 先判窄屏, 再定义重钳制, 最后挂载监听"
        );
        // uninstall: 处理器存在才移除, 随后清空引用 (幂等, 不残留)
        let uninst_start = inst_end;
        let uninst_end = js
            .find("function enterTreeMode()")
            .expect("enterTreeMode 位于 uninstall 之后");
        let uninst = &js[uninst_start..uninst_end];
        let rm_at = uninst
            .find("removeEventListener('resize', treeViewportResizeHandler)")
            .expect("uninstall 移除 resize 监听");
        assert!(
            uninst[rm_at..].contains("treeViewportResizeHandler = null;"),
            "uninstall 移除监听后必须清空处理器引用"
        );
        // install 调用点: enterTreeMode 内宽度状态就绪后安装一次
        let enter_start = js
            .find("function enterTreeMode()")
            .expect("enterTreeMode 存在");
        let enter_end = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 位于 enterTreeMode 之后");
        let enter = &js[enter_start..enter_end];
        let load_at = enter
            .find("treePreferredWidth = loadTreePreferredWidth()")
            .expect("enterTreeMode 恢复偏好");
        let inst_call = enter
            .find("installTreeViewportResizeHandler()")
            .expect("enterTreeMode 安装 resize 处理器");
        assert!(load_at < inst_call, "宽度状态就绪后才能安装 resize 处理器");
        assert_eq!(
            enter.matches("installTreeViewportResizeHandler()").count(),
            1,
            "resize 处理器只安装一次"
        );
        // uninstall 调用点: exitTreeMode 内重载前卸载
        let exit_start = enter_end;
        let exit_end = js
            .find("function applyTreeMode()")
            .expect("applyTreeMode 位于 exitTreeMode 之后");
        let exit = &js[exit_start..exit_end];
        let uninst_call = exit
            .find("uninstallTreeViewportResizeHandler();")
            .expect("exitTreeMode 卸载 resize 处理器");
        let reload_at = exit
            .find("location.reload();")
            .expect("exitTreeMode 整页重载");
        assert!(
            uninst_call < reload_at,
            "退出树模式必须先卸载 resize 处理器再重载"
        );
        assert_eq!(
            exit.matches("uninstallTreeViewportResizeHandler();")
                .count(),
            1,
            "resize 处理器只卸载一次"
        );
    }
    /// 任务 4: 手动折叠 (宽屏左树收起) seam. 行为导向源不变量:
    /// - 键/状态/函数 seam 存在: TREE_COLLAPSED_KEY, treeCollapsePreferred,
    ///   applyTreeCollapsed, installTreeCollapseMql, cb-tree-collapsed, cb-tree-restore;
    /// - applyTreeCollapsed 顺序契约: 生效态计算 → 焦点捕获 (document.activeElement)
    ///   → 树内/调整器内包含判定 (折叠按钮是调整器子控件, 两处都要覆盖) → 恢复按钮
    ///   解除 hidden → 移交焦点 → 折叠类/inert/aria-hidden/resizer 隐藏. 焦点必须
    ///   在任何 class/inert/hidden 变更之前移交 (inert/隐藏元素不可聚焦), 恢复按钮
    ///   必须先解除 hidden 才能聚焦 (display:none 不可聚焦), 展开路径先隐藏恢复
    ///   按钮 (任何已绘制帧都不暴露恢复控件);
    /// - 持久化只写两态 '1'/'0'; 恢复按钮点击只写偏好 false;
    /// - mql install/uninstall 同一 handler 引用 add/remove (含 legacy 回退),
    ///   uninstall 清空引用;
    /// - 折叠按钮 (resizer 子控件): 点击 stopPropagation → 偏好 true, 绝不触碰
    ///   宽度/宽度存储; resizer keydown 同样跳过该子控件;
    /// - enter: 读偏好 (=== '1') → install → apply; exit: uninstall + 清类 +
    ///   移除按钮, 绝不写 localStorage (偏好保留).
    #[test]
    fn browser_js_tree_collapse_seams_are_locked() {
        let js = BROWSER_JS;
        for needle in [
            "const TREE_COLLAPSED_KEY = 'cb-tree-collapsed'",
            "let treeCollapsePreferred = false",
            "function applyTreeCollapsed(",
            "function installTreeCollapseMql(",
            "cb-tree-collapsed",
            "cb-tree-restore",
        ] {
            assert!(js.contains(needle), "missing tree-collapse seam: {needle}");
        }
        // applyTreeCollapsed 切片 (止于 setTreeCollapsePreference): 生效态计算、
        // 焦点捕获/移交 (树内 + 调整器内) 与 class/inert/hidden 同步的顺序契约.
        let apply_start = js
            .find("function applyTreeCollapsed(")
            .expect("applyTreeCollapsed 存在");
        let apply_end = js
            .find("function setTreeCollapsePreference(")
            .expect("setTreeCollapsePreference 位于 applyTreeCollapsed 之后");
        let apply = &js[apply_start..apply_end];
        let state_at = apply
            .find("treeCollapsed = treeCollapsePreferred && !narrow")
            .expect("applyTreeCollapsed 先计算生效态");
        let capture_at = apply
            .find("const active = document.activeElement")
            .expect("applyTreeCollapsed 必须先捕获活动焦点");
        let nav_at = apply
            .find("treeNav.contains(active)")
            .expect("焦点移交判定必须覆盖树内");
        let resizer_at = apply
            .find("treeResizer.contains(active)")
            .expect("焦点移交判定必须覆盖调整器内 (折叠按钮子控件)");
        let show_at = apply
            .find("treeRestoreBtn.hidden = !treeCollapsed")
            .expect("恢复按钮 hidden 同步存在");
        let focus_at = apply
            .find("treeRestoreBtn.focus({ preventScroll: true })")
            .expect("焦点移交到恢复按钮存在");
        let class_at = apply
            .find("classList.toggle('cb-tree-collapsed', treeCollapsed)")
            .expect("折叠类同步存在");
        let inert_at = apply
            .find("treeNav.inert = treeCollapsed")
            .expect("树 inert 同步存在");
        let aria_at = apply
            .find("setAttribute('aria-hidden', treeCollapsed ? 'true' : 'false')")
            .expect("aria-hidden 同步存在");
        let resizer_hidden_at = apply
            .find("treeResizer.hidden = treeCollapsed")
            .expect("调整器 hidden 同步存在");
        assert!(
            state_at < capture_at && capture_at < nav_at && nav_at < resizer_at,
            "生效态计算 → 焦点捕获 → 树/调整器包含判定必须依次在前 (先于任何 class/inert/hidden 变更)"
        );
        assert!(
            resizer_at < show_at && show_at < focus_at,
            "恢复按钮必须先解除 hidden, 再移交焦点 (display:none 不可聚焦)"
        );
        assert!(
            focus_at < class_at
                && class_at < inert_at
                && inert_at < aria_at
                && aria_at < resizer_hidden_at,
            "焦点移交必须先于折叠类/inert/aria-hidden/resizer 隐藏"
        );
        assert!(
            show_at < resizer_hidden_at,
            "恢复按钮 hidden 同步必须先于调整器隐藏 (展开路径先隐藏恢复控件)"
        );
        // setTreeCollapsePreference: persist 只写两态 '1'/'0', 随后立即重算生效态.
        let pref_start = apply_end;
        let pref_end = js
            .find("function installTreeCollapseMql(")
            .expect("installTreeCollapseMql 位于 setTreeCollapsePreference 之后");
        let pref = &js[pref_start..pref_end];
        assert!(
            pref.contains("localStorage.setItem(TREE_COLLAPSED_KEY, preferred ? '1' : '0')"),
            "持久化只写 '1'/'0' 两态"
        );
        assert!(
            pref.contains("applyTreeCollapsed();"),
            "设置偏好后必须立即重算生效态"
        );
        // mql install/uninstall: 同一 handler 引用 add/remove, 含 legacy 回退,
        // uninstall 清空引用 (幂等, 不残留).
        let inst_start = pref_end;
        let inst_end = js
            .find("function uninstallTreeCollapseMql()")
            .expect("uninstallTreeCollapseMql 存在");
        let inst = &js[inst_start..inst_end];
        for needle in [
            "treeCollapseMql = window.matchMedia(TREE_DRAWER_MQL)",
            "treeCollapseMqlHandler = applyTreeCollapsed",
            "addEventListener('change', treeCollapseMqlHandler)",
            "addListener(treeCollapseMqlHandler)",
        ] {
            assert!(
                inst.contains(needle),
                "installTreeCollapseMql 必须包含 {needle}"
            );
        }
        let uninst = &js[inst_end..];
        assert!(
            uninst.contains("removeEventListener('change', treeCollapseMqlHandler)"),
            "uninstall 必须移除同一 change 监听"
        );
        assert!(
            uninst.contains("removeListener(treeCollapseMqlHandler)"),
            "uninstall 必须移除 legacy 监听"
        );
        assert!(
            uninst.contains("treeCollapseMql = null;"),
            "uninstall 必须清空 mql 引用"
        );
        assert!(
            uninst.contains("treeCollapseMqlHandler = null;"),
            "uninstall 必须清空 handler 引用"
        );
        // 折叠按钮 (resizer 子控件) click: 先阻断冒泡再写偏好 true, 绝不触碰
        // 宽度或其存储键; resizer keydown 对同一子控件同样放行 (不步进).
        let click_start = js
            .find("treeCollapseBtn.addEventListener('click',")
            .expect("折叠按钮 click 处理器存在");
        let click_end = js
            .find("treeResizer.append(treeCollapseBtn)")
            .expect("折叠按钮 append 存在");
        let click = &js[click_start..click_end];
        assert!(
            click.contains("event.stopPropagation();"),
            "折叠点击必须先阻断冒泡"
        );
        assert!(
            click.contains("setTreeCollapsePreference(true, true)"),
            "折叠点击写入偏好 true 并持久化"
        );
        assert!(!click.contains("setTreeWidth"), "折叠点击绝不触碰宽度");
        assert!(!click.contains("TREE_WIDTH_KEY"), "折叠点击绝不写宽度存储");
        let kd_start = js
            .find("treeResizer.addEventListener('keydown',")
            .expect("resizer keydown 处理器存在");
        let kd_end = js
            .find("document.body.append(treeResizer)")
            .expect("resizer append 存在");
        let kd = &js[kd_start..kd_end];
        assert!(
            kd.contains("event.target.closest('.cb-tree-collapse')"),
            "resizer keydown 必须跳过折叠按钮子控件"
        );
        // 恢复按钮: 点击写偏好 false.
        let restore_click = js
            .find("treeRestoreBtn.addEventListener('click',")
            .expect("恢复按钮 click 处理器存在");
        assert!(
            js[restore_click..].contains("setTreeCollapsePreference(false, true)"),
            "恢复点击写入偏好 false 并持久化"
        );
        // enter: 读偏好 (=== '1') → install → apply; exit: uninstall + 清类 +
        // 移除恢复按钮, 绝不触碰 localStorage (偏好原样保留).
        let enter_start = js
            .find("function enterTreeMode()")
            .expect("enterTreeMode 存在");
        let enter_end = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 位于 enterTreeMode 之后");
        let enter = &js[enter_start..enter_end];
        let load_at = enter
            .find("localStorage.getItem(TREE_COLLAPSED_KEY) === '1'")
            .expect("enter 恢复折叠偏好");
        let inst_call = enter
            .find("installTreeCollapseMql();")
            .expect("enter 安装折叠 mql");
        let apply_call = enter
            .find("applyTreeCollapsed();")
            .expect("enter 应用折叠态");
        assert!(
            load_at < inst_call && inst_call < apply_call,
            "enter: 先读偏好, 再装 mql, 最后应用生效态"
        );
        let exit_start = enter_end;
        let exit_end = js
            .find("function applyTreeMode()")
            .expect("applyTreeMode 位于 exitTreeMode 之后");
        let exit = &js[exit_start..exit_end];
        assert!(
            exit.contains("uninstallTreeCollapseMql();"),
            "exit 卸载折叠 mql"
        );
        assert!(
            exit.contains("document.body.classList.remove('cb-tree-collapsed')"),
            "exit 清除生效折叠类"
        );
        assert!(
            exit.contains("treeRestoreBtn.remove()"),
            "exit 移除恢复按钮"
        );
        assert!(
            !exit.contains("localStorage"),
            "exit 绝不写折叠键 (偏好保留)"
        );
    }
}
