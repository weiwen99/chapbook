# Spec: 树模式浏览与右侧 TOC 侧栏

Status: ready-for-agent

## Problem Statement

用户对现有 UI 不满意：目录浏览只有单一的服务端表格页，无法像编辑器一样在树中总览并导航；
渲染文档的 TOC 目前在正文顶部（宽屏时固定左侧），阅读长文时目录位置不顺手。需要一个编辑器式
树状浏览外壳（与现有目录模式可切换），并把文档 TOC 移到内容右侧。

## Solution

新增树模式：客户端外壳（服务器 HTML 与 URL 语义不变），左侧懒加载目录树 + 右侧内容区，
SPA 导航；浮动按钮在目录模式与树模式间切换（localStorage 记忆，默认目录模式）；无 JS 时只有
目录模式。文档页 TOC 在宽屏时固定到内容右侧并随滚动高亮当前标题；所有 CB 完整页升级为可信 UI
（反嵌入头）。

## User Stories

1. As a user, I want to switch between 目录模式 and 树模式 via one floating button on every tree-capable full page (directory, rendered doc, code, Office; search excluded), so that I can pick the browsing style I prefer.
2. As a user, I want my mode choice remembered across reloads and sessions, so that I don't re-select it each time.
3. As a returning user, I want 目录模式 to remain the default on a fresh browser, so that existing behavior stays intact.
4. As a no-JS user, I want to always get 目录模式, so that browsing remains fully functional without scripts.
5. As a user in 树模式, I want the same URLs and deep links to work as in 目录模式, so that links remain shareable and mode-independent.
6. As a user, I want the search page to stay a standalone full page in both modes, so that search results are not entangled with tree navigation.
7. As a user, I want a collapsible directory tree on the left showing the served root and its subdirectories, so that I can see the structure like in an editor.
8. As a user, I want tree nodes to load lazily when I expand a directory, so that large trees don't freeze the browser.
9. As a user, I want directories listed before files and alphabetically within each group in the tree, so that navigation is predictable.
10. As a user, I want single-click to expand/collapse a directory without navigating, and Enter/double-click to navigate to it, so that browsing and opening are distinct gestures.
11. As a user, I want tree keyboard navigation (↑↓ move, → expand, ← collapse, Enter open, Backspace to parent, Esc close drawer/clear selection, `/` focus search), so that I can browse without a mouse.
12. As a user, I want my expanded directories and current selection restored after reload, so that I don't rebuild the tree each time.
13. As a user, I want folder icons and file extension badges on tree nodes, so that I can distinguish entries at a glance.
14. As a user, I want a search box at the top of the tree leading to the standalone search page, so that search stays reachable in 树模式.
15. As a user on a narrow screen, I want the tree to become an off-canvas drawer that opens via button and closes on selection, so that content still gets full width.
16. As a user, I want the tree to highlight the current URL path including deep links, so that I always know where I am.
17. As a user, I want files/directories with non-UTF-8 names to appear display-only (escaped, unexpandable, no broken links), so that identity and display stay strictly separated.
18. As a user, I want the tree to follow directory symlinks while staying cycle-safe: cyclic links (a symlink back to an ancestor) stay visible and navigable by URL, but their inline expansion in the tree is terminal, so that the tree can never nest infinitely.
19. As a user, I want opening a file from the tree to show it in the content pane without a full reload, so that tree state and context survive navigation.
20. As a user, I want the URL to update on every in-tree navigation and back/forward to work, so that browser history behaves as expected.
21. As a user, I want rapid successive navigations to always end on the last one I requested, so that the pane and URL never disagree.
22. As a user, I want directories opened via deep link to show a mini listing in the content pane, so that directory URLs remain useful in 树模式.
23. As a user, I want rendered documents (.md/.org/Office/CSV), code, and media to display in the content pane exactly as in 目录模式 rendering, so that nothing is lost.
24. As a user, I want no preview panel in 树模式, so that the interface stays focused on tree + content; the preview panel stays 目录模式-only.
25. As a reader on a wide screen, I want the doc TOC fixed on the right of the content, so that it doesn't eat vertical space and matches reading flow.
26. As a reader scrolling a doc, I want the TOC to highlight the current heading, so that I can see where I am in the document.
27. As a reader on a narrow screen, I want the TOC to fall back to the existing top card, so that nothing is lost.
28. As a reader in 树模式, I want the same right-side TOC and scroll highlight inside the content pane (scrolling within the pane), so that both modes look and behave the same.
29. As a user of the 目录模式 preview panel, I want the panel unchanged (TOC as top card, no rail), so that the small panel stays readable.
30. As a reader, I want documents without TOC to show no rail, so that code pages and TOC-less docs keep full width.
31. As a reader, I want math (KaTeX) in documents opened in 树模式 to be styled correctly, so that rendered math never degrades when arriving via tree navigation.
32. As the server operator, I want all CB-generated full pages (directory/search/doc/code/Office) to carry anti-framing headers, so that the trusted-UI shell cannot be framed by another origin.
33. As the server operator, I want the tree JSON endpoint to resolve paths through the same root-boundary check as file serving, so that no second trust boundary is introduced.
34. As the server operator, I want filenames arriving via JSON to be inserted into the DOM only via createElement/textContent inside bdi, so that hostile filenames cannot inject markup.
35. As the server operator, I want native-open to remain unavailable in 树模式, so that the local-open capability stays behind its existing gates.

