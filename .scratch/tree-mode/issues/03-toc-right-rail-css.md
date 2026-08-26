# 03: TOC 侧栏右侧化（纯 CSS）

**What to build:** 宽屏（≥75rem）完整文档页的 TOC fixed rail 从左侧移到内容右侧；窄屏回退既有顶部卡片；无 TOC 页面不占位。纯 CSS，目录模式文档页立即受益。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 宽视口文档页：TOC 固定于内容右侧、正文不被遮挡（Chromium 截图证据）
- [ ] 窄视口：TOC 回退顶部卡片（既有形态）
- [ ] 无 TOC 页面（如代码页）不产生 rail 占位
- [ ] 目录页预览面板（fragment 嵌入）布局不受影响
- [ ] 既有全部测试通过；`cargo fmt --check` 与 clippy 零警告

## Comments

2026-08-25 implemented: commit bcce59a. Evidence: real Chromium harness (wide viewport — TOC fixed right:0, content margin-right, no overlap; narrow — static top card; no-TOC — full width) + end-to-end curl (doc page serves TOC + katex link). Reviewed (two-axis, no findings against ticket).
