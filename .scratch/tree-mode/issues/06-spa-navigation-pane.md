# 06: SPA 导航与内容区

**What to build:** 树内导航改为 SPA：fetch 现有 fragment 接口注入内容区 + `pushState`/`popstate`；单一活跃导航不变量（abort 前次请求 + 单调 request id，仅最新提交）；替换时拆除旧 observer、卸载媒体；深链目录显示迷你列表；树模式无预览面板。

**Blocked by:** 04, 05

**Status:** ready-for-agent

- [ ] 树中打开文件/目录：内容区注入对应 fragment，无整页刷新，URL 经 `pushState` 同步更新
- [ ] back/forward（popstate）正确恢复内容与 URL；深链目录 URL 内容区显示迷你列表
- [ ] 单一活跃导航：delayed-A/fast-B 确定性浏览器测试（请求拦截延迟 A 响应）——B 最终可见且 URL 一致，含 popstate 之后
- [ ] 导航替换时：旧 TOC observer 拆除、旧媒体元素卸载（音视频停止播放）
- [ ] 目录 → 数学文档 SPA 导航：KaTeX 有样式且 woff2 字体请求发生（请求拦截证据）
- [ ] Office 转换忙时内容区显示占位片段（04 行为贯通到 SPA 路径）
- [ ] 树模式无预览面板；目录模式预览面板行为原样
- [ ] 既有全部测试通过；`cargo fmt --check` 与 clippy 零警告

## Comments (2026-08-25 complete)

Implemented commits 6bc1050 (implementation) + 06fee87/d74c343 (review fixes). Chromium acceptance 9/9 + 1 probe PASS; deterministic reruns: delayed-A/fast-B with AbortSignal-ignoring mock (generation guard alone discards late A) PASS, office-busy placeholder via pane path PASS (explicit, non-optional). Two-axis review (full range 9a34965..6bc1050) findings fixed. Ticket complete.
