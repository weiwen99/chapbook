# chapbook v0.4.3 — 未知扩展名纯文本预览

未知扩展名的纯文本文件（`.env`、`LICENSE`、`.gitignore`、无扩展名配置等）现在可以在预览面板
与浏览器完整页中按纯文本阅读。判定按内容：整文件严格 UTF-8、无 NUL、≤1 MiB、非 PDF。
raw/download、路径解析与既有安全契约不变。

## 纯文本预览

- **内容判定** — 显式类型（md/org/HTML/Office/代码/媒体）仍然优先；未知扩展名在 fragment
  或完整页 `?view=1` / `Accept: text/html` 时读取内容，整文件严格 UTF-8 且无 NUL 才以
  `Plain Text` 渲染（带行号、HTML 转义）。判定与渲染共用一次读取。
- **dotenv** — `.env` 扩展名与 `.env` / `.env.*` 文件名加入纯文本映射，`xyz.env`、
  `.env.local`、`.env.example` 等可直接预览。
- **边界** — 仅 UTF-8、≤1 MiB；非 UTF-8、含 NUL、超限与 `.pdf` 维持占位/原字节。
  PDF 继续由浏览器原生查看器打开；非 UTF-8 纯文本的编码判定不在本版范围。
- **原始出口不变** — raw/download 不嗅探，仍返回原字节；完整页非 HTML Accept 仍由
  `ServeFile` 决定 Content-Type。

## 兼容与升级

- **接口** — 无新增参数；此前显示“不支持预览此文件类型”的未知扩展名文本现在可预览。
  `download > raw > fragment > view` 优先级、排序/隐藏语法、路径与安全头均不变。
- **性能** — 未知类型请求多一次 ≤1 MiB 读；HEAD 与 GET 同样协商以保持头部一致。
- **升级提醒** — 前端资源内嵌在二进制中，需要替换为新二进制并重启原有 chapbook 进程；
  无需数据迁移。
- **发布平台** — Linux x86_64（`x86_64-unknown-linux-musl`，musl 静态链接）与 macOS
  Apple Silicon（`aarch64-apple-darwin`）。其他平台可从源码构建。

## 发布前验证

| 验证项 | 本地结果 |
| --- | --- |
| `cargo test` | 124 单测 + 103 路由测试 + 4 排序测试通过（231） |
| `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo build --release --locked` | 通过 |
| 实际 HTTP：fragment / 完整页 / raw / download / HEAD | 通过 |
| `.env`/`LICENSE`/`.gitignore` 预览；二进制/PDF/超限占位；`<script>` 转义 | 通过 |

二进制 `--version` 应返回 `chapbook 0.4.3`。