## Implementation Decisions

**浏览模式与外壳**

- 树模式是 `chapbook-browser.js` 的客户端外壳；服务器 HTML 与 URL 语义不变；模式偏好、树展开集、
  选中路径存 localStorage；默认目录模式。见 ADR 0001。
- 浮动切换按钮由 JS 注入，出现在所有可挂树壳的完整页（目录/文档/代码/Office），右上角统一位置；
  搜索页除外（独立全屏、无树壳、无按钮）。文档页（含代码/Office 渲染页）新增加载
  `chapbook-browser.js`（defer）。
- 搜索页不挂树壳、不显示切换按钮；无 JS 时永远目录模式（服务器不感知模式）。

**树与端点**

- 新增 `GET /__/tree/{*path}` 与显式根路由；axum 解码后的路径逐分量过 `resolve_within_root`
  （与文件服务同一防线）；子项经 `list_dir`。
- JSON 契约：`{"entries": [{"name": String, "href": String|null, "dir": bool, "expandable": bool}]}`。
  `name` 为显示 codec 产物（`FileMeta::display_name`，HTML 转义前的纯文本）；`href` 为
  `FileMeta::href()`（root-relative RFC 3986 编码）；路径含非 UTF-8 segment 时 `href` 为 null，
  节点 display-only 且 `expandable` 为 false。`expandable` 仅目录携带：正常目录 true；目录
  identity 与任一祖先（含请求路径自身）相同（符号链接环，如指向 root 或上层目录）时为 false——
  树内联展开终止，但该节点仍显示、仍可按 URL 导航（Enter / 直接访问）。越界 403、不存在/非目录
  404，错误体为小 JSON；`Content-Type: application/json`。
- 环检测在服务端：请求路径各祖先与每个子目录的 identity 复用搜索遍历的同一 seam
  （Unix `(dev, ino)` / 非 Unix canonical path），必要时将该 seam 提升为 crate 可见；客户端不做
  第二套 identity 判定。
- 树固定排序：目录在前 + 名字升序（`display_name` byte 序）；不吃 `?sort=`。
- 单击展开/收起、Enter/双击导航；键盘 ↑↓→←/Enter/Backspace/Esc/`/`。
- 窄屏（< 64rem）用自研零依赖抽屉（CSS transform + 遮罩），两种壳共用；不用 Materialize
  sidenav（其 JS 目前未加载，文档页无 Materialize 资产）。
- 节点视觉：文件夹图标 + 扩展名 badge（复用网格 badge 的判定逻辑）。

**内容区与导航**

- SPA：树内导航 fetch 现有 `?fragment`（目录迷你列表/文档/代码/媒体片段）注入内容区 +
  `history.pushState`/`popstate`；内容区注入用 DOM API。
