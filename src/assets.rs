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
    ///   模块级 treeCollapseBtn 引用 (建壳赋值/退出清空), 纯函数
    ///   collapseFocusTarget, applyTreeCollapsed, installTreeCollapseMql,
    ///   cb-tree-collapsed, cb-tree-restore; 导出含 collapseFocusTarget;
    ///   跨断点时浏览器在 mql 处理器运行前就按新视口 CSS 隐藏目标控件并静默
    ///   丢弃焦点 (activeElement 回落 body, 无 focusout 事件), 由 focusin 追踪
    ///   记录 (treeFocusKind, 落到非树壳控件即清空) 补判;
    ///   折叠路径: 恢复按钮解除 hidden → 折叠类 → 移交焦点 → 树 inert/aria →
    ///   调整器隐藏 (恢复按钮必须先解除 hidden 才能聚焦, display:none 不可聚焦);
    ///   折叠路径: 恢复按钮解除 hidden → 折叠类 → 移交焦点 → 树 inert/aria →
    ///   调整器隐藏 (恢复按钮必须先解除 hidden 才能聚焦, display:none 不可聚焦);
    ///   展开路径: 移除折叠类 → 树/调整器解除 inert/hidden → 落点聚焦 (窄屏
    ///   抽屉唤起按钮; 宽屏折叠按钮, 回退调整器/树壳) → 恢复按钮隐藏 (任何已
    ///   绘制帧都不暴露恢复控件), 焦点绝不停留在隐藏/inert 元素;
    /// - 持久化只写两态 '1'/'0'; 恢复按钮点击只写偏好 false;
    /// - mql install/uninstall 同一 handler 引用 add/remove (含 legacy 回退),
    ///   uninstall 清空引用;
    /// - 折叠按钮 (resizer 子控件): 模块引用先赋值再挂 click, click 顺序
    ///   stopPropagation → 偏好 true, 绝不触碰宽度/宽度存储; resizer keydown
    ///   同样跳过该子控件;
    /// - enter: 读偏好 (=== '1') → install → apply; exit: uninstall + 清类 +
    ///   清空折叠按钮引用 + 移除按钮, 绝不写 localStorage (偏好保留).
    #[test]
    fn browser_js_tree_collapse_seams_are_locked() {
        let js = BROWSER_JS;
        for needle in [
            "const TREE_COLLAPSED_KEY = 'cb-tree-collapsed'",
            "let treeCollapsePreferred = false",
            "let treeFocusKind = null",
            "function classifyTreeFocusTarget(",
            "function onTreeFocusIn(",
            "function onTreePointerDown(",
            "cb-tree-collapsed",
            "cb-tree-restore",
            "collapseFocusTarget: collapseFocusTarget",
        ] {
            assert!(js.contains(needle), "missing tree-collapse seam: {needle}");
        }
        // applyTreeCollapsed 切片 (止于 setTreeCollapsePreference): 生效态计算、
        // 焦点捕获、四类焦点分类与纯函数落点计算必须先于任何 class/inert/hidden
        // 变更; 折叠/展开两条路径的内部顺序契约见下.
        let apply_start = js
            .find("function applyTreeCollapsed(")
            .expect("applyTreeCollapsed 存在");
        let apply_end = js
            .find("function setTreeCollapsePreference(")
            .expect("setTreeCollapsePreference 位于 applyTreeCollapsed 之后");
        let apply = &js[apply_start..apply_end];
        let state_at = apply
            .find("const nextCollapsed = treeCollapsePreferred && !narrow")
            .expect("applyTreeCollapsed 先计算生效态");
        let capture_at = apply
            .find("const active = document.activeElement")
            .expect("applyTreeCollapsed 必须先捕获活动焦点");
        let classify_at = apply
            .find("classifyTreeFocusTarget(active)")
            .expect("applyTreeCollapsed 必须经统一分类函数判定焦点");
        let tracker_at = apply
            .find("if (!activeKind && treeFocusKind)")
            .expect("焦点被 CSS 静默丢弃时用追踪记录补判 (跨断点)");
        let helper_at = apply
            .find("collapseFocusTarget(nextCollapsed, narrow, activeKind)")
            .expect("焦点落点由纯函数计算");
        let unhide_at = apply
            .find("treeRestoreBtn.hidden = false")
            .expect("折叠路径先解除恢复按钮 hidden");
        assert!(
            state_at < capture_at
                && capture_at < classify_at
                && classify_at < tracker_at
                && tracker_at < helper_at
                && helper_at < unhide_at,
            "生效态 → 焦点捕获 → 四类分类 → 追踪补判 → 落点计算必须全部先于任何 class/inert/hidden 变更"
        );
        // 统一分类函数: 四类判定 (树内/调整器内/恢复按钮/抽屉按钮) 按序内联,
        // 覆盖折叠按钮 (调整器子控件) 与两个 identity 按钮.
        let classify_start = js
            .find("function classifyTreeFocusTarget(")
            .expect("classifyTreeFocusTarget 存在");
        let classify_end = js
            .find("function onTreeFocusIn(")
            .expect("onTreeFocusIn 位于 classifyTreeFocusTarget 之后");
        let classify = &js[classify_start..classify_end];
        let cls_nav_at = classify
            .find("treeNav.contains(target)")
            .expect("分类覆盖树内");
        let cls_resizer_at = classify
            .find("treeResizer.contains(target)")
            .expect("分类覆盖调整器内 (折叠按钮子控件)");
        let cls_restore_at = classify
            .find("target === treeRestoreBtn")
            .expect("分类覆盖恢复按钮自身");
        let cls_drawer_at = classify
            .find("target === treeOpenBtn")
            .expect("分类覆盖抽屉唤起按钮自身");
        assert!(
            cls_nav_at < cls_resizer_at
                && cls_resizer_at < cls_restore_at
                && cls_restore_at < cls_drawer_at,
            "分类顺序: 树内 → 调整器内 → 恢复按钮 → 抽屉按钮"
        );
        // 折叠路径: 解除 hidden → 折叠类 → 移交焦点 → 树 inert → aria → 调整器隐藏.
        let class_add_at = apply
            .find("classList.add('cb-tree-collapsed')")
            .expect("折叠路径添加折叠类");
        let focus_at = apply
            .find("treeRestoreBtn.focus({ preventScroll: true })")
            .expect("折叠路径移交焦点到恢复按钮");
        let kind_restore_at = apply
            .find("treeFocusKind = 'restore'")
            .expect("折叠路径记录焦点种类 (程序化移交不触发 focusin 的环境兜底)");
        let inert_at = apply
            .find("treeNav.inert = true")
            .expect("折叠路径树 inert 同步存在");
        let aria_at = apply
            .find("treeNav.setAttribute('aria-hidden', 'true')")
            .expect("折叠路径 aria-hidden 同步存在");
        let resizer_hidden_at = apply
            .find("treeResizer.hidden = true")
            .expect("折叠路径调整器隐藏存在");
        assert!(
            unhide_at < class_add_at
                && class_add_at < focus_at
                && focus_at < kind_restore_at
                && kind_restore_at < inert_at
                && inert_at < aria_at
                && aria_at < resizer_hidden_at,
            "折叠路径: 解除 hidden → 折叠类 → 移交焦点 → 记录种类 → 树 inert/aria → 调整器隐藏"
        );
        // 展开路径: 移除折叠类 → 树/调整器解除 inert/hidden → 落点聚焦 (窄屏
        // 抽屉唤起按钮; 宽屏折叠按钮, 回退调整器/树壳) → 恢复按钮隐藏.
        let class_remove_at = apply
            .find("classList.remove('cb-tree-collapsed')")
            .expect("展开路径移除折叠类");
        let inert_clear_at = apply
            .find("treeNav.inert = false")
            .expect("展开路径解除树 inert");
        let aria_clear_at = apply
            .find("treeNav.setAttribute('aria-hidden', 'false')")
            .expect("展开路径解除 aria-hidden");
        let resizer_show_at = apply
            .find("treeResizer.hidden = false")
            .expect("展开路径解除调整器隐藏");
        let drawer_focus_at = apply
            .find("treeOpenBtn.focus({ preventScroll: true })")
            .expect("窄屏展开路径焦点移交抽屉唤起按钮");
        let kind_drawer_at = apply
            .find("treeFocusKind = 'drawer'")
            .expect("窄屏展开路径记录抽屉焦点种类");
        let collapse_focus_at = apply
            .find("treeCollapseBtn.focus({ preventScroll: true })")
            .expect("宽屏展开路径焦点移交折叠按钮");
        let kind_resizer_at = apply
            .find("treeFocusKind = 'resizer'")
            .expect("宽屏展开路径记录调整器焦点种类");
        let fallback_resizer_at = apply
            .find("treeResizer.focus({ preventScroll: true })")
            .expect("折叠按钮缺失时回退调整器");
        // 任务 5: nav 不再挂 tabIndex (不可聚焦), 回退链优先树根 (tabindex=0
        // 单停靠点), nav 仅最后兜底 — 焦点绝不停留在隐藏/inert 元素.
        let fallback_root_at = apply
            .find("treeRootUl.focus({ preventScroll: true })")
            .expect("调整器也缺失时回退树根");
        let fallback_nav_at = apply
            .find("treeNav.focus({ preventScroll: true })")
            .expect("树根也缺失时最后兜底树壳");
        let restore_hidden_at = apply
            .find("treeRestoreBtn.hidden = true")
            .expect("展开路径隐藏恢复按钮");
        assert!(
            class_remove_at < inert_clear_at
                && inert_clear_at < aria_clear_at
                && aria_clear_at < resizer_show_at
                && resizer_show_at < drawer_focus_at
                && drawer_focus_at < kind_drawer_at
                && kind_drawer_at < collapse_focus_at
                && collapse_focus_at < kind_resizer_at
                && kind_resizer_at < fallback_resizer_at
                && fallback_resizer_at < fallback_root_at
                && fallback_root_at < fallback_nav_at
                && fallback_nav_at < restore_hidden_at,
            "展开路径: 移除类 → 解除 inert/hidden → 落点聚焦 → 记录种类 → 恢复按钮隐藏"
        );
        // 折叠按钮 (resizer 子控件): 建壳时先赋值模块引用再挂 click; click 先
        // 阻断冒泡再写偏好 true, 绝不触碰宽度或其存储键; resizer keydown 对
        // 同一子控件同样放行 (不步进).
        let create_at = js
            .find("treeCollapseBtn = document.createElement('button')")
            .expect("折叠按钮创建必须赋值模块引用");
        let click_start = js
            .find("treeCollapseBtn.addEventListener('click',")
            .expect("折叠按钮 click 处理器存在");
        assert!(create_at < click_start, "折叠按钮先创建再挂 click");
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
        // 清空折叠按钮引用 + 移除按钮, 绝不触碰 localStorage (偏好原样保留).
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
        // 焦点追踪随壳安装: focusin 记录 + pointerdown 清空 (内容区点击不触发
        // focusin, 必须单独清空防过期记录误判).
        assert!(
            enter.contains("document.addEventListener('focusin', onTreeFocusIn, true)"),
            "enter 安装 focusin 追踪"
        );
        assert!(
            enter.contains("document.addEventListener('pointerdown', onTreePointerDown, true)"),
            "enter 安装 pointerdown 追踪"
        );
        // 追踪 handler 本身: focusin 经统一分类记录; pointerdown 仅当按下非
        // 树壳控件时清空 (树壳控件按下不动作, 随后的 focusin 重新记录).
        let focusin_start = js
            .find("function onTreeFocusIn(")
            .expect("onTreeFocusIn 存在");
        let pointer_start = js
            .find("function onTreePointerDown(")
            .expect("onTreePointerDown 存在");
        assert!(
            focusin_start < pointer_start,
            "onTreeFocusIn 必须位于 onTreePointerDown 之前"
        );
        let focusin_body = &js[focusin_start..pointer_start];
        assert!(
            focusin_body.contains("treeFocusKind = classifyTreeFocusTarget(event.target)"),
            "focusin 记录最近一次树壳焦点"
        );
        let pointer_end = js
            .find("function applyTreeCollapsed(")
            .expect("applyTreeCollapsed 位于 onTreePointerDown 之后");
        let pointer_body = &js[pointer_start..pointer_end];
        assert!(
            pointer_body.contains("treeFocusKind = null"),
            "pointerdown 落在非树壳控件时清空追踪记录"
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
            exit.contains("document.removeEventListener('focusin', onTreeFocusIn, true)"),
            "exit 卸载 focusin 追踪"
        );
        assert!(
            exit.contains("document.removeEventListener('pointerdown', onTreePointerDown, true)"),
            "exit 卸载 pointerdown 追踪"
        );
        assert!(
            exit.contains("treeFocusKind = null"),
            "exit 清空焦点追踪记录"
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
            exit.contains("treeCollapseBtn = null"),
            "exit 清空折叠按钮引用 (随壳销毁)"
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

    /// 任务 4 质量收尾: 折叠/展开焦点落点纯函数在 QuickJS 中真实执行. 行为
    /// 矩阵覆盖全部过渡与 no-op:
    /// - 折叠生效 (nextCollapsed): 焦点在树内/调整器内/抽屉唤起按钮 → 'restore'
    ///   (inert/隐藏元素不可聚焦, 必须提前移交); 焦点在恢复按钮自身或受影响
    ///   控件之外 → null (恢复按钮保持可见, 无需移交);
    /// - 展开生效 (!nextCollapsed): 焦点在恢复按钮 → 窄屏 'drawer' (恢复按钮
    ///   即将隐藏, 抽屉唤起按钮可见) / 宽屏 'collapse' (折叠按钮是调整器子控件,
    ///   树列重新可见); 宽屏下抽屉唤起按钮焦点 → 'collapse'; 焦点在树内/调整器
    ///   内/其余 → null (元素保持可见/可聚焦, 不移交).
    #[test]
    fn browser_js_tree_collapse_focus_target_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        // CommonJS 钩子 shim: 与 Node 的 module.exports 语义一致 (浏览器无 module).
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        // 期望 'restore': 折叠生效时树内/调整器内/抽屉唤起按钮焦点一律移交恢复按钮.
        for expr in [
            "collapseFocusTarget(true, false, 'tree')",
            "collapseFocusTarget(true, false, 'resizer')",
            "collapseFocusTarget(true, false, 'drawer')",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("collapseFocusTarget must not throw");
            assert_eq!(
                got.as_str(),
                Some("restore"),
                "{expr}: expected restore, got {got:?}"
            );
        }
        // 期望 'collapse': 宽屏展开 — 恢复按钮焦点落回折叠按钮; 抽屉唤起按钮
        // 焦点落回折叠按钮 (树列重新可见).
        for expr in [
            "collapseFocusTarget(false, false, 'restore')",
            "collapseFocusTarget(false, false, 'drawer')",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("collapseFocusTarget must not throw");
            assert_eq!(
                got.as_str(),
                Some("collapse"),
                "{expr}: expected collapse, got {got:?}"
            );
        }
        // 期望 'drawer': 收窄到抽屉且焦点在恢复按钮 → 移交可见的抽屉唤起按钮.
        let expr = "collapseFocusTarget(false, true, 'restore')";
        let got = ctx
            .eval(&format!("module.exports.{expr}"))
            .expect("collapseFocusTarget must not throw");
        assert_eq!(
            got.as_str(),
            Some("drawer"),
            "{expr}: expected drawer, got {got:?}"
        );
        // 期望 null (不移交): 焦点元素保持可见/可聚焦, 或不在受影响控件内.
        for expr in [
            "collapseFocusTarget(false, false, 'tree')",
            "collapseFocusTarget(false, false, 'resizer')",
            "collapseFocusTarget(false, false, null)",
            "collapseFocusTarget(false, true, 'tree')",
            "collapseFocusTarget(false, true, 'resizer')",
            "collapseFocusTarget(false, true, 'drawer')",
            "collapseFocusTarget(false, true, null)",
            "collapseFocusTarget(true, false, 'restore')",
            "collapseFocusTarget(true, false, null)",
            "collapseFocusTarget(true, true, 'restore')",
            "collapseFocusTarget(true, true, null)",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("collapseFocusTarget must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Null),
                "{expr}: expected null, got {got:?}"
            );
        }
    }

    /// 任务 5: 树单击打开 (文件导航/目录展开) + 键盘可聚焦 ARIA 树. 行为导向
    /// 源不变量:
    /// - 单击树行: 双击连击守卫 (event.detail > 1) → li 定位守卫 → 选中 →
    ///   目录展开 / 文件 navigateTreeNode → 收抽屉; dblclick 树行分支开头即
    ///   阻断默认并返回 (导航完全移交单击, 双击不得产生第二次动作);
    /// - ARIA 树 (搜索表单在 nav 内, role/tabindex 必须挂根 ul 而非 nav):
    ///   根 ul role=tree + tabindex=0; nav 保留导航地标 aria-label 且不再挂
    ///   tabIndex; 节点 li role=treeitem + 稳定 id (模块计数器, 每次建壳复位)
    ///   与 aria-selected; 目录节点初始 aria-expanded=false, 展开态同步; 子 ul
    ///   role=group, 根 aria-activedescendant 随选择更新/清除;
    /// - openTreeDrawer 聚焦根 ul (nav 仅兜底) — 活动后代经 tabindex=0 根公告;
    /// - PageUp/PageDown 滚动树列 80% 视口高并阻断默认 (Arrow 行为不变).
    #[test]
    fn browser_js_tree_click_and_aria_contract_is_locked() {
        let js = BROWSER_JS;
        // 单击处理: 树行分支内顺序 — detail 守卫 → li 定位 + 守卫 → 选中 →
        // 目录展开 / 文件导航 → 收抽屉.
        let click_start = js
            .find("function onDocumentClick(")
            .expect("onDocumentClick 存在");
        let tree_row_at = js
            .find("event.target.closest('.cb-tree-row')")
            .expect("树行分支存在");
        let dbl_at = js
            .find("if (event.detail > 1) return;")
            .expect("单击先拦双击连击 (detail > 1)");
        let li_at = js
            .find("const li = treeRow.closest('li.cb-tree-node');")
            .expect("行定位 li");
        let li_guard_at = js.find("if (!li) return;").expect("li 定位守卫");
        let focus_at = js
            .find("focusTreeRootUl();")
            .expect("单击先聚焦树根 (active-descendant 公告 + 键盘接管)");
        let sel_at = js.find("selectTreeNode(li);").expect("单击选中");
        let dir_at = js
            .find("if (isDirNode(li)) toggleTreeNode(li); else navigateTreeNode(li);")
            .expect("目录展开 / 文件导航分支");
        let close_at = js
            .find("closeTreeDrawer(); return;")
            .expect("树行处理后收抽屉");
        assert!(
            click_start < tree_row_at
                && tree_row_at < dbl_at
                && dbl_at < li_at
                && li_at < li_guard_at
                && li_guard_at < focus_at
                && focus_at < sel_at
                && sel_at < dir_at
                && dir_at < close_at,
            "单击顺序: 树行分支 → detail 守卫 → li 定位 → li 守卫 → 聚焦树根 → 选中 → 展开/导航 → 收抽屉"
        );
        // 双击处理: 树行分支开头即阻断默认并返回 (不再整页/SPA 导航).
        let dblclick_start = js
            .find("function onDocumentDblclick(")
            .expect("onDocumentDblclick 存在");
        let dblclick_end = js
            .find("function isEditableTarget(")
            .expect("isEditableTarget 位于 onDocumentDblclick 之后");
        let dblclick_body = &js[dblclick_start..dblclick_end];
        let dbl_guard_at = dblclick_body
            .find("if (event.target.closest('.cb-tree-row')) {")
            .expect("dblclick 树行守卫存在");
        let dbl_entry_at = dblclick_body
            .find("const entry = event.target.closest(ENTRY_SELECTOR);")
            .expect("ENTRY 分支位于树行守卫之后");
        assert!(
            dbl_guard_at < dbl_entry_at,
            "dblclick 树行守卫必须位于树分支开头 (先于 ENTRY 分支)"
        );
        let dbl_guard = &dblclick_body[dbl_guard_at..dbl_entry_at];
        assert!(
            dbl_guard.contains("event.preventDefault();") && dbl_guard.contains("return;"),
            "dblclick 树行: 阻断默认并返回, 不得再导航"
        );
        // ARIA 树根: 建壳时根 ul role=tree + aria-label + tabindex=0; nav 保留
        // 地标且无 tabIndex; 树壳与树根都有稳定 id (抽屉按钮 aria-controls 锚点).
        let shell_at = js
            .find("function buildTreeShell(")
            .expect("buildTreeShell 存在");
        let role_at = js
            .find("treeRootUl.setAttribute('role', 'tree')")
            .expect("根 ul role=tree");
        let label_at = js
            .find("treeRootUl.setAttribute('aria-label', '目录树')")
            .expect("根 ul aria-label 存在");
        let tab_at = js
            .find("treeRootUl.tabIndex = 0")
            .expect("根 ul tabindex=0");
        assert!(
            shell_at < role_at && role_at < label_at && label_at < tab_at,
            "建壳时先设 role=tree → aria-label → tabindex=0"
        );
        assert!(
            js.contains("treeNav.id = 'cb-tree-nav'"),
            "树壳必须携带稳定 id (aria-controls 锚点)"
        );
        assert!(
            js.contains("treeRootUl.id = 'cb-tree-root'"),
            "树根必须携带稳定 id"
        );
        assert!(
            js.contains("treeNav.setAttribute('aria-label', '目录树')"),
            "nav 保留导航地标 aria-label"
        );
        assert!(
            !js.contains("treeNav.tabIndex"),
            "nav 不得再挂 tabIndex (焦点与 role 在根 ul)"
        );
        // 稳定节点 id: 模块计数器声明早于建壳, 每次建壳复位; buildTreeNode 赋
        // id/role/aria-selected, 目录节点初始 aria-expanded=false.
        let counter_at = js.find("let treeItemSeq = 0;").expect("模块计数器声明存在");
        let reset_at = js
            .find("treeItemSeq = 0; // 新壳")
            .expect("建壳复位计数器存在");
        let node_at = js
            .find("function buildTreeNode(")
            .expect("buildTreeNode 存在");
        assert!(
            counter_at < node_at && node_at < shell_at && shell_at < reset_at,
            "计数器声明 → 节点构建 → 建壳 → 复位 (id 从 1 起, 每壳唯一)"
        );
        let build_end = js
            .find("function childrenUlFor(")
            .expect("childrenUlFor 位于 buildTreeNode 之后");
        let build = &js[node_at..build_end];
        for needle in [
            "li.id = 'cb-tree-item-' + (++treeItemSeq)",
            "li.setAttribute('role', 'treeitem')",
            "li.setAttribute('aria-selected', 'false')",
        ] {
            assert!(build.contains(needle), "buildTreeNode 缺少 seam: {needle}");
        }
        // aria-expanded 只挂可展开目录 (有子项可展开): 终端目录 (不可展开) 不得
        // 携带 aria-expanded — 不向 AT 伪装可展开性.
        assert!(
            build.contains("if (expandable) li.setAttribute('aria-expanded', 'false')"),
            "buildTreeNode: aria-expanded 必须仅限可展开目录"
        );
        // 装饰字形 (twist/icon/badge) 必须 aria-hidden — 可访问名只来自文件名
        // bdi; bdi 不得隐藏.
        for needle in [
            "twist.setAttribute('aria-hidden', 'true')",
            "icon.setAttribute('aria-hidden', 'true')",
            "badge.setAttribute('aria-hidden', 'true')",
        ] {
            assert!(
                build.contains(needle),
                "buildTreeNode 缺少 aria-hidden: {needle}"
            );
        }
        assert!(
            !build.contains("bdi.setAttribute('aria-hidden'"),
            "文件名 bdi 不得隐藏 (可访问名)"
        );
        assert!(
            !build.contains("if (isDir) li.setAttribute('aria-expanded'"),
            "终端目录不得携带 aria-expanded"
        );
        // 子 ul role=group; 展开态同步 aria-expanded.
        let child_at = js
            .find("function childrenUlFor(")
            .expect("childrenUlFor 存在");
        let child_end = js
            .find("function isDirNode(")
            .expect("isDirNode 位于 childrenUlFor 之后");
        let child = &js[child_at..child_end];
        assert!(
            child.contains("ul.setAttribute('role', 'group')"),
            "子 ul role=group"
        );
        let expand_at = js
            .find("function setTreeNodeExpanded(")
            .expect("setTreeNodeExpanded 存在");
        let expand_end = js
            .find("function fetchTreeEntries(")
            .expect("fetchTreeEntries 位于 setTreeNodeExpanded 之后");
        let expand = &js[expand_at..expand_end];
        assert!(
            expand.contains("if (isExpandableDir(li)) li.setAttribute('aria-expanded', expanded ? 'true' : 'false')"),
            "setTreeNodeExpanded 只同步可展开目录的 aria-expanded"
        );
        // 选中: 旧节点 aria-selected=false, 新节点 true, 根 aria-activedescendant
        // 指向新 id; 清除: 旧节点置 false, 根移除 aria-activedescendant.
        let sel_fn_at = js
            .find("function selectTreeNode(")
            .expect("selectTreeNode 存在");
        let clear_fn_at = js
            .find("function clearTreeSelection(")
            .expect("clearTreeSelection 存在");
        let sel_fn = &js[sel_fn_at..clear_fn_at];
        for needle in [
            "treeSelectedEl.setAttribute('aria-selected', 'false')",
            "li.setAttribute('aria-selected', 'true')",
            "treeRootUl.setAttribute('aria-activedescendant', li.id)",
        ] {
            assert!(
                sel_fn.contains(needle),
                "selectTreeNode 缺少 seam: {needle}"
            );
        }
        let clear_end = js
            .find("function visibleTreeNodes(")
            .expect("visibleTreeNodes 位于 clearTreeSelection 之后");
        let clear_fn = &js[clear_fn_at..clear_end];
        for needle in [
            "treeSelectedEl.setAttribute('aria-selected', 'false')",
            "treeRootUl.removeAttribute('aria-activedescendant')",
        ] {
            assert!(
                clear_fn.contains(needle),
                "clearTreeSelection 缺少 seam: {needle}"
            );
        }
        // 抽屉打开聚焦根 ul, nav 仅兜底 (活动后代经根公告).
        let open_at = js
            .find("function openTreeDrawer(")
            .expect("openTreeDrawer 存在");
        let close_drawer_at = js
            .find("function closeTreeDrawer(")
            .expect("closeTreeDrawer 位于 openTreeDrawer 之后");
        let open = &js[open_at..close_drawer_at];
        assert!(
            open.contains("const focusTarget = treeRootUl || treeNav;"),
            "openTreeDrawer 聚焦根 ul, nav 仅兜底"
        );
        assert!(
            open.contains("focusTarget.focus({ preventScroll: true })"),
            "openTreeDrawer 程序化聚焦 (preventScroll)"
        );
        // 行点击聚焦 helper: preventScroll 优先, 老环境退化普通 focus; 无树根
        // 时不动作 (点击聚焦只发生在树壳存在时).
        let focus_fn_at = js
            .find("function focusTreeRootUl(")
            .expect("focusTreeRootUl 存在");
        let focus_fn_end = js
            .find("function visibleTreeNodes(")
            .expect("visibleTreeNodes 位于 focusTreeRootUl 之后");
        let focus_fn = &js[focus_fn_at..focus_fn_end];
        for needle in [
            "if (!treeRootUl) return;",
            "treeRootUl.focus({ preventScroll: true })",
            "treeRootUl.focus();",
        ] {
            assert!(
                focus_fn.contains(needle),
                "focusTreeRootUl 缺少契约: {needle}"
            );
        }
        assert!(
            focus_fn.contains("catch (err)"),
            "focusTreeRootUl 必须捕获 focus options 不支持的老环境"
        );
        // 抽屉唤起按钮: 创建即带稳定 aria-controls (树壳稳定 id) 与
        // aria-expanded='false'; open/close 同步 'true'/'false'.
        let open_btn_at = js
            .find("treeOpenBtn = document.createElement('button')")
            .expect("抽屉唤起按钮创建存在");
        let open_btn_click_at = js
            .find("treeOpenBtn.addEventListener('click', openTreeDrawer)")
            .expect("唤起按钮 click 监听存在");
        assert!(open_btn_at < open_btn_click_at, "唤起按钮先创建再挂 click");
        let open_btn = &js[open_btn_at..open_btn_click_at];
        assert!(
            open_btn.contains("treeOpenBtn.setAttribute('aria-controls', 'cb-tree-nav')"),
            "唤起按钮必须带 aria-controls (树壳稳定 id)"
        );
        assert!(
            open_btn.contains("treeOpenBtn.setAttribute('aria-expanded', 'false')"),
            "唤起按钮创建时必须显式 aria-expanded='false'"
        );
        assert!(
            open.contains("treeOpenBtn.setAttribute('aria-expanded', 'true')"),
            "openTreeDrawer 必须同步 aria-expanded='true'"
        );
        let close_fn_end = js
            .find("function installDrawerMql(")
            .expect("installDrawerMql 位于 closeTreeDrawer 之后");
        let close_fn = &js[close_drawer_at..close_fn_end];
        assert!(
            close_fn.contains("treeOpenBtn.setAttribute('aria-expanded', 'false')"),
            "closeTreeDrawer 必须同步 aria-expanded='false'"
        );
        // PageUp/PageDown: 仅聚焦树根时滚动树列 80% 视口高并阻断默认 — 焦点门卫
        // (根 ul 存在且为事件目标或活动元素) 必须先于 scrollBy 与 preventDefault;
        // 焦点在 pane/链接/切换按钮/body 时不得滚动树列也不得阻断默认 (放行原生滚动).
        // PageUp 先于 PageDown; Arrow 语义不变 (Page 门卫不得用于 Arrow).
        let kd_at = js
            .find("function onTreeKeydown(")
            .expect("onTreeKeydown 存在");
        let kd_end = js
            .find("function init(")
            .expect("init 位于 onTreeKeydown 之后");
        let kd = &js[kd_at..kd_end];
        let pu_at = kd.find("case 'PageUp':").expect("PageUp 分支存在");
        let pd_at = kd.find("case 'PageDown':").expect("PageDown 分支存在");
        let arrow_at = kd.find("case 'ArrowDown':").expect("ArrowDown 分支存在");
        assert!(
            pu_at < pd_at && pd_at < arrow_at,
            "PageUp 先于 PageDown, 均先于 ArrowDown"
        );
        for needle in [
            "treeNav.scrollBy({ top: -Math.round(treeNav.clientHeight * 0.8) })",
            "treeNav.scrollBy({ top: Math.round(treeNav.clientHeight * 0.8) })",
            "event.preventDefault();",
        ] {
            assert!(kd.contains(needle), "onTreeKeydown 缺少 seam: {needle}");
        }
        // 逐分支切片: 焦点门卫 → scrollBy → preventDefault 的顺序必须保持, 且
        // 门卫同时含树根存在检查与树根焦点检查 (否则聚焦别处时不得滚动/阻断).
        // ArrowLeft 收起可见节点: 可见 ul 即已加载 (无在途) — 直接改 DOM, 不得
        // 写意图残留 (陈旧意图会挡住后续 reveal 的缓存命中展开); 收起必须经
        // setTreeNodeExpanded 同步持久化/aria.
        let al_at = kd.find("case 'ArrowLeft':").expect("ArrowLeft 分支存在");
        let enter_at = kd
            .find("case 'Enter':")
            .expect("Enter 分支位于 ArrowLeft 之后");
        let al_branch = &kd[al_at..enter_at];
        assert!(
            al_branch.contains("setTreeNodeExpanded(treeSelectedEl, false)"),
            "ArrowLeft 收起必须经 setTreeNodeExpanded"
        );
        assert!(
            !al_branch.contains("treeExpansionIntents"),
            "ArrowLeft 收起 (无在途) 不得写意图"
        );
        let focus_gate = "(event.target === treeRootUl || document.activeElement === treeRootUl)";
        for (body, label) in [
            (&kd[pu_at..pd_at], "PageUp"),
            (&kd[pd_at..arrow_at], "PageDown"),
        ] {
            let root_at = body
                .find("treeRootUl &&")
                .expect("{label} 缺少树根存在门卫 (treeRootUl &&)");
            let gate_at = body.find(focus_gate).expect("{label} 缺少焦点门卫");
            let scroll_at = body.find("treeNav.scrollBy").expect("{label} 缺少滚动调用");
            let prevent_at = body
                .find("event.preventDefault();")
                .expect("{label} 缺少默认阻断");
            assert!(
                root_at < gate_at && gate_at < scroll_at && scroll_at < prevent_at,
                "{label}: 焦点门卫必须先于 scrollBy 与 preventDefault"
            );
        }
    }

    /// 任务 5 质量收尾: 树键范围纯函数在 QuickJS 中真实执行. 行为矩阵:
    /// - Escape: 搜索框聚焦 → 'escape-search' (失焦/先收抽屉), 即使树根同聚焦
    ///   (搜索优先); 树根聚焦 → 'tree' (清选中/收抽屉); 其余 (pane/按钮/切换/
    ///   body) → null;
    /// - '/': 非可编辑即 'focus-search' (刻意全局快捷, 聚焦树搜索); 可编辑
    ///   (搜索框内输入) → null (原样输入 '/' 字符);
    /// - Arrow/Enter/Backspace/PageUp/PageDown: 仅树根聚焦 → 'tree'; pane/
    ///   按钮/body 聚焦 → null (不阻断原生滚动/激活); 可编辑目标 → null;
    /// - 未知键一律 null.
    #[test]
    fn browser_js_tree_key_scope_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        // CommonJS 钩子 shim: 与 Node 的 module.exports 语义一致 (浏览器无 module).
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        // 'escape-search': Escape 且搜索框聚焦 (可编辑与否均可, 搜索优先于树根).
        for expr in [
            "treeKeyScope('Escape', true, false, true)",
            "treeKeyScope('Escape', false, false, true)",
            "treeKeyScope('Escape', true, true, true)",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("treeKeyScope must not throw");
            assert_eq!(
                got.as_str(),
                Some("escape-search"),
                "{expr}: expected escape-search, got {got:?}"
            );
        }
        // 'tree': 树根聚焦的 Escape 与全部树导航键.
        for expr in [
            "treeKeyScope('Escape', false, true, false)",
            "treeKeyScope('ArrowDown', false, true, false)",
            "treeKeyScope('ArrowUp', false, true, false)",
            "treeKeyScope('ArrowLeft', false, true, false)",
            "treeKeyScope('ArrowRight', false, true, false)",
            "treeKeyScope('Enter', false, true, false)",
            "treeKeyScope('Backspace', false, true, false)",
            "treeKeyScope('PageUp', false, true, false)",
            "treeKeyScope('PageDown', false, true, false)",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("treeKeyScope must not throw");
            assert_eq!(
                got.as_str(),
                Some("tree"),
                "{expr}: expected tree, got {got:?}"
            );
        }
        // 'focus-search': '/' 且非可编辑 (树根聚焦 / pane / 按钮 / body).
        for expr in [
            "treeKeyScope('/', false, false, false)",
            "treeKeyScope('/', false, true, false)",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("treeKeyScope must not throw");
            assert_eq!(
                got.as_str(),
                Some("focus-search"),
                "{expr}: expected focus-search, got {got:?}"
            );
        }
        // null: pane/按钮/body 聚焦的 Escape 与导航键 (不阻断默认); 可编辑目标
        // 的导航键与 '/' (原样输入); 未知键.
        for expr in [
            "treeKeyScope('Escape', false, false, false)",
            "treeKeyScope('ArrowDown', false, false, false)",
            "treeKeyScope('ArrowUp', false, false, false)",
            "treeKeyScope('ArrowLeft', false, false, false)",
            "treeKeyScope('ArrowRight', false, false, false)",
            "treeKeyScope('Enter', false, false, false)",
            "treeKeyScope('Backspace', false, false, false)",
            "treeKeyScope('PageUp', false, false, false)",
            "treeKeyScope('PageDown', false, false, false)",
            "treeKeyScope('Escape', true, false, false)",
            "treeKeyScope('/', true, false, false)",
            "treeKeyScope('/', true, false, true)",
            "treeKeyScope('ArrowDown', true, true, false)",
            "treeKeyScope('ArrowDown', true, true, true)",
            "treeKeyScope('j', false, true, false)",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("treeKeyScope must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Null),
                "{expr}: expected null, got {got:?}"
            );
        }
    }

    /// 任务 5 收尾 (版本化意图重构): 显式用户激活意图纯函数 + 发布版本时序在
    /// QuickJS 中真实执行. 行为矩阵 (latest-wins 时序语义):
    /// - prior 为最新已发布记录且 explicit=true (显式用户意图): 翻转其 expanded
    ///   — true→false (第二次激活表达收起), false→true (重新表达展开); 可见态
    ///   expanded 参数不参与 (最新用户意图权威);
    /// - prior 为 null (无记录) 或 nav/默认记录 (explicit=false): 按可见 DOM
    ///   推导 — 可见折叠 (expanded=false) 即表达展开 (true), nav 的隐藏意图
    ///   绝不参与 (首击可见折叠目录必须展开, 无需第三次点击);
    /// - 返回记录 { expanded, explicit: true } — 显式标记恒 true;
    /// - publishTreeExpansionIntent 递增全局单调版本并返回记录: 更晚发布天然
    ///   覆盖更早 (更新的导航覆盖更早显式折叠 / 更晚显式点击覆盖导航); 版本
    ///   跨路径共享时序 (全局计数器).
    #[test]
    fn browser_js_tree_expansion_intent_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        // CommonJS 钩子 shim: 与 Node 的 module.exports 语义一致 (浏览器无 module).
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        // prior 显式: 翻转其 expanded (可见态参数不参与 — 最新用户意图权威).
        for (expr, expected) in [
            (
                "treeNextExplicitIntent({ expanded: true, explicit: true }, false)",
                false,
            ),
            (
                "treeNextExplicitIntent({ expanded: true, explicit: true }, true)",
                false,
            ),
            (
                "treeNextExplicitIntent({ expanded: false, explicit: true }, false)",
                true,
            ),
            (
                "treeNextExplicitIntent({ expanded: false, explicit: true }, true)",
                true,
            ),
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}.expanded"))
                .expect("treeNextExplicitIntent must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(b) if b == expected),
                "{expr}: expected {expected}, got {got:?}"
            );
        }
        // prior 无 / nav 默认 (explicit=false): 按可见 DOM 推导 (!expanded) —
        // nav 的隐藏意图绝不参与 (首击可见折叠目录必须展开).
        for (expr, expected) in [
            ("treeNextExplicitIntent(null, false)", true),
            ("treeNextExplicitIntent(null, true)", false),
            (
                "treeNextExplicitIntent({ expanded: true, explicit: false }, false)",
                true,
            ),
            (
                "treeNextExplicitIntent({ expanded: true, explicit: false }, true)",
                false,
            ),
            (
                "treeNextExplicitIntent({ expanded: false, explicit: false }, false)",
                true,
            ),
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}.expanded"))
                .expect("treeNextExplicitIntent must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(b) if b == expected),
                "{expr}: expected {expected}, got {got:?}"
            );
        }
        // 返回记录形态: explicit 恒 true (用户激活即显式).
        for expr in [
            "treeNextExplicitIntent(null, false)",
            "treeNextExplicitIntent({ expanded: true, explicit: false }, true)",
            "treeNextExplicitIntent({ expanded: true, explicit: true }, false)",
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}.explicit"))
                .expect("treeNextExplicitIntent must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(true)),
                "{expr}: explicit 恒 true, got {got:?}"
            );
        }
        // 发布版本单调: 每次发布递增全局版本; 更晚发布覆盖更早 (latest wins).
        let v1 = ctx
            .eval_as::<i32>("module.exports.publishTreeExpansionIntent('p', true, false).version")
            .expect("publishTreeExpansionIntent must not throw");
        let v2 = ctx
            .eval_as::<i32>("module.exports.publishTreeExpansionIntent('p', false, true).version")
            .expect("publishTreeExpansionIntent must not throw");
        let v3 = ctx
            .eval_as::<i32>("module.exports.publishTreeExpansionIntent('q', true, true).version")
            .expect("publishTreeExpansionIntent must not throw");
        assert!(
            v1 < v2 && v2 < v3,
            "发布版本必须严格单调递增 (跨路径共享全局时序): {v1} < {v2} < {v3}"
        );
        // 发布记录形态: { expanded, version, explicit } 三字段齐全.
        let rec_explicit = ctx
            .eval("module.exports.publishTreeExpansionIntent('p', false, true).explicit")
            .expect("publishTreeExpansionIntent must not throw");
        let rec_expanded = ctx
            .eval("module.exports.publishTreeExpansionIntent('p', false, true).expanded")
            .expect("publishTreeExpansionIntent must not throw");
        let rec_version = ctx
            .eval_as::<i32>("module.exports.publishTreeExpansionIntent('p', false, true).version")
            .expect("publishTreeExpansionIntent must not throw");
        assert!(
            matches!(rec_explicit, quick_js::JsValue::Bool(true))
                && matches!(rec_expanded, quick_js::JsValue::Bool(false))
                && rec_version > v3,
            "发布记录必须含 expanded/version/explicit 且版本继续单调: explicit={rec_explicit:?} expanded={rec_expanded:?} version={rec_version} (v3={v3})"
        );
    }

    /// 任务 5 质量收尾: 树键范围 helper 必须被生产 onTreeKeydown 使用.
    /// - helper 内顺序: Escape 分支先于 editable 守卫 (搜索框聚焦的 Escape
    ///   优先), editable 守卫先于 '/' 与导航键 (搜索框内 '/' 原样输入);
    /// - onTreeKeydown: 以 helper 决策为总闸 — scope === null 直接返回 (绝不
    ///   preventDefault); 'escape-search' 分支: 窄屏抽屉打开时先 closeTreeDrawer
    ///   (焦点归还唤起按钮) 再 blur, 宽屏仅 blur; 'focus-search' 聚焦树搜索框;
    ///   Escape 的 tree 范围清选中并收抽屉; switch 不再重复处理 '/' (已由
    ///   focus-search 分支接管);
    /// - helper 经 CommonJS 钩子导出 (浏览器无全局污染).
    ///
    /// 任务 5 收尾 + 异步生命周期隔离: 树加载提交决策纯函数在 QuickJS 中真实执行.
    /// 行为矩阵:
    /// - 壳不当前 (shellCurrent=false): 一律拒绝, 即使导航当前;
    /// - 壳当前 + 无导航所有权 (navRequired=false, 显式用户调用): 提交,
    ///   navCurrent 参数无关;
    /// - 壳当前 + 导航所有权 (navRequired=true, reveal/sync 链): 仅导航当前可提交;
    /// - 返回决策直接门禁缓存命中与异步响应的 DOM/展开/持久化提交; 缓存结果
    ///   共享不受此约束 (陈旧导航可复用结果, 但不得提交 DOM).
    #[test]
    fn browser_js_tree_load_may_commit_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        // CommonJS 钩子 shim: 与 Node 的 module.exports 语义一致 (浏览器无 module).
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        for (expr, expected) in [
            // 壳不当前: 恒拒绝.
            ("treeLoadMayCommit(false, false, false)", false),
            ("treeLoadMayCommit(false, true, true)", false),
            // 壳当前 + 显式用户调用 (无导航所有权): 提交, navCurrent 无关.
            ("treeLoadMayCommit(true, false, false)", true),
            ("treeLoadMayCommit(true, false, true)", true),
            // 壳当前 + 导航所有权: 仅导航当前可提交.
            ("treeLoadMayCommit(true, true, true)", true),
            ("treeLoadMayCommit(true, true, false)", false),
        ] {
            let got = ctx
                .eval(&format!("module.exports.{expr}"))
                .expect("treeLoadMayCommit must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(b) if b == expected),
                "{expr}: expected {expected}, got {got:?}"
            );
        }
    }

    #[test]
    fn browser_js_tree_key_scope_seam_is_locked() {
        let js = BROWSER_JS;
        let helper_start = js
            .find("function treeKeyScope(")
            .expect("treeKeyScope 存在");
        let helper_end = js
            .find("function onTreeKeydown(")
            .expect("onTreeKeydown 位于 treeKeyScope 之后");
        let helper = &js[helper_start..helper_end];
        let esc_at = helper
            .find("if (key === 'Escape') {")
            .expect("helper Escape 分支存在");
        let editable_at = helper
            .find("if (editable) return null;")
            .expect("helper editable 守卫存在");
        let slash_at = helper
            .find("if (key === '/') return 'focus-search';")
            .expect("helper '/' 分支存在");
        assert!(
            esc_at < editable_at && editable_at < slash_at,
            "helper 顺序: Escape 先于 editable 守卫, editable 先于 '/'"
        );
        assert!(
            helper.contains("key === 'PageDown'"),
            "helper 必须覆盖 Page 键"
        );
        assert!(
            js.contains("treeKeyScope: treeKeyScope"),
            "treeKeyScope 必须经 CommonJS 钩子导出"
        );
        // onTreeKeydown: 总闸顺序 — helper 决策 → null 返回 → escape-search →
        // 先收抽屉 (窄屏) → 后失焦 → preventDefault.
        let kd_start = js
            .find("function onTreeKeydown(")
            .expect("onTreeKeydown 存在");
        let kd_end = js
            .find("function init(")
            .expect("init 位于 onTreeKeydown 之后");
        let kd = &js[kd_start..kd_end];
        let scope_at = kd
            .find("const scope = treeKeyScope(")
            .expect("onTreeKeydown 必须经 helper 决策");
        let null_at = kd
            .find("if (scope === null) return;")
            .expect("null 范围直接返回 (不取消默认)");
        let esc_scope_at = kd
            .find("if (scope === 'escape-search') {")
            .expect("escape-search 分支存在");
        let drawer_guard_at = kd
            .find("document.body.classList.contains('cb-drawer-open')")
            .expect("窄屏抽屉判定存在");
        let close_at = kd
            .find("closeTreeDrawer();")
            .expect("escape-search 先收抽屉");
        let blur_at = kd
            .find("searchInput.blur();")
            .expect("escape-search 失焦搜索框");
        let pd_at = kd
            .find("event.preventDefault();")
            .expect("handled 分支阻断默认");
        assert!(
            scope_at < null_at
                && null_at < esc_scope_at
                && esc_scope_at < drawer_guard_at
                && drawer_guard_at < close_at
                && close_at < blur_at
                && blur_at < pd_at,
            "onTreeKeydown 顺序: helper 决策 → null 返回 → escape-search → 窄屏先收抽屉 → 后失焦 → preventDefault"
        );
        // 分支顺序: escape-search → focus-search → tree 范围 Escape.
        let focus_at = kd
            .find("if (scope === 'focus-search') {")
            .expect("focus-search 分支存在");
        let tree_esc_at = kd
            .find("if (event.key === 'Escape') {")
            .expect("tree 范围 Escape 分支存在");
        assert!(
            esc_scope_at < focus_at && focus_at < tree_esc_at,
            "分支顺序: escape-search → focus-search → tree 范围 Escape"
        );
        assert!(
            kd.contains("searchInput.focus()"),
            "focus-search 必须聚焦树搜索框"
        );
        assert!(
            kd.contains("case 'ArrowDown':"),
            "Arrow 键仍在 tree 范围处理"
        );
        assert!(
            !kd.contains("case '/':"),
            "'/' 已由 focus-search 分支接管, switch 不得重复"
        );
    }

    /// 任务 5 收尾 (版本化意图重构) + 异步生命周期隔离: 懒加载并发去重 +
    /// 意图终态 seam. 行为导向源不变量:
    /// - 模块状态: treePendingLoads (path → 在途 Promise) 与 treeExpansionIntents
    ///   (path → { expanded, version, explicit } 意图记录) 声明; 全局单调
    ///   treeExpansionIntentVersion 由发布递增 (跨路径共享时序), 退出树模式
    ///   清空记录但版本计数保留 (跨壳单调, 记录身份比较天然隔离); 意图只在
    ///   退出/叶子化/安装方消费/owner finally 失败兜底清除;
    /// - publishTreeExpansionIntent(path, expanded, explicit): 唯一写入点 —
    ///   递增版本、存储记录并返回; 时序即优先级 — 更新的导航 (explicit=false)
    ///   覆盖更早的显式折叠, 更晚的显式点击覆盖导航;
    /// - treeNextExplicitIntent (纯函数, 导出): 显式用户激活的下一条意图 —
    ///   prior 显式则翻转其 expanded, prior 无/nav 默认则按可见 DOM 推导
    ///   (!expanded; 行为矩阵见 browser_js_tree_expansion_intent_matrix);
    /// - treeLoadMayCommit (纯函数, 导出): 壳当前恒必需; 带导航所有权时还须
    ///   导航当前; 显式用户调用 (无导航所有权) 只要求壳当前 (行为矩阵见
    ///   browser_js_tree_load_may_commit_matrix);
    /// - loadTreeChildren(li, path, owner): owner 传调用方所有权 — { navGen }
    ///   (reveal/sync 导航链) 或 { enterGen } (进入链持久化展开); owner 存在
    ///   时每次调用都发布 nav/默认意图 (expanded=true, explicit=false, 版本
    ///   更新 — 更新的导航覆盖更早显式折叠); 显式用户调用 (无 owner) 已由
    ///   toggleTreeNode/ArrowRight 发布显式意图, 绝不覆盖; 陈旧导航/进入链只
    ///   共享缓存结果, 不提交 DOM/展开/持久化; 缓存命中与异步响应都经
    ///   installTreeChildren 唯一替换点 (空缓存 → 叶子化); 替换前先清除被
    ///   替换子树内的选中 (clearTreeSelection → aria-activedescendant 同步移除);
    ///   终态跟随最新记录 (setTreeNodeExpanded 以 record.expanded 为准, 无记录
    ///   默认 true, 绝不盲目折叠), 安装方在落地终态后按记录身份消费 (DOM 即
    ///   权威 — 续接方晚于 owner finally 注册, finally 不得抢先清除意图);
    ///   在途复用: 每个调用方都在共享 Promise 上挂自己的续接 — 发起方所有权
    ///   陈旧时 (如陈旧导航) 响应只写缓存不落 DOM, 本调用方所有权当前则补齐
    ///   安装 (ensureTreeChildrenInstalled), 绝不 bare-return 发起方 Promise;
    ///   清理只由 owner Promise 在 finally 执行: 先比对在途条目身份, 同一
    ///   owner 块内删除在途条目; 失败兜底只删本请求适用记录 — 无缓存 && 当前
    ///   记录与本请求起始记录身份相同 (请求起始版本 token) 才删除, 绝不删除
    ///   更新导航/显式点击发布的更新记录; 成功落地但无安装方 (陈旧导航只写
    ///   缓存, 有缓存) 不删记录 — 后续发布恒新版本, 陈旧 nav 记录不影响任何
    ///   决策;
    /// - convertTreeLeaf: 空目录响应 → 终端叶子 — treeExpandable='0'、移除
    ///   aria-expanded、清 twist、移除子 role=group、持久化展开全部重复条目
    ///   过滤并保存、删除意图记录 (未来 click/ArrowRight 经 isExpandableDir
    ///   短路, 零请求);
    /// - toggleTreeNode: 在途分支 = 显式用户激活 — 经 treeNextExplicitIntent
    ///   按最新记录推导 (prior 显式翻转 / prior nav 按可见 DOM 推导, nav 的
    ///   隐藏意图绝不参与, 首击可见折叠目录即表达展开), 发布显式意图后恒经
    ///   loadTreeChildren 挂 ownerless 续接, 绝不裸 return、绝不发起第二个
    ///   请求; 无在途展开分支同样经纯函数 + 发布 (用户激活即显式, nav/reveal
    ///   默认不算 — 在途期间的后续激活因此翻转而非再推导); 无在途收起时 DOM
    ///   即权威, 不写意图残留;
    /// - ArrowRight (键盘显式展开): 发布显式 (true, true) 意图后加载 — 最新
    ///   用户动作权威 (覆盖既有收起意图);
    /// - enterTreeMode: 进入链 generation (treeGen += 1 后捕获 enterGen), 旧进入
    ///   链 (buildTreeShell().then / reveal.then / 持久化 walk 递归续接 / 选中
    ///   恢复) 全部以 enterGen === treeGen 守卫 — 退出/重进后旧链不得触碰新壳.
    #[test]
    fn browser_js_tree_load_dedup_seam_is_locked() {
        let js = BROWSER_JS;
        for needle in [
            "const treePendingLoads = new Map()",
            "const treeExpansionIntents = new Map()",
            "let treeExpansionIntentVersion = 0",
            "function treeNextExplicitIntent(",
            "function publishTreeExpansionIntent(",
            "function treeLoadMayCommit(",
            "function loadTreeChildren(",
            "function convertTreeLeaf(",
            "function installTreeChildren(",
            "function ensureTreeChildrenInstalled(",
            "treePendingLoads.clear()",
            "treeExpansionIntents.clear()",
        ] {
            assert!(js.contains(needle), "missing load-dedup seam: {needle}");
        }
        // 旧 bool map + 显式 Set 已整体移除 (统一为版本化意图记录).
        for gone in [
            "treeDesiredExpanded",
            "treeUserExpansionIntent",
            "new Set()",
        ] {
            assert!(
                !js.contains(gone),
                "removed intent seam still present: {gone}"
            );
        }
        // loadTreeChildren: owner 发布 nav/默认意图 → 缓存命中 → 在途复用 → fetch.
        let load_start = js
            .find("function loadTreeChildren(")
            .expect("loadTreeChildren 存在");
        let load_end = js
            .find("function toggleTreeNode(")
            .expect("toggleTreeNode 位于 loadTreeChildren 之后");
        let load = &js[load_start..load_end];
        // owner (nav/进入链) 每次请求都发布 nav/默认意图 — 时序即优先级: 更新的
        // 导航覆盖更早的显式折叠 (绝不因已有意图而跳过发布).
        let publish_at = load
            .find("if (owner) publishTreeExpansionIntent(path, true, false)")
            .expect("owner 发布 nav/默认意图");
        let cache_at = load
            .find("if (treeCache.has(path)) {")
            .expect("缓存命中分支存在");
        assert!(
            publish_at < cache_at,
            "nav/默认意图发布必须先于缓存命中分支 (每次请求都刷新版本)"
        );
        // 在途复用: 先查在途 → 每个调用方挂自己的续接 (绝不 bare-return 发起方
        // Promise) → 仅无在途才发起 fetch.
        let inflight_at = load
            .find("const inFlight = treePendingLoads.get(path)")
            .expect("在途查询存在");
        let reuse_at = load
            .find("return inFlight.then(function (entries) {")
            .expect("在途复用同一 Promise 并挂调用方续接");
        let fetch_at = load
            .find(
                "const pending = fetchTreeEntries(path, treeAbort ? treeAbort.signal : undefined)",
            )
            .expect("发起加载存在 (必须传本壳 signal)");
        assert!(
            inflight_at < reuse_at && reuse_at < fetch_at,
            "必须先在途去重 (复用同一 Promise, 挂调用方续接), 再发起 fetch"
        );
        // 调用方续接: 所有权当前 (treeLoadMayCommit: 壳 + 导航/进入链) 才补齐
        // 安装 — ensureTreeChildrenInstalled (空缓存 → 叶子化; 已安装 → 跳过),
        // 终态由 installTreeChildren 按最新意图记录落地; 陈旧发起方 (如陈旧导航)
        // 的响应只写缓存不落 DOM, 续接方所有权当前时补齐.
        let adopt = &load[reuse_at..fetch_at];
        let adopt_commit_at = adopt
            .find("treeLoadMayCommit(")
            .expect("在途续接必须先经调用方所有权检查");
        let adopt_install_at = adopt
            .find("ensureTreeChildrenInstalled(li, path)")
            .expect("在途续接所有权当前时补齐安装");
        assert!(
            adopt_commit_at < adopt_install_at,
            "在途续接顺序: 所有权检查 → 补齐安装"
        );
        assert!(
            !adopt.contains("treeCache.set"),
            "在途续接绝不重复写缓存 (共享发起方结果)"
        );
        assert_eq!(
            load.matches("fetchTreeEntries(path, treeAbort ? treeAbort.signal : undefined)")
                .count(),
            1,
            "单条加载路径只允许一个 fetch 调用点 (去重契约)"
        );
        // 缓存命中分支: 先经提交决策 (陈旧导航只取缓存结果, 不动 DOM), 再经
        // installTreeChildren 唯一替换点.
        let cache_start = load
            .find("if (treeCache.has(path)) {")
            .expect("缓存命中分支存在");
        let cache_end = load
            .find("const inFlight = treePendingLoads.get(path)")
            .expect("在途分支位于缓存命中分支之后");
        let cache_branch = &load[cache_start..cache_end];
        assert!(
            cache_branch.contains("treeLoadMayCommit("),
            "缓存分支必须先经提交决策 (陈旧导航只取缓存结果, 不提交 DOM)"
        );
        assert!(
            cache_branch.contains("installTreeChildren(li, path)"),
            "缓存分支替换必须经 installTreeChildren 唯一替换点"
        );
        // 异步分支: 壳/进入链守卫 → 缓存写入 → 导航守卫 → 唯一替换点.
        let finally_at = load
            .find(".finally(function () {")
            .expect("owner finally 清理存在");
        let async_branch = &load[fetch_at..finally_at];
        let gen_at = async_branch
            .find("gen !== treeGen")
            .expect("旧壳 generation 守卫存在");
        let cache_set_at = async_branch
            .find("treeCache.set(path, nodes)")
            .expect("缓存写入存在");
        let nav_guard_at = async_branch
            .find("navRequired && !paneNavGuard.isCurrent(navGen)")
            .expect("导航所有权守卫存在");
        let install_at = async_branch
            .find("installTreeChildren(li, path)")
            .expect("异步分支经唯一替换点提交");
        assert!(
            gen_at < cache_set_at && cache_set_at < nav_guard_at && nav_guard_at < install_at,
            "异步分支顺序: 壳守卫 → 缓存写入 → 导航守卫 → 唯一替换点"
        );
        // 请求起始记录捕获: 先于 fetch 发起 (失败兜底的版本 token).
        let intent_record_at = load
            .find("const intentRecord = treeExpansionIntents.get(path)")
            .expect("请求起始记录捕获存在");
        assert!(
            intent_record_at < fetch_at,
            "请求起始记录必须先于 fetch 捕获 (失败兜底只删该记录)"
        );
        // owner 清理: finally 内先比对在途条目身份, owner 块内删在途条目;
        // 意图由安装方 (installTreeChildren) 在落地终态后按记录身份消费 (续接方
        // 晚于 finally 注册, finally 不得抢先清除意图 — 否则折叠意图丢失), 兜底
        // 只清无消费方的意图: 失败响应 (无缓存) 且当前记录与本请求起始记录身份
        // 相同才删 — 绝不删除更新导航/显式点击发布的更新记录 (latest wins);
        // 成功落地但无安装方 (陈旧导航只写缓存, 有缓存) 不删 — 后续发布恒新版本.
        let owner_cmp_at = load
            .find("treePendingLoads.get(path) === pending")
            .expect("owner 身份比对存在");
        let pending_del_at = load
            .find("treePendingLoads.delete(path)")
            .expect("删除在途条目存在");
        let cache_check_at = load
            .find("if (!treeCache.has(path) && treeExpansionIntents.get(path) === intentRecord) {")
            .expect("兜底必须限定失败响应 (无缓存) + 本请求记录身份");
        let intent_del_at = load
            .find("treeExpansionIntents.delete(path)")
            .expect("删除意图记录存在 (失败响应兜底)");
        assert!(
            finally_at < owner_cmp_at
                && owner_cmp_at < pending_del_at
                && pending_del_at < cache_check_at
                && cache_check_at < intent_del_at,
            "owner 清理顺序: finally → 在途身份比对 → 删在途 → 无缓存+记录身份比对 → 删本请求记录"
        );
        assert_eq!(
            load.matches("treeExpansionIntents.delete(path)").count(),
            1,
            "意图记录删除只出现在 owner 失败兜底 (安装方消费在 installTreeChildren)"
        );
        // convertTreeLeaf: 空目录 → 终端叶子契约 (无子项无展开语义, 未来
        // click/ArrowRight 经 isExpandableDir 短路, 零请求).
        let leaf_start = js
            .find("function convertTreeLeaf(")
            .expect("convertTreeLeaf 存在");
        let leaf_end = js
            .find("function installTreeChildren(")
            .expect("installTreeChildren 位于 convertTreeLeaf 之后");
        let leaf = &js[leaf_start..leaf_end];
        for needle in [
            "li.dataset.treeExpandable = '0'",
            "li.removeAttribute('aria-expanded')",
            "twist.textContent = ''",
            "ul.remove()",
            "treeExpansionIntents.delete(path)",
        ] {
            assert!(
                leaf.contains(needle),
                "convertTreeLeaf 缺少叶子化契约: {needle}"
            );
        }
        // 持久化展开: 过滤全部重复条目 (绝不 indexOf+splice 只删首条) 并保存.
        assert!(
            leaf.contains("treeExpandedPaths = treeExpandedPaths.filter("),
            "convertTreeLeaf 必须过滤全部重复持久化条目"
        );
        assert!(
            leaf.contains("saveJsonArray(TREE_EXPANDED_KEY, treeExpandedPaths)"),
            "过滤后必须保存持久化展开"
        );
        // installTreeChildren: 唯一替换点 — 替换前清被替换子树内的选中,
        // 读最新记录落地终态, 按记录身份消费 (绝不误删更新记录).
        let inst_start = js
            .find("function installTreeChildren(")
            .expect("installTreeChildren 存在");
        let inst_end = js
            .find("function ensureTreeChildrenInstalled(")
            .expect("ensureTreeChildrenInstalled 位于 installTreeChildren 之后");
        let inst = &js[inst_start..inst_end];
        let inst_clear_at = inst
            .find("clearTreeSelection();")
            .expect("唯一替换点替换前清除选中");
        let inst_replace_at = inst
            .find("ul.replaceChildren(")
            .expect("唯一替换点替换子项");
        let inst_record_at = inst
            .find("const record = treeExpansionIntents.get(path)")
            .expect("安装方读取最新意图记录");
        let inst_set_at = inst
            .find("setTreeNodeExpanded(li, record ? record.expanded : true)")
            .expect("落地终态先经 setTreeNodeExpanded (record.expanded 或默认 true)");
        let inst_consume_at = inst
            .find("treeExpansionIntents.get(path) === record")
            .expect("安装方必须按记录身份比对后消费");
        let inst_del_at = inst
            .find("treeExpansionIntents.delete(path)")
            .expect("安装方消费意图记录 (落地终态后)");
        assert!(
            inst_clear_at < inst_replace_at
                && inst_replace_at < inst_record_at
                && inst_record_at < inst_set_at
                && inst_set_at < inst_consume_at
                && inst_consume_at < inst_del_at,
            "唯一替换点: 清选中 → replaceChildren → 读最新记录 → 终态 setTreeNodeExpanded → 同记录身份消费"
        );
        // toggleTreeNode: 在途只发布显式意图, 不发起第二个请求; 无在途按可见态.
        let toggle_end = js
            .find("function selectTreeNode(")
            .expect("selectTreeNode 位于 toggleTreeNode 之后");
        let toggle = &js[load_end..toggle_end];
        // 在途分支 = 显式用户激活: 经 treeNextExplicitIntent 按最新记录推导
        // (prior 显式翻转 / prior nav 按可见 DOM 推导), 发布显式意图后恒经
        // loadTreeChildren 挂 ownerless 续接 — 响应落地时续接方安装, 终态跟随
        // 最新记录; 绝不裸 return、绝不发起第二个请求.
        let toggle_pending_at = toggle.find("if (pending) {").expect("在途分支存在");
        let toggle_intent_at = toggle
            .find("treeNextExplicitIntent(treeExpansionIntents.get(path), expanded)")
            .expect("toggle 必须经意图纯函数推导 (最新记录 + 可见态)");
        let toggle_publish_at = toggle
            .find("publishTreeExpansionIntent(path, next.expanded, true)")
            .expect("toggle 必须发布显式意图 (版本化)");
        let toggle_adopt_at = toggle
            .find("return loadTreeChildren(li, path);")
            .expect("在途激活必须挂 ownerless 续接 (绝不裸 return)");
        assert!(
            toggle_pending_at < toggle_intent_at
                && toggle_intent_at < toggle_publish_at
                && toggle_publish_at < toggle_adopt_at,
            "在途分支: 纯函数推导 → 发布显式意图 → 挂续接 (绝不裸 return / 绝不二次请求)"
        );
        // 无在途分支: 收起直接改 DOM (意图无消费方, 绝不写入残留 — 陈旧意图会
        // 挡住后续 reveal 的缓存命中展开); 展开经纯函数 + 发布 (用户激活即显式,
        // nav/reveal 默认不算 — 在途期间的后续激活因此翻转而非再推导).
        let toggle_nopending_at = toggle.find("if (expanded) {").expect("无在途分支存在");
        let toggle_else_at = toggle.find("} else {").expect("无在途分支存在 else");
        let toggle_collapse = &toggle[toggle_nopending_at..toggle_else_at];
        assert!(
            !toggle_collapse.contains("treeExpansionIntents"),
            "无在途收起分支不得触碰意图 (DOM 直接权威)"
        );
        let toggle_expand = &toggle[toggle_else_at..];
        assert!(
            toggle_expand.contains("treeNextExplicitIntent("),
            "无在途展开分支必须经意图纯函数推导"
        );
        assert!(
            toggle_expand.contains("publishTreeExpansionIntent(path, next.expanded, true)"),
            "无在途展开分支必须发布显式意图"
        );
        assert!(
            toggle_expand.contains("loadTreeChildren(li, path)"),
            "无在途展开分支必须经 loadTreeChildren 加载"
        );
        // ArrowRight (键盘显式展开): 发布显式 (true, true) 意图后加载 — 最新
        // 用户动作权威 (覆盖既有收起意图); 绝不写 bool map/Set 残留.
        let kd_start = js
            .find("function onTreeKeydown(")
            .expect("onTreeKeydown 存在");
        let kd_end = js
            .find("function init(")
            .expect("init 位于 onTreeKeydown 之后");
        let kd = &js[kd_start..kd_end];
        let ar_at = kd.find("case 'ArrowRight':").expect("ArrowRight 分支存在");
        let ar_end = kd
            .find("case 'ArrowLeft':")
            .expect("ArrowLeft 位于 ArrowRight 之后");
        let ar_branch = &kd[ar_at..ar_end];
        assert!(
            ar_branch.contains("publishTreeExpansionIntent(path, true, true)"),
            "ArrowRight 必须发布显式展开意图"
        );
        assert!(
            ar_branch.contains("loadTreeChildren(treeSelectedEl, path)"),
            "ArrowRight 必须经 loadTreeChildren 加载"
        );
        // enterTreeMode: 先递增树壳 generation 再捕获 enterGen; 进入链每处异步
        // 边界后 (buildTreeShell().then / reveal.then / walk 递归续接 / 选中恢复)
        // 都必须以 enterGen === treeGen 守卫 — 旧进入链不得触碰新壳全局.
        let enter_start = js
            .find("function enterTreeMode()")
            .expect("enterTreeMode 存在");
        let enter_end = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 位于 enterTreeMode 之后");
        let enter = &js[enter_start..enter_end];
        let inc_at = enter
            .find("treeGen += 1")
            .expect("进入链递增树壳 generation");
        let enter_gen_at = enter
            .find("const enterGen = treeGen")
            .expect("进入链捕获 enterGen");
        assert!(inc_at < enter_gen_at, "必须先递增再捕获 enterGen");
        assert!(
            enter.matches("enterGen !== treeGen").count() >= 4,
            "进入链每处异步边界后都必须有 enterGen 守卫 (buildTreeShell.then / reveal.then / walk 续接 / 选中恢复)"
        );
        // exitTreeMode: 清空在途与意图记录表 (随树缓存整体销毁); 版本计数保留
        // (跨壳单调, 记录身份比较天然隔离).
        let exit_start = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 存在");
        let exit = &js[exit_start..];
        let cache_clear_at = exit.find("treeCache.clear()").expect("退出清空树缓存");
        let pending_clear_at = exit
            .find("treePendingLoads.clear();")
            .expect("退出清空在途表");
        let intent_clear_at = exit
            .find("treeExpansionIntents.clear();")
            .expect("退出清空意图记录表");
        assert!(
            cache_clear_at < pending_clear_at && pending_clear_at < intent_clear_at,
            "exitTreeMode: 先清树缓存, 再清在途表与意图记录表"
        );
        assert!(
            !exit.contains("treeExpansionIntentVersion = 0"),
            "版本计数不得在退出时清零 (跨壳单调)"
        );
    }

    /// 任务 5 收尾: 树根加载失败契约. buildTreeShell 的根 JSON 请求带拒绝
    /// 处理 (then 双参): 壳退出中止 (AbortError) 与旧壳 (gen !== treeGen)
    /// 一律静默忽略 — 无可见陈旧 alert、无未处理拒绝; 当前壳清空根、把纯文本
    /// role=alert 错误 (textContent, 绝不 innerHTML) 挂为 nav 内 treeRootUl
    /// 的兄弟 (绝不直接挂进 role=tree 的 ul), 链路 resolve; 成功分支清除
    /// 既有错误元素 (重试/成功即清理); 重进/重试由新壳全新请求恢复.
    #[test]
    fn browser_js_tree_root_fetch_failure_seam_is_locked() {
        let js = BROWSER_JS;
        let shell_at = js
            .find("function buildTreeShell(")
            .expect("buildTreeShell 存在");
        let drawer_at = js
            .find("/* ---------- 窄屏抽屉")
            .expect("buildTreeShell 位于抽屉段之前");
        let shell = &js[shell_at..drawer_at];
        let root_at = shell
            .find("return fetchTreeEntries(''")
            .expect("根加载存在 (then 双参: 成功 + 失败)");
        let success_at = shell
            .find("treeRootUl.replaceChildren(...nodes)")
            .expect("根加载成功替换子项");
        let fail_fn_at = shell[success_at..]
            .find("function (err) {")
            .expect("根加载失败处理函数存在 (带 err 参数, AbortError 守卫)");
        let fail_fn_at = success_at + fail_fn_at;
        assert!(
            root_at < success_at && success_at < fail_fn_at,
            "根加载: then 成功分支先于失败分支"
        );
        let fail = &shell[fail_fn_at..];
        for needle in [
            "err.name === 'AbortError'",
            "if (gen !== treeGen) return;",
            "treeCache.delete('')",
            "treeRootUl.replaceChildren()",
            "errEl.className = 'cb-tree-error'",
            "errEl.setAttribute('role', 'alert')",
            "errEl.textContent = '无法加载目录树'",
            "treeNav.append(errEl)",
        ] {
            assert!(fail.contains(needle), "根失败处理缺少契约: {needle}");
        }
        // AbortError 守卫必须先于一切 (中止请求绝无可见 alert), 再是旧壳守卫.
        let abort_at = fail.find("AbortError").expect("AbortError 守卫存在");
        let gen_guard_at = fail
            .find("if (gen !== treeGen) return;")
            .expect("旧壳守卫存在");
        let alert_at = fail
            .find("treeNav.append(errEl)")
            .expect("alert 挂到 nav (ul role=tree 的兄弟)");
        assert!(
            abort_at < gen_guard_at && gen_guard_at < alert_at,
            "根失败顺序: AbortError 静默 → 旧壳忽略 → 当前壳才挂 alert"
        );
        assert!(
            !fail.contains("treeRootUl.append"),
            "alert 绝不挂进 role=tree 的根 ul (必须是 nav 内兄弟)"
        );
        assert!(
            !fail.contains("innerHTML"),
            "根失败错误只经 textContent (绝不 innerHTML)"
        );
        // 成功分支: 清除既有 .cb-tree-error (重试/成功即清理).
        let success = &shell[root_at..success_at];
        assert!(
            success.contains("prevErr.remove()") && success.contains(".cb-tree-error"),
            "成功分支必须清除既有根错误元素"
        );
    }

    /// 任务 5 收尾: 树请求按壳中止 + 同步守卫顺序. 每壳一个 AbortController:
    /// 进入时创建 (先于 root/子项请求), 退出时 abort 并置空 — 在途树请求以
    /// AbortError 收尾, 失败处理器静默吞掉; fetchTreeEntries 接收 signal 并
    /// 仅在存在时附加 (root 与子项两个调用点都传); syncTreeToPath 必须先查
    /// paneNavGuard 当前性再执行 clearTreeCurrent 变异 (陈旧导航零 mutation).
    #[test]
    fn browser_js_tree_shell_abort_and_sync_guard_seam_is_locked() {
        let js = BROWSER_JS;
        assert!(
            js.contains("let treeAbort = null"),
            "树壳级 AbortController 状态必须声明"
        );
        // enterTreeMode: 先递增壳 generation, 再创建本壳 AbortController.
        let enter_at = js
            .find("function enterTreeMode()")
            .expect("enterTreeMode 存在");
        let enter_end = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 位于 enterTreeMode 之后");
        let enter = &js[enter_at..enter_end];
        let enter_gen_at = enter
            .find("const enterGen = treeGen")
            .expect("进入链捕获 enterGen");
        let abort_create_at = enter
            .find("treeAbort = new AbortController()")
            .expect("进入时必须创建本壳 AbortController");
        assert!(
            enter_gen_at < abort_create_at,
            "enterTreeMode: 创建 AbortController 必须先于 root/子项请求"
        );
        // exitTreeMode: 先递增壳 generation, 再 abort + 置空.
        let exit_at = js
            .find("function exitTreeMode()")
            .expect("exitTreeMode 存在");
        let exit = &js[exit_at..];
        let exit_gen_at = exit.find("treeGen += 1").expect("退出递增壳 generation");
        let abort_at = exit
            .find("treeAbort.abort()")
            .expect("退出必须中止本壳在途树请求");
        let abort_null_at = exit
            .find("treeAbort = null")
            .expect("退出后置空 AbortController");
        assert!(
            exit_gen_at < abort_at && abort_at < abort_null_at,
            "exitTreeMode: 递增 → abort → 置空"
        );
        // fetchTreeEntries: 接收 signal, 仅在存在时附加 (两个调用点都传).
        let fetch_at = js
            .find("function fetchTreeEntries(path, signal)")
            .expect("fetchTreeEntries 必须接收 signal");
        let fetch_end = js
            .find("// 展开意图纯函数")
            .expect("fetchTreeEntries 位于展开意图纯函数之前");
        let fetch = &js[fetch_at..fetch_end];
        assert!(
            fetch.contains("opts.signal = signal") && fetch.contains("if (signal)"),
            "fetchTreeEntries 必须仅在 signal 存在时附加 (无 signal 不传 abort 选项)"
        );
        let shell_at = js
            .find("function buildTreeShell(")
            .expect("buildTreeShell 存在");
        let shell = &js[shell_at..];
        assert_eq!(
            shell
                .matches("fetchTreeEntries('', treeAbort ? treeAbort.signal : undefined)")
                .count(),
            1,
            "根加载必须把本壳 signal 传给 fetchTreeEntries"
        );
        let load_at = js
            .find("function loadTreeChildren(")
            .expect("loadTreeChildren 存在");
        let load = &js[load_at..js.find("function toggleTreeNode(").expect("toggleTreeNode")];
        assert!(
            load.contains("fetchTreeEntries(path, treeAbort ? treeAbort.signal : undefined)"),
            "子项加载必须把本壳 signal 传给 fetchTreeEntries"
        );
        // 子项失败处理器: AbortError 静默 (中止不标记失败/不重试).
        let child_fail = &load[load.find("function (err) {").expect("子项失败处理器")..];
        assert!(
            child_fail.contains("err.name === 'AbortError'"),
            "子项加载失败处理器必须静默吞掉 AbortError"
        );
        // syncTreeToPath: paneNavGuard 当前性检查必须先于 clearTreeCurrent 变异.
        let sync_at = js
            .find("function syncTreeToPath(")
            .expect("syncTreeToPath 存在");
        let sync_end = js
            .find("function paneInitialLoad(")
            .expect("paneInitialLoad 位于 syncTreeToPath 之后");
        let sync = &js[sync_at..sync_end];
        let guard_at = sync
            .find("if (!paneNavGuard.isCurrent(gen)) return;")
            .expect("syncTreeToPath 必须带导航当前性守卫");
        let mutate_at = sync
            .find("clearTreeCurrent()")
            .expect("clearTreeCurrent 变异存在");
        assert!(
            guard_at < mutate_at,
            "syncTreeToPath: 陈旧导航不得执行任何 clearTreeCurrent 变异"
        );
    }
}
