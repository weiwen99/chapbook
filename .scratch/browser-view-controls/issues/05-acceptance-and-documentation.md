# 05: 消费者行为验收与发布文档

**What to build:** 在所有功能改动集成后统一验证接口与真实浏览器行为，并更新使用说明及兼容性记录。

**Blocked by:** None（[04](04-shared-browser-state-and-navigation.md) 已完成）。

**Status:** ready-for-agent

## Context

完整验收矩阵见 [spec.md](../spec.md)。2026-10-10 的实际实施、最终构建验证及清理结果记录在本文 Comments；原有计划/review 评论作为历史记录保留。

## Required change

- 按实际行为更新 `tests/router.rs`、`tests/sort.rs`、`src/listing.rs` 及相关消费者测试。树忽略 sort、旧类型二分排序、旧混排等被新契约替代；精确 sort 语法与非法值 404 保持。
- 更新现有 `tests/browser_mode_toggle_toolbar_acceptance.mjs`、`tests/browser_tree_async_acceptance.mjs`、`tests/browser_pdf_native_open_acceptance.mjs` 的受影响场景，使用可访问入口驱动实际操作并验证内容、路径、状态和安全结果。
- 删除 `src/assets.rs` / `src/render.rs` 中本次涉及的源码字符串、类名排列、按钮 wording、精确高度、迁移顺序等 incidental 实现断言；不要重新锁定新内部实现。
- 永久测试只新增容易犯错的消费者边界：隐藏祖先/可见链接别名/500 上限、同值和 raw-name 排序、临时链接与长期偏好隔离、隐藏焦点回退、仅含隐藏孩子的目录展开往返、分支部分失败/过期错误、历史失败后同路径重取及跨文档恢复等。不能测 wiring、mock echo、源码文本、非空或长度变大。
- 构造隔离临时服务根目录，含普通/点文件、多级点目录、仅有隐藏孩子的普通目录、真正空目录、两个可独立注入慢响应/失败的展开分支、可见链接别名、环、特殊 Unicode/非法 UTF-8、无扩展名/大小写扩展名、同大小/时间、超过 500 条匹配、文档、媒体、真实 PDF。非 UTF-8 fixture 沿用仓库平台 guard。
- 集成完成后统一执行 cargo test、cargo fmt --check、cargo clippy --all-targets；实际启动当前构建二进制，以 HTTP 验证 query、fragment、JSON、状态、头及字节/Range 行为。
- 在真实 Chromium 中执行 spec 的桌面、窄屏、亮/暗、无 JS、键盘、直链、搜索、历史与异步场景，观察控件/内容并留截图或结构化证据。除现有禁用 BFCache 的隔离脚本外，必须有允许 BFCache 的生命周期场景，观察 `pageshow.persisted=true`；包含真实 reload、跨完整页后退/前进、其他 tab 改偏好后的历史恢复、同路径补 token 与文件进入父目录恰好一个历史项。仅 tests 通过不足以证明 UI。
- 展开分支验收包括显示→隐藏→显示仅含隐藏孩子的普通目录；同一选项版本的空分支不重复加载，改规则可恢复，结构性不可展开状态保持。两个分支刷新一成一败后再释放旧版本成功和错误，观察条目/错误/焦点/展开意图与已打开内容，不能只沿用根请求失败场景。历史加载失败后再次恢复同路径须重取；隐藏/排序不重转换文档。
- native-open 验收覆盖完整目录及完整搜索页列表/网格：搜索树→列表/网格补能力保留 q/sort/hidden 和布局。HTTP 覆盖非法 UTF-8 路径提取、非法 sort/hidden、缺失/越界、忙响应与 fragment/download/raw 混用的最终出口；No-JS 分别操作搜索排序/隐藏、重新搜索与条目/../面包屑/搜索位置导航，验证必要字段而非 form 源码。
- 保留原始文件隔离、可信 UI anti-framing、native-open 四道门、PDF/媒体 sandbox 例外、fragment nosniff/no-token、Office/search bounded-job permit 的既有回归。
- smoke 证明后更新 README：三种互斥视图、默认隐藏、六键含义、500 候选集后排序、链接与历史、v 循环、无 JS 行为。更新有关产品层级的行为文档和 ADR 0001 术语说明，不撤销客户端树壳架构。
- 在现有 release-notes 命名约定下新增 `docs/release-notes-unreleased.md` 记录功能和兼容性变化，不改历史 v0.2.0/v0.3.0 发布记录，不顺便 bump 版本或依赖。
- 清除临时 fixture/服务/throwaway 脚本；关闭浏览器；只报告实际运行的验证及尚存限制。

