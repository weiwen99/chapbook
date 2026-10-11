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

/// Mermaid 图表库 (v11.17.2, 官方 dist 的 IIFE 单文件自包含构建, MIT,
/// 见 THIRD_PARTY_NOTICES): 浏览器端把 `<pre class="mermaid">` 渲染为 SVG;
/// 由 chapbook-browser.js 按需懒加载, 仅页面含 mermaid 代码块时才请求.
pub const MERMAID_JS: &[u8] = include_bytes!("../assets/mermaid.min.js");

/// 浏览器渐进增强 JS (目录页/搜索页): 编译期内嵌, 由共享页面 head 引用.
pub const BROWSER_JS: &str = include_str!("../assets/chapbook-browser.js");

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 可执行行为矩阵: 折叠/展开焦点落点纯函数 (collapseFocusTarget) 在 QuickJS
    /// 中真实执行, 覆盖全部过渡与 no-op:
    /// - 折叠生效 (nextCollapsed): 焦点在树内/调整器内/抽屉唤起按钮 → 'restore'
    ///   (inert/隐藏元素不可聚焦, 必须提前移交); 焦点在恢复按钮自身或受影响
    ///   控件之外 → null (恢复按钮保持可见, 无需移交);
    /// - 展开生效 (!nextCollapsed): 焦点在恢复按钮 → 窄屏 'drawer' (恢复按钮
    ///   即将隐藏, 抽屉唤起按钮可见) / 宽屏 'collapse' (折叠按钮在树顶栏内,
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

    /// 可执行行为矩阵: PDF 只在「条目 kind=file + 编码路径后缀 .pdf」时走原生
    /// 新 tab 语义 (folder.pdf 目录与后缀不匹配不误判); decode 仍恰一次, 钩子经
    /// CommonJS 暴露 (原生打开与面板流程的浏览器证据见 pdf 验收脚本).
    #[test]
    fn browser_js_is_pdf_file_matrix() {
        let ctx = quick_js::Context::new().expect("quick-js context");
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");
        for (path, kind, expected) in [
            ("doc.pdf", "file", true),
            ("a/b/doc.pdf", "file", true),
            ("DOC.PDF", "file", true),
            ("folder.pdf", "dir", false),
            ("doc.pdf", "dir", false),
            ("doc.md", "file", false),
            ("doc.pdfx", "file", false),
            ("", "file", false),
        ] {
            let expr = format!(
                "module.exports.isPdfFile({}, {})",
                serde_json::to_string(path).unwrap(),
                serde_json::to_string(kind).unwrap()
            );
            let got = ctx.eval(&expr).expect("isPdfFile must not throw");
            assert!(
                matches!(got, quick_js::JsValue::Bool(b) if b == expected),
                "{expr}: expected {expected}"
            );
        }
        // 无新增 decode 出口 (native-open 独占唯一一次)
        assert_eq!(BROWSER_JS.matches("decodeURIComponent(").count(), 1);
        assert!(BROWSER_JS.contains("isPdfFile: isPdfFile"));
    }
    /// 可执行行为矩阵: 新版浏览状态纯 seam (QuickJS 真实执行真实导出) —
    /// 版本化提交裁决 (treeLoadDecision)、视图循环 (nextBrowseView)、
    /// 隐藏焦点回退 (nearestVisibleIndex)、父编码路径 (parentEncodedPath)、
    /// query 精确语法 (isValidSort/isValidHidden) 与 managed 快照 (isValidSnapshot).
    #[test]
    fn browser_js_browse_state_pure_seams_matrix() {
        fn eval_str(ctx: &quick_js::Context, expr: &str) -> Option<String> {
            let got = ctx.eval(expr).expect("pure seam must not throw");
            got.as_str().map(str::to_string)
        }
        fn eval_int(ctx: &quick_js::Context, expr: &str) -> Option<i32> {
            match ctx.eval(expr).expect("pure seam must not throw") {
                quick_js::JsValue::Int(n) => Some(n),
                _ => None,
            }
        }
        fn eval_bool(ctx: &quick_js::Context, expr: &str) -> bool {
            matches!(
                ctx.eval(expr).expect("pure seam must not throw"),
                quick_js::JsValue::Bool(true)
            )
        }

        let ctx = quick_js::Context::new().expect("quick-js context");
        ctx.eval("globalThis.module = { exports: {} };")
            .expect("CommonJS module shim");
        ctx.eval(BROWSER_JS).expect("eval BROWSER_JS");

        // treeLoadDecision(shellCurrent, versionCurrent, navRequired, navCurrent):
        // 旧壳 drop; 旧选项版本 success/error 一律 stale; 当前版本但导航过期只共享
        // 缓存 (cache); 壳/版本/导航全当前才 install.
        for (shell, version, nav_required, nav_current, expected) in [
            (false, true, false, true, "drop"),
            (false, false, false, false, "drop"),
            (false, false, true, false, "drop"),
            (true, false, true, true, "stale"),
            (true, false, false, false, "stale"),
            (true, true, true, false, "cache"),
            (true, true, true, true, "install"),
            (true, true, false, false, "install"),
        ] {
            let expr = format!(
                "module.exports.treeLoadDecision({shell}, {version}, {nav_required}, {nav_current})"
            );
            assert_eq!(eval_str(&ctx, &expr), Some(expected.to_string()), "{expr}");
        }

        // nextBrowseView: list -> grid -> tree -> list (Q16)
        for (from, to) in [("list", "grid"), ("grid", "tree"), ("tree", "list")] {
            let expr = format!("module.exports.nextBrowseView('{from}')");
            assert_eq!(eval_str(&ctx, &expr), Some(to.to_string()), "{expr}");
        }

        // nearestVisibleIndex: 含起点向前扫描, 再向后; 空/全不可见 -> -1; 越界钳制 (Q17)
        for (visible, from_index, expected) in [
            ("[true,false,true]", 1, 2),
            ("[true,false,false]", 1, 0),
            ("[false,false,true]", 0, 2),
            ("[true,false]", 1, 0),
            ("[false]", 0, -1),
            ("[]", 0, -1),
            ("[true]", 5, 0),
            ("[true,false,true]", -5, 0),
        ] {
            let expr = format!("module.exports.nearestVisibleIndex({visible}, {from_index})");
            assert_eq!(eval_int(&ctx, &expr), Some(expected), "{expr}");
        }

        // parentEncodedPath: 单段 -> 根 '', 根/非法 -> null (校验与 transport 相同)
        for (input, expected) in [
            ("a/b/c", Some("a/b")),
            ("a", Some("")),
            ("", None),
            ("/a", None),
            ("a/", None),
            ("a//b", None),
            ("a/%2e", None),
            ("bad%", None),
        ] {
            let expr = format!(
                "module.exports.parentEncodedPath({})",
                serde_json::to_string(input).unwrap()
            );
            assert_eq!(
                eval_str(&ctx, &expr),
                expected.map(str::to_string),
                "{expr}"
            );
        }

        // isValidSort: 六键 x 两方向精确语法; 大小写/结构/normalize 不合法
        for column in [
            "Name",
            "Type",
            "Size",
            "LastModified",
            "LastAccess",
            "Creation",
        ] {
            for order in ["Asc", "Desc"] {
                let value = format!("{column}:{order}");
                let expr = format!(
                    "module.exports.isValidSort({})",
                    serde_json::to_string(&value).unwrap()
                );
                assert!(eval_bool(&ctx, &expr), "must accept {value}");
            }
        }
        for invalid in [
            "name:Asc",
            "Name:asc",
            "NAME:ASC",
            "Name",
            "Name:",
            "Name:Asc:Desc",
            "Name:Invalid",
            "",
            " Size:Asc",
            "Name:Asc ",
            "Name:Asc\n",
        ] {
            let expr = format!(
                "module.exports.isValidSort({})",
                serde_json::to_string(invalid).unwrap()
            );
            assert!(!eval_bool(&ctx, &expr), "must reject {invalid:?}");
        }

        // isValidHidden: 仅 '0' | '1'
        for valid in ["0", "1"] {
            let expr = format!("module.exports.isValidHidden('{valid}')");
            assert!(eval_bool(&ctx, &expr), "must accept {valid:?}");
        }
        for invalid in ["", "2", "01", "00", "true", "false", "0 ", " 1", "0\0"] {
            let expr = format!(
                "module.exports.isValidHidden({})",
                serde_json::to_string(invalid).unwrap()
            );
            assert!(!eval_bool(&ctx, &expr), "must reject {invalid:?}");
        }

        // isValidSnapshot: managed 历史快照形状 (版本/表面/视图/规则/身份/可空性)
        for snap in [
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'tree',path:'a/b',q:null,hash:'#x',sort:'Size:Desc',hidden:1,opened:'a/b/f.txt',openedKind:'reading',selected:'a/b'}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:'.hidden.pdf',openedKind:'directory',selected:null}",
            "{v:1,surface:'search',view:'grid',path:'',q:'hi',hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'reading',view:null,path:'a.md',q:null,hash:'#h',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:'a.md'}",
            "{v:1,surface:'reading',view:'tree',path:'a.md',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
        ] {
            let expr = format!("module.exports.isValidSnapshot({snap})");
            assert!(eval_bool(&ctx, &expr), "must accept {snap}");
        }
        for snap in [
            "null",
            "42",
            "'x'",
            "{}",
            "{v:2,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:'1',surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'browse',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'reading',view:'list',path:'a.md',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'reading',view:null,path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:null,path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:'x',hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'search',view:'grid',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'search',view:'grid',path:'a',q:'hi',hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:'true',opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:2,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:'/a',openedKind:'reading',selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,openedKind:null,selected:null}",
            "{v:1,surface:'search',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:null,selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:null,openedKind:'reading',selected:null}",
            "{v:1,surface:'directory',view:'list',path:'',q:null,hash:'',sort:'Name:Asc',hidden:0,opened:'a.md',openedKind:'file',selected:null}",
        ] {
            let expr = format!("module.exports.isValidSnapshot({snap})");
            assert!(!eval_bool(&ctx, &expr), "must reject {snap}");
        }
    }
}
