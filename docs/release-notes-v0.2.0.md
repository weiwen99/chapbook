# chapbook v0.2.0 — 只读文件浏览器增强

chapbook 从「渲染能力强的静态文件服务器」迈向「基于浏览器的本地文件浏览器」：
目录浏览的交互与安全模型全面升级，渲染管线与零运行时外部依赖的决策不变。

## 新功能

- **递归文件名搜索** — 页头常驻搜索框，`GET /__/search?q=` 服务端递归遍历（不建索引），
  大小写不敏感、目录也参与匹配，确定性排序 BFS（按原始文件名 bytes），上限 500 条；
  进程级并发门：扫描进行中重复请求返回 429 + `Retry-After: 1`。
- **面包屑 + 键盘导航** — 逐级面包屑（RFC 3986 编码、特殊字符安全）；`j/k/↑/↓` 移动选中行，
  `Enter` 当前 tab 打开，`o` 新 tab，`p` 预览面板，`v` 列表/网格切换，`Backspace` 上级目录，
  `/` 聚焦搜索框，`Esc` 关闭面板。禁用 JS 时所有基础功能仍可用（渐进增强）。
- **侧边预览面板** — 点击行尾图标即可在面板内预览 .md/.org/Office/CSV/代码/图片/音视频，
  不离开列表。fragment 为严格接口：任何路径都只返回 CB 生成的安全 HTML 片段，
  失败路径返回占位提示，绝不回退原始字节。
- **列表/网格视图** — 网格视图图片 lazy 缩略图（客户端 CSS 缩放，零新依赖），
  视图偏好 localStorage 持久化。
- **音视频浏览器内播放** — 原生 `<audio>/<video>` + Range 请求（206），支持拖动 seek。
- **显式下载** — `?download=1` 一律返回原始字节 + `Content-Disposition: attachment`，
  文件名按 RFC 8187 编码（`filename*`），中文/空格/`()*` 等文件名正确。
- **本机默认应用打开** — 目录页与预览面板的「本机打开」经 xdg-open（Linux）/ open（macOS）
  用系统默认应用打开文件；仅 loopback 场景可用（见安全）。
- **`.txt` / `.log` 渲染** — 加入代码渲染管线：Accept 协商返回带行号的 plaintext 高亮页或原文。
- **响应模式统一解析** — `download > raw > fragment > view` 固定优先级；`.md`/`.org` 新增
  `?raw=1` 原文支持。

## 安全

- **原始内容 origin 隔离** — 所有 ServeFile 响应统一带 `CSP: sandbox allow-scripts`
  （严禁 `allow-same-origin`）：顶层 HTML/SVG 获得 opaque origin，无法读取 token-bearing 页面；
  媒体例外仅限音视频响应（Chromium 无法在 document sandbox 中加载媒体），并强制 `nosniff`。
- **可信 UI 防点击劫持** — 目录页与搜索页统一带 `CSP: frame-ancestors 'none'` +
  `X-Frame-Options: DENY`（HTTP header，非 meta）。
- **native-open 四重门** — peer loopback → 字面 loopback Host → 精确 `http://Host` Origin →
  随机 token（启动时 `/dev/urandom` 生成，失败即拒绝启动），任一失败返回 403；
  token 与错误 token 不写入日志；路径经既有的逐分量 `resolve_within_root` 校验。
- **Org 主动内容封堵** — HTML ExportBlock/Snippet 强制文本转义；Org link 拒绝
  `javascript:`/`vbscript:`/`data:`/`file:` 危险 scheme（混合大小写与 ASCII 空白变体均拦截），
  完整页与 fragment 共用同一实现。
- **Unicode 文件名安全显示** — 显示 label 与文件 identity 严格分离：全部 Unicode 17.0
  Default_Ignorable、Bidi_Control 与异常 whitespace 显示为可见大写 escape（如
  `photo\u{202E}gnp.command`），首尾/连续空格显示为 `\x20`，非 UTF-8 路径仅显示不可操作；
  浏览器 action 只经 RFC 3986 逐 segment 编码的 ASCII-only `data-native-path-encoded` 传输。

## 变更与兼容性

- **`--host` 默认值 `0.0.0.0` → `127.0.0.1`**：本机为主的使用场景下默认不暴露局域网；
  局域网分享需显式 `--host 0.0.0.0`。
- **库接口**：`routes::app(root) -> io::Result<Router>`（token 生成失败显式报错）；
  测试装配与 opener trait 全部私有。
- 无新增运行时依赖；二进制仍为单文件内嵌全部前端资源与语法集。
- 行为契约与安全不变量全文见 [AGENTS.md](AGENTS.md)，设计过程见
  [docs/2026-08-12-design-readonly-file-browser.org](docs/2026-08-12-design-readonly-file-browser.org)。

## 测试

- 198 项测试全绿：124 单元（含 search gate、native-open 校验链、token 读取、
  opener reaper、Unicode 17.0 matcher）+ 72 路由集成 + 2 排序解析；
  clippy 零警告基线，`cargo fmt --check` 通过。
