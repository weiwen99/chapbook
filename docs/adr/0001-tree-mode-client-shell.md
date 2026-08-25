# 树模式采用客户端外壳，服务器 HTML 不变

**Status**: accepted

## Context

用户对现有 UI 不满，要求编辑器式树状文件浏览（与现有目录模式可切换），并把文档 TOC 移到内容右侧。
2026-08-25 三轮 grilling（Q1–Q21）确认全部设计决策，本 ADR 记录其中结构性的部分。

## Decision

树模式是 `chapbook-browser.js` 在**服务器 HTML 不变**之上搭的客户端外壳：左侧懒加载目录树
（新增 `GET /__/tree/{*path}` JSON 端点，逐分量复用 `resolve_within_root`）+ 右侧内容区
（SPA：fetch 现有 `?fragment` + `history.pushState`/`popstate`）。模式偏好、树展开集与选中路径
存 localStorage，默认目录模式；无 JS 时只有目录模式（渐进增强契约保持）。服务器因此不感知模式：
目录模式与树模式深链收到同一响应。文档页（含代码/Office 渲染页）由此升级为可信 UI，与目录/搜索页
一样经 `trusted_ui_response` 携带 `frame-ancestors 'none'` + `X-Frame-Options: DENY`。TOC 侧栏
从既有宽屏左侧 fixed rail 搬到右侧，加 IntersectionObserver 滚动高亮（完整页以视口为 root，
树模式内容区以 pane 为 root）；窄屏回退顶部卡片。

## Considered Options

- **服务端渲染树壳**（cookie 区分模式）：否决——两套树渲染、为模式引入 cookie、无 JS 契约复杂化。
- **`?path=` query 参数**：否决——query 解码是表单语义（`+`→空格），与仓库的 RFC 3986 segment
  identity 不变量冲突，且是第二个文件系统路径信任边界；改用通配路由复用 `resolve_within_root`。
- **树模式保留 native-open**：否决——fragment 不携带 token（安全不变量），传播
  peer/Host/Origin/token 四道门代价不成比例；native-open 仍是目录模式专属。
- **TOC 左侧 rail**：既有实现位置；用户要求右侧，直接搬移。

## Consequences

- 树模式无 native-open（文件只在浏览器内预览；低频功能，目录模式仍在）。
- doc 页开始加载 `chapbook-browser.js`（defer）；所有 CB 完整页（目录/搜索/.md/.org/Office/代码）
  带反嵌入头，需实测四类直链响应。
- 深链目录页在树模式下先渲染完整页再换成 fragment 视图（同 URL 一次 XHR，可接受）。
- 二进制体积基本不变：JS/CSS 本就内嵌，新增一个只读 JSON 端点。