## Acceptance criteria

- [x] spec 每个验收场景都有可观察证据，所有产品决定得到端到端实现，不以按钮存在或编译成功代替。
- [x] Rust 检查统一通过，所有受影响消费者契约已更新，未保留 obsolete 行为或 incidental 源码测试。
- [x] 实际 HTTP、浏览器及无 JS 场景覆盖新增能力和既有安全不变量。
- [x] README、行为说明、术语与未发布变更记录一致；没有把未验证行为写成已通过。
- [x] 没有运行时外部依赖、自动重试、无关功能修复或永久 smoke scaffolding。

## Comments

计划待最终确认；没有执行功能验证或发布。

完整计划已获用户最终共同理解确认；已移除确认门槛并标为 ready-for-agent，保留实施依赖，尚未执行功能验证或发布。

设计 review 新增的生命周期、过滤空分支、部分失败、历史失败重取与搜索能力恢复验收已纳入；详见 [review](../review.md)。review smoke 只验证当前实现前提，不充当本 issue 的未来功能验收；保持 ready-for-agent，尚未执行新功能验证或发布。

### 2026-10-10 — 最终实施与验收完成

验证环境：Linux x64，隔离临时根目录，真实构建二进制与 Chromium。最终浏览器脚本及最后一轮 HTTP/窄屏表面验证使用 `target/release/chapbook`；不是源码断言或替代 UI。

#### 实际执行的命令

```bash
cargo test
cargo fmt --check
nix shell --extra-experimental-features 'nix-command flakes' nixpkgs#clippy -c cargo clippy --all-targets -- -D warnings
cargo build --release
CHROME_BIN=/run/current-system/sw/bin/google-chrome node tests/browser_mode_toggle_toolbar_acceptance.mjs
CHROME_BIN=/run/current-system/sw/bin/google-chrome node tests/browser_tree_async_acceptance.mjs
CHROME_BIN=/run/current-system/sw/bin/google-chrome node tests/browser_pdf_native_open_acceptance.mjs
```

| 检查 | 实际结果 |
| --- | --- |
| Rust 消费者回归 | 218 passed，5 suites，无失败 |
| 格式检查 | `cargo fmt --check` 通过 |
| 静态检查 | all-targets、`-D warnings` 通过；系统 Cargo 无 clippy，使用 Nix 提供的 clippy |
| 发布构建 | optimized release 构建成功，版本与依赖未变 |
| 工具区/无 JS 浏览器脚本 | 9/9 |
| 树异步/焦点/历史浏览器脚本 | 14/14 |
| PDF/native-open 能力/生命周期浏览器脚本 | 13/13 |

#### spec 验收矩阵对应证据

