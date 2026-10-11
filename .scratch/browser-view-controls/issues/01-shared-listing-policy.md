# 01: 统一条目过滤与排序 Module

**What to build:** 在现有 `listing` / `sort` / `meta` 中提供唯一的隐藏项与排序规则，供目录、搜索、树和目录片段共用。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

## Context

产品决定见 [spec.md](../spec.md) Q3、Q9–Q12、Q18–Q22；领域用语见 [GLOSSARY.md](../../../GLOSSARY.md)。实施结果记录在本文 Comments，统一验证证据见 [05](05-acceptance-and-documentation.md)。

## Required change

- 在 `src/listing.rs` 定义小型 `ListingOptions` Interface：`sort_by: SortBy` 与 `show_hidden: bool`，默认 Name:Asc / false。复用现有 `SortBy`，不新建通用文件系统 Adapter。
- 以原始条目 basename bytes 判断点名前缀；不能用 `display_name`、链接目标名字或 lossy 文本推断 identity。服务根目录自身不参与隐藏规则。
- `list_dir` 按选项过滤并原地排序。搜索保持原始名字字节序的确定性 BFS：隐藏关闭时在登记目录 identity、入队和累计匹配数之前跳过隐藏项；普通名字的符号链接仍可遍历，即使目标名字隐藏。
- 搜索最多收集 500 条符合过滤规则的匹配，第 501 条合格匹配触发 truncated；收集后才按显示规则排序，不改成全根目录 Top-N，不改变目录去环和 hard-link 路径保留规则。
- 唯一共享 comparator：目录优先不受升降序影响；主键按选择方向，同值固定原始名称升序、完整 root-relative 原始路径升序。名称区分大小写，不做自然数排序。
- Type 按文件末尾扩展名、ASCII 大小写不敏感；无扩展名在升序时位于有扩展名前，`.tar.gz` 为 gz，`.env` 无扩展名。目录不按名字后缀分类。
- `FileMeta` 提供借用原始名称/扩展名的辅助能力；扩展名比较不为每次 comparator 调用分配小写 String。类型列显示通过既有安全显示 codec 转义，不改变 `data-kind` 的 dir/file identity。
- 大小与时间沿用既有 metadata 语义及 creation→mtime→epoch fallback，不递归统计目录大小，不另建元数据缓存或读取文件内容探测 MIME。
- 改动 exported Interface 前先获取引用清单；有 LSP 时用 references。迁移全部调用方，删除旧 `display_name` 排序、整表 reverse 与 Directory/File 类型排序路径，不保留兼容 overload。

## Acceptance criteria

- [x] 不同类型、大小、时间和等值条目在六键/两方向下均满足目录优先和确定性次级顺序。
- [x] 点目录的隐藏跳过不会抢先占用 identity，导致普通名字的链接别名无法搜索其内容。
- [x] 500 个名额只由可展示匹配占用；排序不改变候选集合或 truncated 的语义。
- [x] 名称与路径始终来自原始 identity，非 UTF-8 条目仍可安全显示而不可操作。
- [x] 显式打开隐藏目录时只过滤其直接子项，不将正常名字的孩子清空。
- [x] 同一 comparator 被所有条目出口使用，不出现客户端另一套类型/时间排序规则。

## Verification ownership

实现期间不单独运行 build/lint/tests/formatters；统一验收见 [05](05-acceptance-and-documentation.md)。永久测试仅保留消费者可见排序、过滤、数量边界与 identity 行为，不锁源码或转发 wiring。

## Comments

计划待最终确认；没有启动实现。

完整计划已获用户最终共同理解确认；已移除确认门槛并标为 ready-for-agent，尚未启动实现。

设计 review 已完成：本 issue 的过滤、比较与候选集契约无阻塞项；[review 记录](../review.md) 的客户端空分支恢复修正由 04/05 落实，不向树 JSON 添加隐藏计数或排序字段。保持 ready-for-agent，尚未实施。

### 2026-10-10 — 实施与验收完成

- `src/listing.rs` 的 `ListingOptions` 和共享 comparator 已接通全部条目出口；排序原地进行，比较借用原始名称与扩展名，不为扩展名比较创建小写字符串。
- 隐藏过滤先于搜索 identity 登记、入队与匹配计数；六键双向、固定次级顺序、链接别名、非 UTF-8、隐藏直链及 500/501 边界由消费者回归验证。
- 最终 release 的实际搜索 fixture 含 501 个普通匹配与 700 个隐藏匹配：隐藏关闭返回 500 个普通候选，Size:Desc 首项为 `499-capmatch.bin`，更大的第 501 项仍不入选；开启隐藏返回 500 个点名候选。
- 所有验收项完成；命令、浏览器与安全证据集中见 [05](05-acceptance-and-documentation.md)。`ready-for-agent` 保留为原有 triage 标签，不表示实施仍待启动。
