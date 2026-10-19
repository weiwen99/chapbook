# chapbook v0.4.2 — HTML 预览与源码视图

`.html` / `.htm` 文件现在可以在不离开浏览界面的情况下查看：预览面板与树内容区以内嵌
sandboxed iframe 显示浏览器原生渲染结果，并可在页面与高亮源码之间切换。raw 出口、完整页
与下载仍是原文件；既有安全策略、路径解析与编码规则不变。相对 v0.4.1，CLI、Range 请求与
渲染依赖不变，仍是单文件可执行、零运行时外部依赖。

## HTML 预览

- **预览面板与树内容区** — 列表/网格的“预览”与树节点打开 `.html`/`.htm`（ASCII 大小写
  不敏感）时，fragment 返回 `.cb-doc.cb-html-preview`，内嵌
  `<iframe sandbox="allow-scripts" src="exact raw URL">`；页面自身的 CSS、脚本、相对资源与
  布局按浏览器原生方式工作。
- **双层隔离** — raw 响应继续带 `CSP: sandbox allow-scripts`；iframe 元素再带同值
  `sandbox`，跨导航持续生效，两层都不含 `allow-same-origin`。页面仍不能读取站点
  fetch/storage，不能提交表单或弹窗。
- **原文出口不变** — 直接访问、`?view=1`、`?raw=1`、`?download=1` 仍返回 ServeFile
  原文件；新标签始终打开渲染好的页面。

## HTML 源码视图

- **页面 / 源码切换** — 预览面板工具栏与树内容区工具栏在 HTML 文件上显示 `源码` /
  `页面` 按钮。源码视图复用代码管线：syntect 高亮、行号、严格 UTF-8 与 1 MiB 上限；
  超限或非 UTF-8 返回占位片段，不回退裸字节。
- **显式 URL** — `?fragment=1&source=1` 直接返回源码 fragment；`source` 只是 fragment
  修饰符，单独出现或用于非 HTML 时忽略。
- **瞬态视图** — 树内容区切换不写 URL/历史快照；换文件或后退回到页面。侧边面板关闭/
  换文件也回到页面。

## 树视图新标签

- 树节点 ⌘/Ctrl/Shift+点击与中键在新 tab 打开完整页；普通点击仍走内容区导航。列表/
  网格原有的行尾 `↗` 与文件名修饰键点击保持不变。

## 兼容与升级

- **接口** — 新增可选的 `source=1`（仅 fragment 模式、仅 `.html`/`.htm`）；`download >
  raw > fragment > view` 优先级、`sort`/`hidden` 语法与非法值 404 均不变。
- **安全** — 原始内容 sandbox、PDF/音视频兼容例外、可信页面反嵌入头与 native-open
  能力边界不变；HTML iframe 只允许 scripts。
- **升级提醒** — 前端资源内嵌在二进制中，需要替换为新二进制并重启原有 chapbook 进程；
  无需数据迁移。
- **发布平台** — Linux x86_64（`x86_64-unknown-linux-musl`，musl 静态链接）与 macOS
  Apple Silicon（`aarch64-apple-darwin`）。其他平台可从源码构建。

## 发布前验证

| 验证项 | 本地结果 |
| --- | --- |
| `cargo test` | 122 单测 + 100 路由测试 + 4 排序测试通过 |
| `cargo fmt --check`、`cargo clippy --all-targets` | 通过 |
| `cargo build --release --locked` | 通过 |
| Chromium HTML 预览 / 页面 ↔ 源码切换 | 通过 |
| Chromium 树内容区源码切换、长源码滚动与换文件重置 | 通过 |
| Chromium 树节点修饰键/中键新标签 | 通过 |

二进制 `--version` 应返回 `chapbook 0.4.2`。