| 场景 | 可观察结果与证据 |
| --- | --- |
| 列表→网格→树→列表 | 工具区 T2/T3：单一视图选中态、同位置 replace、q/sort/hidden 保持；v 在编辑区及修饰键组合中不触发 |
| 首次访问与旧偏好 | T1：默认列表/Name:Asc/隐藏关闭，三个新偏好键均未写入；实际隔离存储验证旧 tree 优先、否则 grid/list，一次迁移后旧键删除 |
| 桌面、窄屏、亮/暗 | 实际 1240×900 与 390×844 观察及截图；最终暗色文档树视图的抽屉位于工具区下方，5 个控件与 12 个排序选项均通过真实命中检查，无树横向溢出；hover 与键盘 focus 提示已实际观察 |
| 点项、普通链接别名 | Rust 与实际 HTTP：隐藏过滤先于 identity 登记，普通 `visible-alias` 可访问隐藏目标；完整页、树与 fragment 使用一致规则 |
| 隐藏目录/文件直链 | 实际 HTTP：隐藏目录正常名字孩子仍可读，隐藏 Markdown 可直接渲染，raw 与 download 返回相同原始字节 |
| 隐藏选中项与再次显示 | 树脚本 Q17 与列表 Q17：最近可见祖先/行回退，不打开回退项、不新增历史；实际网格验证隐藏 `.secret.md` 后打开文档仍是同一 DOM，选中回退到邻近可见项 |
| 六键、两方向、等值、扩展名 | Rust 排序/路由回归覆盖六键×两方向、目录优先、固定次级顺序及 Type；T5 以 Tab/Enter 选择 Size:Desc 并观察实际顺序与同历史项更新；实际 Name:Desc/Type:Desc 摘要显示当前规则 |
| raw 名称、Unicode、非 UTF-8 | Rust identity/排序回归及真实目录/tree：非法 UTF-8 名称显示可见 escape、不可操作；`doc+ %252e #.md` 的小写 percent-encoded 直链被规范为同一编码 identity，独立阅读与树快照保持 |
| 超过 500 个匹配 | 最终 HTTP fixture：501 个普通匹配、700 个隐藏匹配；hidden=0 返回 500 个普通项，Size:Desc 首项为 `499-capmatch.bin`，更大的 `500-capmatch.bin` 被候选集上限排除；hidden=1 返回 500 个点名项，边界回归验证 truncated |
| 搜索三视图与历史 | T7、PDF 脚本 8/9 及历史场景：树搜索安全片段保持 q/规则，切回列表/网格的完整页补能力不多增历史；实际条目导航离开搜索，历史恢复查询上下文 |
| URL 规则与保存偏好不同 | Rust/QuickJS 消费者矩阵及实际隔离存储：显式 Name:Asc/hidden=0 不覆盖保存的 Size:Desc/hidden=1，只持久化手动改变字段 |
| 控件变化与真实导航 | T2–T5、PDF 脚本 9/10 与树历史场景：同位置控件/补能力 handoff 不增长历史，实际路径导航新增一个目标项；文件进入父目录保留定位 |
| 已展开树、预览、慢响应 | 树 A/A2/1/2/C/3/D：共享单次请求、缓存展开、旧文档结果不提交、显式采用请求与折叠意图保持；T4/T5b 在列表/网格中验证同一 live 文档且零 fragment 重取，树场景保持打开内容 |
| 普通目录仅含隐藏孩子 | 树 only-hidden-children 场景：empty 状态仍可展开，同版本不重复加载，新规则重取并恢复孩子；固定“目录片段先完成、分支后释放”顺序，修复前 13/14、修复后最终 release 14/14；环链接仍不可展开 |
| 两个展开分支一成一败 | 树 two-expanded-branches：一支安装当前数据、一支显示当前安全错误，旧成功/旧错误不回灌；打开文档与展开意图保留，无自动重试 |
| reload、完整页历史、BFCache | 树 reload/back/forward、PDF 脚本 11/12：真实 reload 按快照而非其他 tab 偏好恢复；BFCache 实际 `persisted=[false,true]`，恢复后 tree/fragment/Document 请求均为 0，正文、URL、快照不变；实际 opened=`note.md`、selected=`beta` 跨完整页返回后独立恢复 |
| 历史失败后同路径恢复 | 树 history-restore-failure：目标 URL/快照保留、安全错误展示、成功标记失效；fragment 次数 1→2→3，下一次同路径恢复确实重取并成功安装 |
| 文档直链与独立阅读 | T6、实际 Markdown/Org/CSV/代码页：无第四视图选中态，隐藏/排序禁用，父目录入口正确；最终文档→树实际显示 1 个 KaTeX 公式和 Mermaid SVG，正文亮/暗正确且无 token |
| 无 JS | T8 真正禁用脚本，点击隐藏/排序链接、提交搜索 GET 表单、重新搜索及条目/../面包屑/位置导航；必要字段与实际目标均正确，不错误延续 q |
| fragment/JSON/错误/忙 | 最终真实 HTTP 11 组验证涵盖 400/403/404 严格 HTML + nosniff、非法 sort/hidden/UTF-8、越界/缺失、树 schema 及 raw/download 优先；8 个并发搜索实际为 200×1、429×7，忙片段均 HTML + nosniff + Retry-After: 1、无 token；Rust 回归锁定真实 job 持有 permit |
| PDF/native-open/raw/download/Range | PDF 脚本 1–10/13 与真实 HTTP：PDF 新 tab、目录 folder.pdf 正常展开/预览，隐藏 `.pdf` 后缀目录 reload 后仍为目录且零 PDF 打开；目录/搜索列表网格保留合法完整页本机打开能力，树及 fragment 无 token；真实原生查看器显示一页 `Controls smoke PDF`。PDF Range 206、下载 attachment+sandbox、四道门及媒体例外回归通过；实际 raw HTML 为 opaque origin，不能读取可信页或 localStorage |

#### 文档与清理

