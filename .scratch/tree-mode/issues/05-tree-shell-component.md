# 05: 树模式外壳与树组件

**What to build:** JS 树模式外壳：文档/代码/Office 完整页加载浏览器脚本；浮动切换按钮（搜索页除外）+ localStorage 记忆；左侧目录树——懒加载、目录在前排序、键盘导航、badge、树顶搜索框、深链高亮、非 UTF-8 display-only、环节点终止展开；无 JS 时永远目录模式。

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] 所有可挂树壳完整页（目录/文档/代码/Office）出现浮动切换按钮（右上角统一位置）；搜索页无按钮、无树壳
- [ ] 默认目录模式；切到树模式后 localStorage 记忆，刷新保持；切回目录模式恢复原页面行为
- [ ] 无 JS：始终目录模式，浏览功能完整（渐进增强契约保持）
- [ ] 树：根起懒加载（展开才请求端点）；目录在前 + 名字升序；文件夹图标 + 扩展名 badge；树顶搜索框跳独立搜索页
- [ ] 单击展开/收起不导航；Enter/双击导航（此票阶段为整页导航）；键盘 ↑↓→←/Enter/Backspace/Esc/`/` 生效
- [ ] 展开集与选中路径 localStorage 恢复；深链时当前路径高亮并展开祖先链
- [ ] 非 UTF-8 名条目 display-only（无 action）；`expandable: false` 目录无展开箭头、仍显示、仍可按 URL 导航
- [ ] Chromium 证据：DOM 注入文件名（`<img src=x onerror=…>`、引号、bidi/control、字面 `+`/`%`）无元素/脚本注入，可见转义与 action href 与目录模式一致
- [ ] 既有全部测试通过；`cargo fmt --check` 与 clippy 零警告

## Comments

2026-08-25 security addition (DesignGate): directory full pages carry `meta[name=cb-native-open-token]` + native controls, and browser JS reads the token at startup. Entering 树模式 MUST remove/clear the token meta and native controls and gate all native action paths (including `o`) as no-ops — token value may live in a JS-scoped variable read once at startup and restored on returning to 目录模式. Chromium acceptance: zero `/__/native-open` POSTs while in 树模式.

## Comments

2026-08-25 implementation committed (JS shell + CSS); server contracts locked by cargo tests (207 green), JS syntax checked. PENDING: Seam B Chromium acceptance evidence — DOM-injection fixtures, no-JS fallback, mode persistence, keyboard, deep-link highlight, native-open POST absence. Browser tool was unavailable this session; to be produced in the verification pass.

## Comments (2026-08-25 acceptance)

Seam B Chromium acceptance PASS 11/11 (headless Chromium via CDP; screenshots /tmp/cb05-shots/): toggle presence/absence (dir+doc yes, search no), mode persistence + reload restore, dirs-first root order with badges, lazy load on expand (single GET /__/tree/subdir), cycle node loop unexpandable (no twist, zero fetches), Enter/dblclick full-page navigation, deep-link highlight with ancestor expansion + cb-tree-expanded persistence, hostile filename `<img src=x onerror=alert(1)>.md` rendered as escaped text (zero injected elements, no alert), native-open fully gated in tree mode (token meta absent, zero /__/native-open POSTs, directory-mode contract intact after toggle back), no-JS server-side listing intact, full keyboard matrix (↑↓→←/Enter/Backspace/Esc//). Ticket 05 complete.