- 单一活跃导航不变量：每次导航 abort 前次 fetch + 单调 request id；仅当 id 与当前 canonical
  URL 均为最新时提交 DOM/history/TOC observer；替换时拆除旧 observer、卸载媒体元素。
- 深链目录页在树模式下先渲染完整页再换成 fragment 视图（同 URL 一次 XHR，可接受）。
- 树模式无预览面板、无 native-open（fragment 不携带 token，四道门不传播）。

**TOC 侧栏**

- 既有宽屏（≥ 75rem）fixed rail 从左侧移到右侧（`right: 0`；`margin-right` 替代 `margin-left`），
  作用于完整文档页且存在 `#TOC` 时；窄屏保持顶部卡片；无 TOC 不生效。
- scroll-spy：IntersectionObserver 高亮当前标题（完整页 root = 视口，树模式内容区 root = pane）。
- fragment 预览面板不启用 rail/spy。

**安全与样式**

- 所有 CB 完整页统一经可信 UI 出口携带 `frame-ancestors 'none'` + `X-Frame-Options: DENY`；
  fragment/JSON/静态资源不携带 token、不复用该头（契约不变）。
- 所有树宿主（`page_shell` 与 `doc_page`）预加载 `katex.min.css`；fragment 本身不含资源标签
  （契约不变）。顺带修复目录页预览面板既有 KaTeX 无样式缺口。
- JSON→DOM 渲染：`createElement` + `textContent`（外层 `<bdi dir="auto">`）；禁止把文件名
  插值进 `innerHTML`/HTML 模板。

## Testing Decisions

- 好测试标准：只测外部可观测行为（HTTP 状态/头/JSON 形状/渲染输出契约），不测实现细节；
  与既有测试同风格（`tests/router.rs` 的 HTTP 级断言 + 模块内单元测试）。
- **Seam A（cargo test，提交级自动化）**：tree 端点契约（200/403/404、JSON 字段、非 UTF-8
  `href: null`、环检测：symlink-to-root 子目录 `expandable: false` 且仍在 entries、非环 symlink
  子目录 `expandable: true`）；`.md`/`.org`/Office/代码四类完整页响应的反嵌入头；`page_shell`
  与 `doc_page` 均含 `katex.min.css` link；既有 fragment/排序/路径穿越/状态端点测试保持通过。
  Prior art：`tests/router.rs`、`render.rs`/`routes.rs` 模块测试。
- **Seam B（/implement 验收证据，不提交测试文件）**：真实 Chromium 请求拦截——
  delayed-A/fast-B 导航竞态 + popstate（B 最终可见且 URL 一致）；DOM 注入（`<img src=x onerror=…>`、
  引号、bidi/control、字面 `+`/`%` 文件名——无元素/脚本注入，可见转义与 action href 与目录模式
  一致）；TOC 右侧 rail 布局 + scroll-spy；窄屏抽屉；KaTeX 样式生效 + woff2 字体请求。
- 测试契约不变：`cargo test` 仍是完整恒跑套件，零新依赖，无独立 JS 测试命令。

## Out of Scope

- 树模式 native-open（目录模式保留；将来可单独立项）。
- 搜索页改造（保持独立全屏）。
- 目录模式本身的行为/视觉改动（表格、排序、预览面板、键盘契约原样）。
- 树节点缩略图、文件大小/修改时间列、多选/批量操作。
- 服务端渲染树壳 / cookie 区分模式（ADR 0001 已否决）。
- 跨 tab 实时同步树状态。
- TOC 折叠/展开交互与深度配置变更（沿用现有 `toc:N`）。

## Further Notes

- ADR 0001 记录架构决策；CONTEXT.md 术语：树模式 / 目录模式 / 内容区 / TOC 侧栏 / 可信 UI。
- 深链目录页在树模式有短暂完整页闪现（一次同 URL XHR 后替换，可接受）。
- 反嵌入头扩展顺带修补既有缺口（文档/代码页此前无这两个头）。