- 已更新 README、AGENTS 行为契约、`docs/2026-08-12-design-readonly-file-browser.org`、ADR 0001、spec 及五份 issue；新增未发布记录 `docs/release-notes-unreleased.md`，不修改历史发布记录、不 bump 版本/依赖。
- GLOSSARY 中已经确认的领域定义保持不变；实现细节不搬入术语表。受影响的旧行为与源码/字形/按钮文案/精确像素 incidental 测试已删除，保留消费者边界与交互回归。
- 三份现有浏览器脚本保留为行为验收入口；未新增永久 throwaway/smoke 脚本、自动重试或运行时外部依赖。临时 HTTP/浏览器补充验证在隔离服务与 Eval 中执行。
- 自建 debug/release 服务与自建浏览器已关闭，隔离临时根目录已删除；脚本自建的服务、Chrome profile 与 fixture 随退出清理。用户原有 7777 服务未停止、未重启、未修改。
- 发布产物为 `target/release/chapbook`；原有运行进程不会自动获得嵌入资源更新，重启时需使用新二进制。没有遗留未完成验收项。

### 2026-10-10 — 树目录折叠后刷新回归修复

- 用户报告：左侧目录展开后手动折叠，刷新页面却重新展开。
- 稳定复现了两个来源：保存的子目录展开记录在恢复时逐级展开父目录；当前已打开文档的路径定位也会展开刚折叠的祖先。单独展开、折叠一个没有子树记忆或已打开后代文档的目录未触发问题，未将该窄场景作为结论。
- 将快照/显示规则恢复与实际导航定位分开：前者尊重目录的折叠状态，后者及新进入树视图仍展开目标路径。折叠不清空后代展开记忆，显式重新展开父目录时恢复子树；不会自动重试已有错误的孩子分支。
- 不改变已打开文档、URL、排序/隐藏规则、独立选择状态和现有异步分支所有权约束；无存储迁移、新依赖或自动重试。

#### 永久消费者回归

在现有 `tests/browser_tree_async_acceptance.mjs` 中增加真实 Chromium 点击与页面 reload 场景：

| 场景 | 修复前证据 | 最终 release 结果 |
| --- | --- | --- |
| R：展开父子目录、折叠父目录、刷新，再显式展开父目录 | 刷新使父目录重新展开；初步修复后补充断言又捕获子树记忆未恢复 | 刷新保持折叠；显式重新展开恢复孩子展开与可见内容，选择/URL/历史不变 |
| R2：打开后代文档、折叠祖先、刷新 | 当前文档定位重新展开祖先 | 祖先保持折叠，已打开文档、URL、历史与父目录选择保留 |
| R3：孩子分支加载失败、折叠并重新展开父目录 | 初步子树恢复实现触发了非用户要求的自动重试 | 错误保持可见；只有显式重试孩子后才成功安装内容 |

脚本支持可选的精确 `CB_SCENARIO` 过滤；不存在的场景名称报错，不允许空跑显示成功。

#### 最终验证与交付

| 检查 | 实际结果 |
| --- | --- |
| `cargo test` | 218 项通过 |
| `cargo fmt --check` | 通过 |
| `nix shell --extra-experimental-features 'nix-command flakes' nixpkgs#clippy -c cargo clippy --all-targets -- -D warnings` | 通过，零警告 |
| `cargo build --release` | 通过 |
| Release 浏览器树异步验收 | 17/17 通过 |
| Release 浏览器工具栏验收 | 9/9 通过 |
| Release 浏览器 PDF/native-open/历史恢复验收 | 13/13 通过；BFCache 实际 `persisted=[false,true]`，恢复时 tree/fragment/Document 重取均为 0 |

- 三份浏览器脚本使用 `CHROME_BIN=/run/current-system/sw/bin/google-chrome` 与默认 `target/release/chapbook` 执行。
- 隔离的实际界面验证：父目录折叠刷新保持；重新展开恢复已记忆子树；已打开文档的祖先折叠刷新仍保持，正文与 URL 不变；新进入树视图仍展开文档路径。排序/隐藏规则改变后，折叠状态保留且仍为同一个 live 文档节点。最终 release 界面实际刷新后，`adir/grand` 仍折叠，父目录仍展开，选择与 `Size:Desc`/`hidden=1` 保留，正文仍为原来的 `Nested collapse proof`。
- 已更新 README、只读浏览器设计文档和未发布记录。无新领域术语、服务端或安全契约变化，GLOSSARY、ADR 与 AGENTS 保持不变。
- 本次自建 debug/release 服务和浏览器已关闭，隔离临时目录已删除；用户原有 7777 服务未停止、未重启、未修改。发布产物已更新为 `target/release/chapbook`，现有进程需要使用新二进制重启后才能获得修复。
