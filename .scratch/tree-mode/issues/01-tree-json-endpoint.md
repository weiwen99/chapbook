# 01: 树 JSON 端点（含环检测）

**What to build:** 只读 JSON 端点 `GET /__/tree/{*path}`（含显式根路由），按需返回目录子项，供树模式懒加载。越界 403、不存在/非目录 404；目录项带服务端环检测标志。curl 即可演示完整契约。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 端点契约：200 `application/json`，entries 含 `name`（显示 codec 纯文本）、`href`（root-relative RFC 3986 编码，或 null）、`dir`、`expandable`（仅目录携带）
- [ ] 路径安全：请求路径与文件服务共用同一 root 边界解析（`resolve_within_root`），越界 403；不存在/非目录 404 + 小 JSON 错误体
- [ ] 非 UTF-8 segment：`href: null`；目录 `expandable: false`、文件省略 `expandable` 字段；条目仍出现在 entries（display-only）
- [ ] 环检测：symlink-to-root 子目录 `expandable: false` 且仍显示；非环 symlink 子目录 `expandable: true`；identity 判定复用搜索遍历同一 seam（Unix `(dev, ino)` / 非 Unix canonical path），必要时提升该 seam 可见性，客户端不做第二套判定
- [ ] JSON 经 `axum::Json` + serde 强类型 `Serialize` 输出；Cargo.toml 显式声明 serde/serde_json 直接依赖；AGENTS.md 依赖表补两行
- [ ] release 构建体积实测上报（对比改动前）
- [ ] Seam A：HTTP 级测试覆盖上述契约（仿 tests/router 风格）；`cargo fmt --check` 与 clippy 零警告；既有全部测试保持通过

## Comments

2026-08-25 implemented: commits 5e782f6 (test guards), 93b3d5e (endpoint). Release size 11874912 -> 11924512 B (+49600 B). Full suite green (204 tests). Non-UTF-8 fixture tests run only on non-macOS filesystems (APFS rejects invalid UTF-8, EILSEQ) — pre-existing tests gained the same guard.
