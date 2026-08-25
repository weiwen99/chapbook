# 08: Scroll-spy 当前标题高亮

**What to build:** TOC 当前条目高亮：完整文档页以视口为观察根，树模式内容区以 pane 为观察根；SPA 导航替换时拆除旧 observer，高亮只属于当前文档。

**Blocked by:** 03, 05, 06

**Status:** ready-for-agent

- [ ] 完整文档页滚动：TOC 对应条目高亮跟随当前标题（视口根）
- [ ] 树模式内容区滚动：pane 内滚动高亮 TOC（pane 根）
- [ ] 快速 SPA 导航后 observer 不串台：旧 observer 拆除，高亮只作用于当前文档
- [ ] 窄屏（无 rail）与无 TOC 文档不启动 spy
- [ ] Chromium 证据：两种模式滚动高亮 + 导航后正确性
- [ ] 既有全部测试通过；`cargo fmt --check` 与 clippy 零警告

## Comments

2026-08-25 scope expansion (DesignGate): ticket 03's right rail applies only to full pages (.cb-doc-page #TOC); injected .cb-pane fragments still render #TOC as a top card. Ticket 08 MUST also create a pane-local wide-screen right rail (sticky within the pane scroll container, grid/flex with the fragment content; narrow screens keep the top card) in addition to the IntersectionObserver scroll-spy (pane as root). Both pane rail and spy teardown go through registerPaneDisposable.

## Comments (2026-08-25 review fixes)

Two-axis review fixes applied: full-page spy disconnected on entering tree mode (no detached-target observation); pane spy disposer registered via registerPaneDisposable (exactly-one-observer invariant); pane reading line computed in pane-relative coordinates; scroll-end listener guarantees last-heading highlight without IO transition; spies not installed below 75rem (ticket contract); tree mode neutralizes the full-page fixed rail (body margin + inherited fixed-rail props) so the pane rail is isolated; .cb-pane-error styles restored (accidentally deleted). Pane rail mechanism finalized as float + sticky within the pane scroll container (verified in Chromium through full scroll; the earlier "grid/flex" wording is superseded — float+sticky preserves document flow without restructuring).
