# 未知扩展名按内容兜底为纯文本预览

**Status**: accepted

## Context

扩展名白名单必然漏：`.env`、`LICENSE`、`.gitignore` 与无扩展名配置曾显示“无法预览”。
2026-10-20 grilling 确认目标是“任意 UTF-8 文本都能预览”，同时不改变显式类型的优先级与原始出口契约。

## Decision

显式类型（md/org/HTML/Office/代码/媒体）仍然优先；未知扩展名在 fragment 与完整页 HTML 协商时
按内容兜底：普通文件、≤1 MiB、非 `.pdf`、整文件严格 UTF-8 且不含 NUL，则以 `Plain Text` 阅读视图
渲染（带行号、HTML 转义）。判定与渲染共用一次读取。raw/download 不嗅探，Content-Type 保持
`ServeFile` 现状。完整页仅在 `?view=1` 或 `Accept` 含 `text/html` 时渲染，否则返回原字节。
条目类型、搜索与树 JSON 仍按扩展名，不探测内容。

## Considered Options

- **继续扩白名单**：否决——打地鼠，`LICENSE` 与无扩展名文件无法覆盖。
- **取消白名单、全量嗅探**：否决——会推翻 md/org/Office/媒体/HTML 的既有分派与安全边界。
- **raw/download 也按内容改 `text/plain`**：否决——为下载增加读盘，改变 Range 与原字节契约，独立议题。

## Consequences

- 未知类型请求多一次 ≤1 MiB 读；axum 的 GET 路由也承载 HEAD，HEAD 同样付出该成本以保持头部一致。
- 整文件合法 UTF-8 的二进制会被当作文本（只读、转义、无执行风险）；`.pdf` 显式排除，
  避免 ASCII-only PDF 落入文本兜底而破坏浏览器原生查看器契约。
- 超过 1 MiB、非 UTF-8 或含 NUL 的文件维持原状：fragment 占位，完整页原字节。
- 协商响应未加 `Vary: Accept`，与既有代码/Office 协商一致，记录为已知约束。
