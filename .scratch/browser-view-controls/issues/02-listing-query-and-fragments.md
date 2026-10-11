# 02: 接通目录、搜索、树与 fragment 的显示规则

**What to build:** 让所有条目出口接受同一套 `sort` / `hidden` query，并提供树视图所需的安全搜索片段。

**Blocked by:** None（[01](01-shared-listing-policy.md) 已完成）。

**Status:** ready-for-agent

## Context

产品决定见 [spec.md](../spec.md) Q2、Q8–Q12、Q24–Q25。服务器不读取 localStorage，不感知浏览视图；文件响应模式与路径安全不是本次重构目标。

## Required change

- 在 `src/routes.rs` 复用统一 query 解析，得到 `ListingOptions`。保留 `sort=Column:Order` 的精确语法与非法值 404；新增 `hidden=0|1`，缺省 false，非法值同样 404。
- 完整目录页、目录 fragment、`GET /__/tree` / `GET /__/tree/{*path}` 与 `GET /__/search` 均传递选项到同一 listing Module。
- 与 03 共用页面上下文交接：服务器提供目录/搜索/独立阅读的明确语义、文件系统位置的规范编码 identity、搜索 q 与 ListingOptions；03 统一输出，04 消费。identity 复用现有编码器与 `data-native-path-encoded`，不从显示标题或另一个 query-path 重建。
- 删除目录 fragment 固定默认排序、树固定目录优先名称排序，以及忽略用户 sort 的旧路径。
- 树 JSON 继续以现有 name/href/dir/expandable schema 返回已排序条目；不为了客户端复制排序而添加全套大小/时间字段。保留 spawn_blocking、原始路径解析和祖先 identity 环检测。
- 搜索支持完整页与 `?fragment=1` 出口，以同一 `SearchResult` 及相同过滤/排序规则渲染。完整搜索页在原有门禁允许时继续提供 native-open；片段不携带 token，不 fetch 完整页后从 head 抽 token 或把完整文档塞入片段。
- 搜索空查询仍不遍历；大小写不敏感匹配、BFS 候选集、500 上限与 Semaphore(1) 不排队保持。忙仍 429 + Retry-After: 1，permit 由真实阻塞 job 持有到结束。
- 先判定最终 ResponseMode，再进入会提前返回的参数/路径/metadata 错误出口；Path 提取等框架拒绝也不能绕过该出口。仅最终模式为 fragment 时，成功与错误均为 CB 生成的 HTML + `text/html; charset=utf-8` + nosniff，不能只检查 fragment 键是否存在。保留非法 query/缺失目标 404、越界 403、非法 UTF-8 路径提取 400、搜索忙 429 + Retry-After；普通不可预览文件及 Office 忙的 fragment 仍为既有 200 安全占位。完整页与 JSON 保留各自既有错误样式。
- 保持 download > raw > fragment > view > 默认协商。`view=1` 仍是文件渲染模式，不是浏览布局。有效隐藏规则不禁止直接访问隐藏文件、ServeFile、下载或隐藏目录直链。
- URL 与 form 保留 q、sort、hidden 的必要字段；GET 表单以实际提交字段保留它们，不只依赖 action query。搜索改排序保留 q/hidden，改隐藏保留 q/sort，重新搜索保留 sort/hidden；条目/../面包屑/搜索位置导航延续 sort/hidden，实际离开搜索才移除 q。不新建 query-path 文件系统入口，不 decode 后重新编码 action identity。

## Acceptance criteria

- [x] 同一目录的完整列表/网格、目录片段与树 JSON 返回相同过滤集合及对应顺序。
- [x] 搜索完整页与片段匹配集合、截断提示及顺序一致；不泄露 token 或回退原始字节。
- [x] 显式 `hidden=1` 能展示点文件和点目录；`hidden=0` 的搜索不进入隐藏分支，直链仍可达。
- [x] 六键/两方向实际影响树 JSON 与目录片段，不再被忽略。
- [x] sort/hidden 非法值、越界路径、缺失目录、非法 UTF-8 路径提取及忙搜索均有准确状态和正确 response kind；fragment 与 download/raw 混用按最终优先级选择出口。
- [x] Range、下载头、raw sandbox/PDF/媒体例外、可信完整页反嵌入策略及 native-open 四道门不变。

## Verification ownership

实现期间跳过 build/lint/tests/formatters；路由回归、真实 HTTP 与浏览器证明统一在 [05](05-acceptance-and-documentation.md)。旧 `tree_endpoint_ignores_sort_param` 等旧行为契约须改成新消费者行为，不保留旧路径。

## Comments

计划待最终确认；没有启动实现。

完整计划已获用户最终共同理解确认；已移除确认门槛并标为 ready-for-agent，保留实施依赖，尚未启动实现。

设计 review 已落实接口与错误出口澄清，并保留完整搜索页已有本机打开能力；详见 [review](../review.md)。搜索片段和旧提前错误返回本已属于本 issue 的实施范围，不将尚未实施的功能重复列为设计缺陷。保持 ready-for-agent，尚未实施。

### 2026-10-10 — 实施与验收完成

- 目录、树 JSON、目录/搜索 fragment 与完整搜索统一消费显示规则；树 JSON 未新增排序元数据。完整页交接语义与规范编码 identity，搜索片段不携带 token。
- 最终 ResponseMode 在提取/参数/路径/metadata 失败之前确定；真实 HTTP 覆盖 400/403/404 安全 HTML + nosniff、download/raw 优先、PDF 200/206 例外及下载隔离。
- 最终 release 的 8 个并发搜索请求实际得到 1 个 200、7 个 429；忙片段均为安全 HTML + nosniff + Retry-After: 1，未含 token。
- 消费者回归保留 Range、安全头、native-open 四道门与 Office/search permit 生命周期；全部验收项完成，详细证据见 [05](05-acceptance-and-documentation.md)。
