# 浏览视图控件与显示规则：设计 review

Review: completed; findings resolved in planning documents
Implementation: not started
Product decisions: Q1–Q27 unchanged
Open blockers: none

## 范围与结论

审查 [spec](spec.md) 与 [01](issues/01-shared-listing-policy.md)、[02](issues/02-listing-query-and-fragments.md)、[03](issues/03-unified-browser-toolbar.md)、[04](issues/04-shared-browser-state-and-navigation.md)、[05](issues/05-acceptance-and-documentation.md)，对照当前 Rust 路由/渲染/条目规则、浏览器状态代码和既有验收脚本。

两份独立只读 review 分别检查状态/生命周期与服务端 Interface/安全出口；整合后共五项 P2 工程问题。已在 spec 与对应 issue 修正，不改变已确认产品取舍，不新增跨 tab 实时同步、自动重试或运行时依赖。spec 与五份 issue 保持 ready-for-agent，实施依赖不变。

以下风险场景是尚未实施方案的推演，使用 `[INFERENCE]` 标注；代码事实与运行时观察单独列出。已分配但尚未实施的新功能不重复列为设计缺陷。

## R1：焦点回退的“不改 URL”与控件 query 更新冲突

**问题与证据**：审查前 Q17 和隐藏焦点验收写“不改 URL”，Q24/Q26 又要求显示规则写入 query 并更新当前历史项；04 已使用较窄的“不改路径 URL”。这三处表述不能按字面同时满足。

**修正**：回退本身不导航、不打开回退项，不改变 pathname、q 或文档 hash；隐藏控件仍更新 hidden query，排序规则保持。更新 spec Q17/验收矩阵及 04 的要求与验收。

**验收落点**：隐藏选中项后内容不变，query 反映新规则，焦点有效；重新显示恢复展开但不抢焦点。由 04/05 落实。

## R2：搜索列表/网格的本机打开能力被错误排除

**代码证据**：`src/routes.rs:285–310` 向合法完整搜索页传 token；`src/render.rs:304–318,352,385–386` 为搜索结果生成本机打开控制。审查前 04 将 native-open 限为目录列表/网格，spec 的恢复出口也只提完整目录。

**风险**：`[INFERENCE]` 按该限定实施会移除搜索页已有能力；搜索树退出时若导航到目录，会丢失 q，违反搜索跨视图保持。

**修正**：完整目录与完整搜索页的列表/网格均保留合法 token 下的既有能力。恢复搜索能力加载当前 `/__/search` 完整页并保留 q/sort/hidden，不能转到目录。树/fragment 仍没有 native-open；token 不进入历史快照或持久存储。

**验收落点**：02 保留服务端完整搜索页能力，04 负责恢复与定位，05 覆盖搜索树→列表/网格及四道门回归。

## R3：过滤产生的空分支会丢失待恢复展开意图

**代码证据**：`assets/chapbook-browser.js:991–1023` 将空结果永久叶子化，删除持久化展开路径和展开意图；`1130–1131` 随后阻止再次展开。原 Interface 没有区分“当前规则无可见孩子”与结构性不可展开。

**风险**：`[INFERENCE]` 已展开的普通目录若仅含隐藏孩子，关闭隐藏显示后返回空数组；原逻辑会删除展开意图，即使再次显示并使缓存失效，也无法按 Q17 恢复。

**修正**：正常目录空结果仅属于当前选项版本，空分支状态也随版本失效；不永久改写展开能力、不删除待恢复意图，改变规则后重取并恢复展开，但不恢复旧焦点。环/非 UTF-8 的服务端不可展开语义保持。无需扩展树 JSON schema 或添加隐藏计数。

**验收落点**：04/05 验证“仅含隐藏孩子的普通目录”显示→隐藏→显示往返；同版本空分支不重复加载，真正空目录和结构性不可展开状态分别覆盖。

## R4：跨完整页、reload 与 BFCache 的恢复入口未定义

**代码与运行时证据**：`assets/chapbook-browser.js:1959–1964` 会写入 null 历史状态；树入口 `2271–2279` 清 token/控制，退出 `2411–2414` 完整重载。实测任意历史标记可跨 reload 保留，跨完整页后退可恢复原标记；后退与前进均实际观察到 `pageshow.persisted=true`。

**风险**：`[INFERENCE]` 若补 token 重载再次按共享偏好初始化，其他 tab 的新偏好可覆盖历史布局；只保存在模块内存的定位目标也会丢失。仅按 pathname 恢复无法区分独立阅读、树内文件、搜索及打开/选中身份。

**修正**：先消费有效 managed 快照，再做 URL 规范化和普通初始化。快照包含表面、浏览布局、上下文/查询/hash、显示规则，以及打开/选中条目各自的编码 identity，不含 token 或正文。同路径补能力 replace 后 reload；跨完整页只形成一个携带快照的目标历史项。没有有效快照的新访问才使用 URL > 偏好 > 默认；分享 URL 不带布局。

