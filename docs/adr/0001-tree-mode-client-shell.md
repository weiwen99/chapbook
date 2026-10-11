# 树视图采用客户端外壳，服务器不感知浏览布局

**Status**: accepted

## Context

用户对现有 UI 不满，要求编辑器式树状文件浏览（与现有目录模式可切换），并把文档 TOC 移到内容右侧。
2026-08-25 三轮 grilling（Q1–Q21）确认全部设计决策，本 ADR 记录其中结构性的部分。

## Decision

树视图是 `chapbook-browser.js` 在服务器完整页之上搭的客户端外壳：左侧懒加载目录树
（`GET /__/tree/{*path}` JSON 端点，逐分量复用 `resolve_within_root`）+ 右侧内容区
（SPA：fetch `?fragment` + `history.pushState`/`popstate`）。树、列表、网格是三个同级浏览视图，
默认列表；无 JS 时只有服务端列表。布局偏好与树展开集存 localStorage，历史项单独保存
浏览上下文、布局与显示规则，恢复不覆盖偏好。服务器接受共享隐藏/排序选项、输出页面语义与
规范编码 identity，但不感知布局：三种视图深链收到同一完整响应，不引入布局 query 或 cookie。
文档页（含代码/Office 渲染页）仍是独立阅读表面，并作为可信 UI，与目录/搜索页
一样经 `trusted_ui_response` 携带 `frame-ancestors 'none'` + `X-Frame-Options: DENY`。TOC 侧栏
从既有宽屏左侧 fixed rail 搬到右侧，加 IntersectionObserver 滚动高亮（完整页以视口为 root，
树模式内容区以 pane 为 root）；窄屏回退顶部卡片。

## Considered Options

- **服务端渲染树壳**（cookie 区分模式）：否决——两套树渲染、为模式引入 cookie、无 JS 契约复杂化。
- **`?path=` query 参数**：否决——query 解码是表单语义（`+`→空格），与仓库的 RFC 3986 segment
  identity 不变量冲突，且是第二个文件系统路径信任边界；改用通配路由复用 `resolve_within_root`。
- **树视图保留 native-open**：否决——fragment 不携带 token（安全不变量），不能在客户端
  树壳中转移该能力；native-open 仅完整可信目录/搜索页的列表、网格提供。
- **TOC 左侧 rail**：既有实现位置；用户要求右侧，直接搬移。

## Consequences

- 树视图无 native-open；切回列表/网格通过同路径或同搜索上下文的完整可信页恢复 token，
  保留 q/sort/hidden，布局切换不多增历史项。
- doc 页开始加载 `chapbook-browser.js`（defer）；所有 CB 完整页（目录/搜索/.md/.org/Office/代码）
  带反嵌入头，需实测四类直链响应。
- 深链目录页在树模式下先渲染完整页再换成 fragment 视图（同 URL 一次 XHR，可接受）。
- 二进制体积基本不变：JS/CSS 本就内嵌，新增一个只读 JSON 端点。
