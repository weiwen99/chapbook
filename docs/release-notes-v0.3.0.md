# chapbook v0.3.0 — 文件树阅读模式

chapbook 从「列表式文件浏览器」迈向「阅读优先的树形浏览器」：新增懒加载文件树 + 内容面板的
树阅读模式，文档页 TOC 变为真正的兄弟列，Office 转换与搜索共用有界并发门，PDF 交给浏览器
原生查看器。渲染管线与零运行时外部依赖的决策不变。

## 新功能

- **文件树阅读模式** — 页头一键切换列表/树（偏好 localStorage 记忆）。树按需懒加载：新增
  `GET /__/tree` 与 `GET /__/tree/{*path}` JSON 端点，请求路径逐分量复用 `resolve_within_root`
  （与文件服务同一防线），目录展开带服务端 `expandable` 标记，环（祖先 identity）与非 UTF-8
  目录不可展开（fail-closed）。首次点击即打开文件，内容区 SPA 导航不整页刷新，深链接 hash
  规范化；↑/↓ 移动选中，→ 展开，← 收起/回父级，`Enter` 打开；ARIA tree 语义（单 tab 停靠 +
  `aria-activedescendant` 公告）。
- **可调宽侧栏 + 持久化展开状态** — 树宽 160–480px 拖拽调整（键盘 ±16/Home/End 步进），
  偏好、展开路径与选中项分别持久化（`cb-tree-width` / `cb-tree-expanded` / `cb-tree-selected`）；
  导航时自动展开并高亮当前路径；宽屏可整体收起（左缘恢复按钮），窄屏（<64rem）自动变抽屉
  （遮罩 + 唤起按钮）。
- **TOC 滚动跟随** — 文档阅读时 TOC 高亮当前章节（scroll-spy），滚动与展开状态随导航同步。
- **文档页 TOC 独立列** — 宽屏文档页改为 `正文 | TOC` 兄弟列布局（TOC 不再悬浮覆盖正文），
  窄屏变为 TOC 在正文之前；正文限宽防长串破版；完整页与 SPA 面板共用同一布局装配。
- **Office 转换有界并发门** — 转换走容量 1 的 `spawn_blocking` + Semaphore：忙即失败不排队，
  完整页返回 503 + `Retry-After: 1`，fragment 返回 200 安全占位（绝不回退裸字节）；与搜索门
  合并为通用 `run_blocking_job` seam。
- **PDF 浏览器原生查看** — 预览面板、文件树与迷你列表点击 PDF 一律 `window.open` 新 tab 交给
  浏览器内置查看器（支持 Range seek）；服务端对 PDF 响应省略 sandbox 并强制 `nosniff`（见安全）。
- **可信 UI 防框架全覆盖** — `frame-ancestors 'none'` + `X-Frame-Options: DENY` 从目录/搜索页
  扩展到全部渲染完整页（文档/代码/Office）；目录页 head 预加载 KaTeX。

## 安全

- **PDF sandbox 例外收窄** — 仅非 download 的 200/206 `application/pdf`（精确匹配）省略
  sandbox：Chromium 内置 PDF 查看器拒绝渲染 document-sandbox 的文档；download 与非 200/206
  响应永不例外，并强制 `X-Content-Type-Options: nosniff`。
- **防框架策略覆盖所有完整页** — 此前仅目录页与搜索页带 anti-framing header，现扩展至全部
  CB 渲染的完整页；策略保持 HTTP header 实现，不降级为 meta。
- **树端点安全** — 复用逐分量 `resolve_within_root` 防线（越界 403，不存在/非目录 404）；
  祖先 identity 去环 fail-closed；全部文件系统操作在单个 `spawn_blocking` job 内执行。
- **树模式禁用 native-open**（Q17）：树内无本机打开动作，native 动作一律 no-op。

## 变更与兼容性

- 新端点 `GET /__/tree` / `GET /__/tree/{*path}`（JSON，树模式专用，与搜索同构的
  `spawn_blocking` 并发门语义）。
- 无新增运行时依赖；CLI 无变化；二进制仍为单文件内嵌全部前端资源。
- 树模式交互契约、持久化键与安全决策见 [AGENTS.md](AGENTS.md)；设计决策见
  [docs/adr/0001-tree-mode-client-shell.md](docs/adr/0001-tree-mode-client-shell.md) 与
  [docs/superpowers/specs/2026-08-26-tree-reading-layout-design.md](docs/superpowers/specs/2026-08-26-tree-reading-layout-design.md)。
- 行为契约与安全不变量全文见 [AGENTS.md](AGENTS.md)。

## 测试

- 235 项全绿：150 单元 + 83 路由集成 + 2 排序解析（较 v0.2.0 新增 37 项，含树端点环检测、
  Office 并发门、fragment 契约、非 UTF-8 路径省略 expandable）；clippy 零警告基线，
  `cargo fmt --check` 通过。
- Chromium 浏览器验收脚本：树模式键盘与异步竞态（`tests/browser_tree_async_acceptance.mjs`）、
  模式切换工具栏（`tests/browser_mode_toggle_toolbar_acceptance.mjs`）、PDF 原生打开
  （`tests/browser_pdf_native_open_acceptance.mjs`）。
- macOS（APFS 以 EILSEQ 拒绝非法 UTF-8 文件名）相关测试自动跳过，其余平台全量运行。