**验收落点**：04/05 覆盖真实 reload、跨完整页后退/前进、其他 tab 改偏好后恢复、补能力不多增历史。`tests/browser_mode_toggle_toolbar_acceptance.mjs:400` 与 `tests/browser_tree_async_acceptance.mjs:423–426` 都禁用 BFCache，因此必须另有允许 BFCache 且观察到 persisted pageshow 的生命周期场景。

## R5：异步加载没有可消费的失败结果，历史失败留下成功标记

**代码证据**：`assets/chapbook-browser.js:1096–1110` 的子分支加载在成功/失败路径都可能 resolve 为 undefined；失败被吞掉。历史加载失败或身份不符时 `1908–1912,1946–1949` 清 pane，`2035–2037` 却仍仅按 paneCommittedPath 跳过同路径加载。

**风险**：`[INFERENCE]` 两个已展开分支刷新一成一败，无法据原 Interface 判断整体结果，旧集合可能冒充新规则或失败被当空目录；历史失败后同路径恢复也可能被错误跳过。

**修正**：分支结果区分成功、当前版本失败与过期。控件立即提交新规则，成功分支安装新数据，当前版本失败分支显示安全错误，不保留可操作旧集合或删除待恢复展开意图；过期成功与错误均丢弃，不自动重试。请求目标与成功安装内容分开：普通 fragment 导航失败不提交目标历史，历史恢复失败保留已切换目标并失效成功标记。目录/搜索内容键包含相关显示规则，文档正文不因规则改变重转换；错误/忙占位不是跳过重取的成功凭据。

**验收落点**：04/05 覆盖两个分支部分失败后释放旧成功/错误、历史失败后同路径再次恢复，以及已打开内容/焦点/展开意图保持；不能只沿用根请求失败场景。

## 其他接口澄清

- 02/03/04 的交接明确为服务器提供页面语义、规范编码 identity、q 与 ListingOptions；统一输出并由客户端消费，复用既有编码器与 data-native-path-encoded，不从显示文本或另一个 query-path 重建身份。
- 最终 ResponseMode 决定错误出口，必须覆盖参数、路径、metadata 与 Path 提取的早期失败。存在 fragment 键不等于最终 fragment，download/raw 优先级保持；400/403/404/429、Retry-After 及 Office fragment 的既有 200 安全占位不改。
- No-JS GET 表单不能只靠 action query 保存状态，必须提交必要字段；分别覆盖搜索排序/隐藏、重新搜索、条目/../面包屑/搜索位置导航，实际离开搜索才移除 q。
- 01 的过滤、raw-name 比较、扩展名、目录优先、固定次级顺序、过滤前 identity 与 BFS 候选集上限无设计阻塞。02/03 继续共享 render.rs integration owner；未引入额外模块、平行模板或客户端重复排序。
- GLOSSARY 已区分显示偏好/显示规则、浏览上下文、选中条目/打开条目、独立阅读页；本轮工程澄清不增加领域术语或 ADR。

## 本轮实际验证与限制

在隔离临时根目录上执行 `cargo run -- --host 127.0.0.1 --port 38746 <临时根目录>`，启动当前版本。fixture 包括 Markdown、原始 HTML、普通文本和有效的一页 PDF。

实际 HTTP 观察：目录/文档 fragment 为 HTML + nosniff 且无 token；完整目录与完整搜索页在本机门禁允许时含 token，独立文档页不含。当前搜索 `?fragment=1` 仍返回完整可信页，非法 sort 的 fragment 仍为无类型裸 404，非法 UTF-8 路径 fragment 为 text/plain 400 且无 nosniff；这些是 02 原已分配的严格出口 cutover 前提，不是未来功能通过证明。原始 HTML 带 sandbox allow-scripts；PDF Range 返回 206、nosniff、无 sandbox，下载返回 attachment 且带 sandbox。

实际 Chromium 观察：reload 保留 history.state 与历史长度；现有树入口丢掉排序 query 和历史快照，退出重载恢复本机打开控制。完整页目录→文档→后退/前进恢复原历史标记，并在两个方向触发 persisted pageshow。标记用于核对浏览器 API，不是已经实现的新状态协议；本轮未发起 native-open POST 或启动本机应用。

运行证据保留在会话 artifact：`local://browser-design-review-runtime.json`（11 个 HTTP 前提及 reload 标记）、`local://browser-design-review-lifecycle.json`（跨完整页与 BFCache 状态）。只记录 token meta 是否存在，不记录 token 值。

本轮仅改计划/issue/review 文档，没有实施功能、运行新功能验收或执行 cargo test/fmt/clippy；本轮 smoke 不能替代 05 的未来验收。临时服务、浏览器和 fixture 在交付前清理。
