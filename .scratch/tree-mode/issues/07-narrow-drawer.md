# 07: 窄屏抽屉

**What to build:** 窄视口（<64rem）树模式：树变为 off-canvas 抽屉（CSS transform + 遮罩），按钮开合、选择条目后收起、Esc 关闭。零依赖实现，两种壳共用。

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] 窄视口（<64rem）：树隐藏，按钮唤起抽屉 + 遮罩；点遮罩/Esc/选择条目后收起
- [ ] 宽视口（≥64rem）：树常驻左侧，无抽屉行为
- [ ] Chromium 窄视口证据（截图 + 交互记录）
- [ ] 既有全部测试通过；`cargo fmt --check` 与 clippy 零警告

## Comments (2026-08-25 complete)

Implemented commits 12a2e3b + 08064d1 (review fixes: drawer focus gating on open state, range breakpoint, aria-expanded). Chromium acceptance 10 scenarios PASS at 1400x900 and 500x800. Ticket complete.
