# 04: Office 转换 spawn_blocking 边界

**What to build:** anydoc 转换（fragment 与完整页两条路径）移出 async handler：有界并发 `spawn_blocking`，permit 生命周期归阻塞 job 所有；busy 行为定义——fragment 走安全占位 HTML（200，绝不裸字节），完整页 503 + Retry-After。模式仿既有搜索并发门。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 转换不再于 async handler 内同步执行；并发容量有界（容量值自定并记录理由）
- [ ] permit 随 job 移入阻塞任务：handler 被取消（客户端断开）不提前释放容量（测试锁定该语义）
- [ ] fragment 转换忙 → 200 安全占位 HTML（绝不回退 ServeFile/裸字节）；完整页转换忙 → 503 + `Retry-After: 1`
- [ ] 空闲路径行为不变：转换成功/失败/超限的既有响应契约保持（既有测试锁定）
- [ ] 既有全部测试通过；`cargo fmt --check` 与 clippy 零警告

## Comments

2026-08-25 implemented: commits 9600b72 + seam merge follow-up. Deterministic private gate tests (preheld-permit busy mapping, cancellation permit lifetime, panic recovery, fragment/full-page recovery); FIFO/mkfifo HTTP tests rejected per zero-external-dependency test contract. Fragment busy -> 200 placeholder, full page busy -> 503 + Retry-After: 1. Two-axis review findings (capacity rationale, seam merge) fixed.
