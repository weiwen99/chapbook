# EPUB、CBZ、FB2 服务端可读预览设计

Status: accepted — 设计与实现各第 2 轮 Standards / Spec 独立评审通过；本机 HTTP/浏览器验收完成，见 [review.md](review.md)
Date: 2026-10-30
基线：`d8ea4c4`（main）。需求与已确认决策：[评估记录](../../docs/2026-10-30-emacs-reader-preview-assessment.md#7-已确认决策)。术语沿用 [GLOSSARY](../../GLOSSARY.md) 的“可读预览”。

## 1. 规模与范围

| 维度 | 需求要求 | 设计产出 |
|---|---|---|
| 格式 | 修补 EPUB，新增 CBZ/FB2 | 相同三种；不新增其他格式 |
| 数据表 / 缓存 / 后台服务 | 0 | 0 |
| Module | 三种格式复用现有阅读表面 | 1 个私有 `books` Module，4 个源文件；复用 `render`/路由 |
| HTTP 出口 | 原文件模式保持；漫画页可读取 | 只增加 1 个 CBZ 图片子资源出口 |
| CLI / 配置 | 0 | 0；资源限额为实现常量 |
| 新依赖 | 纯 Rust、Apache 兼容 | 新增 roxmltree；zip/base64/encoding_rs/quick-xml 提升现有锁版本为直接依赖 |
| 工程师日 | 已确认的有限范围 | 初步量级 12–20 人日 `[INFERENCE]`，不是交付承诺 |
| 发布波次 | 一次完整交付 | 1；无数据迁移、无渐进切换协议 |

目标：独立阅读页无 JS 也可读；启用 JS 后相同内容进入现有列表/网格预览面板和树视图内容区。PDF 保持浏览器原生查看，Office 保持原正文管线。不要为本需求放开文档脚本执行。

非目标：MOBI/KF8、XPS/OXPS、ODG、CBR/CBT、`.fb2.zip`；原 CSS/固定分页保真；书签、标注、阅读进度同步；MuPDF/foliate-js/外部转换程序；原有 Office 图片改造。

## 2. 证据与复用决定

1. 上一轮已用真实 chapbook + Chromium 验证：EPUB 内嵌 PNG 变 alt，跨章节 href 存在但目标 `<a id>` 被转义；FB2 为 XML 源码；CBZ 为占位。复现与原因已记录，不重复跑基线来确认已知失败。
2. `anydoc::to_document` 返回公开 `Document { blocks, notes, assets }`；现有 `to_markdown_bytes` 会丢资产且不能配置私有 serializer。改用一次 `to_document` 加受控 HTML renderer，不 fork、不升级 anydoc、不重复解析、不关闭 comrak 的 `escape`。
3. EPUB 与 FB2 共用现有 anydoc model，不引入另一份通用文档模型或插件 trait。CBZ 是页图片集合，不硬塞进文本模型。
4. `render::doc_page`、`doc_fragment`、`slugify`、`toc_html`、`.cb-doc` 样式及浏览器布局/TOC 驱动直接复用。文件 identity 仍只来自 `encode_relative_path`。
5. FB2 选择 roxmltree 0.21 + encoding_rs：有节点限制、明确的外部实体解析器开关，可直接映射公共 model。`fb2` crate 仍需同样映射，还引入 quick-xml 0.30 与现有 0.41 并存；事件式 quick-xml 则需要更复杂的嵌套状态机。新增 DOM 依赖是用小而明确的 Interface 替代该状态机。
6. zip 保持 anydoc 同款 `default-features = false, features = ["deflate"]`，不让 dev 默认 features 隐式扩大 release 支持面。只承诺 Stored/Deflated 的普通单卷 ZIP。

## 3. Module、Interface 与职责

`src/lib.rs` 注册私有 `mod books;`。不扩大公共库 API。

| 文件 | 职责 |
|---|---|
| `src/books/mod.rs` | 格式识别、类型化错误、共同限额、受限读入、EPUB 一次解析与分派 |
| `src/books/html.rs` | anydoc Document → 安全 HTML；统一锚点/TOC/列表/表格/图片/链接 |
| `src/books/fb2.rs` | 严格解码/XML → anydoc Document；不产生原始 HTML |
| `src/books/cbz.rs` | 受限归档索引、自然页序、阅读 HTML、按索引读取一张图片 |

面向路由的 Interface：

```rust
pub(crate) enum Format { Epub, Fb2, Cbz }
pub(crate) enum Error {
    Io(std::io::Error),
    Invalid(String),
    Unsupported(&'static str),
    Limit(&'static str),
    NotFound,
}
pub(crate) struct Image {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
}
pub(crate) fn format_for_path(path: &Path) -> Option<Format>;
pub(crate) fn render(path: &Path, format: Format, encoded: &str) -> Result<String, Error>;
pub(crate) fn cbz_page(path: &Path, index: usize) -> Result<Image, Error>;
```

`render` 返回已经转义/安全生成的 body（含 TOC），不返回完整页面或 `.cb-doc` wrapper；路由沿用安全文件名作为页面 title，恰好包装一次。所有同步读入/转换只在 blocking job 中调用；`encoded` 为已有编码器的输出，不由书籍模块 decode/rebuild。内部 `fb2::parse(&[u8]) -> Result<Document, Error>` 和 `html::render(&Document) -> Result<String, Error>` 是两个实际 Adapter 共用的 Seam。

不新建渲染注册表、缓存、任务调度框架。既有危险 URL 判断可从 Org 的私有函数移到共享呈现 helper，原 Org 行为保持不变，不新造第二套 scheme 规范。

## 4. EPUB 与公共 HTML 呈现

### 4.1 内容保留

- EPUB 一次调用 `anydoc::to_document(bytes, Format::Epub)`；按其 spine 顺序保留正文（含非 linear 的已解析内容）。不另做 NCX/nav 或未在 spine 内的封面发现。
- 穷举处理公开 model 的 Heading、Paragraph、List、Table、BlockQuote、CodeBlock、Rule，及 Text/Link/Image/Anchor/NoteRef/LineBreak。
- 保留粗体/斜体/删除线/行内代码、有序列表起点/marker label/任务勾选、表格 Origin 的 colspan/rowspan（Covered 不重复产生单元格）、代码块转义和注释内容。允许 Layout 单格表展开为正文，与旧转换语义相同。
- 不解析或执行书中的 script、事件处理器、CSS；没有任何来源字符串可作为 `PreEscaped` 直接进入可信页面。只对 Module 自己生成的 HTML 使用外层 `PreEscaped`。

### 4.2 锚点与 TOC

预扫描 blocks/notes，包括列表和单元格：分配 heading slug，建立源 anchor → 不透明目标 id 的映射。标题使用 `render::slugify`；内部目标与标题共用 `used` 集合，保证全局唯一。不得直接把 EPUB 包内路径用作 DOM/action identity。

- 标题保持 `<hN><a id="..." href="#...">…</a></hN>` 结构，TOC 与正文使用同一已分配 slug。
- 对 `Heading.anchor`、`Inline::Anchor` 都建立映射；重复源 id 首次出现生效；空 id 不产生空 fragment。
- 对章节起点、段落中锚点保留准确落点；不因为只有 anchor 就生成可见空段落。
- 内部 LinkTarget::Anchor 只链接实际存在的目标；缺失目标保留文字，不产生死链接。
- 外部链接沿用现有危险 scheme 防线；阻止 javascript/vbscript/data/file 及 ASCII 空白/control 混淆。包内非 spine 的 Relative 资源不能当文件系统 URL 原样透出，保留可读文字。

### 4.3 图片

内嵌图片以受限的 `data:image/...;base64,` 放入 `<img>`，无需资源缓存、持久化或重复解析。PNG/APNG、JPEG、GIF、BMP、WebP、AVIF、ICO 依据字节签名选择 MIME，SVG 仅作为 `<img>` 图像文档呈现，绝不内联 SVG 标记或在可信源下执行。候选后缀为 `png/apng/jpg/jpeg/jpe/jfif/gif/bmp/webp/avif/ico/svg`，大小写不敏感；索引不能另行缩窄共同判定器支持的格式。SVG 声明需确认为 SVG XML 根，使用下述同一受限 XML 入口，不配置外部实体 resolver。

`ImageSource::External` 的 HTTP/HTTPS 图片可由浏览器直接读取，与既有 Markdown 一致；服务端不发网络请求。来源自带 data URI 仅接受受限、校验过的图片，再归一化为生成的 data URI；不接受 data HTML 或任意 scheme。缺失/非支持图片输出明确的图片占位及转义 alt，不静默消失。超过资源预算则整次预览失败并走已有降级，不冒充完整成功。

这里不保证浏览器解码后的像素内存上限，现有独立图片预览也没有这一保证。

## 5. FB2 Adapter

- 支持独立 `.fb2`，扩展名 ASCII 大小写不敏感。严格识别 FictionBook 根（标准命名空间或无命名空间），无容器后缀猜测。
- 解码：BOM 优先；无 BOM 时识别 XML 声明，默认 UTF-8。使用 encoding_rs 无 replacement 的严格解码，支持 UTF-8、UTF-16 BOM 及声明的常见 Windows-1251 等编码；未知/非法编码失败。转码后的 UTF-8 仍检查预算。
- XML 入口先检查解码后 UTF-8 中字面 `<`、`=` 的数量，各不超过 200,000，防止 roxmltree 按字符估算直接预分配超量 node/attribute 容量。使用现有锁版本 quick-xml 的借用事件读取，仅检查 prolog：允许外部 identifier-only DOCTYPE；遇到引号外 `[` 的内部 subset 即拒绝，不能让 DOM 构建开始后才发现实体膨胀。注释/CDATA 中的示例文本不是 DTD 声明。随后 roxmltree 设置 `allow_dtd = true`、`entity_resolver = None`、`nodes_limit = 200_000`；外部 DTD/实体绝不读取，未知实体引用失败。roxmltree 的嵌套解析是迭代的，模型转换另限制深度 128。FB2 和 SVG 共用此私有 XML 入口。
- 读取 `book-title`，保留 coverpage 图像和 annotation；body/section/title/paragraph、subtitle、poem/stanza/v、cite/epigraph/text-author、表格与混合内容转为 model。未建模的装饰保留文本，不执行 markup。
- 普通及 notes body 保持源顺序；section/p 的 id 产生源锚点，`a` 的 xlink/unqualified href 通过同一映射跳转。保留 `[1]` 等原注释链接文字，不强行重编号。基本脚注是可点击的正文导航，不承诺弹窗或阅读器批注。
- binary 先限制 base64 长度、去 XML whitespace、严格解码；id → AssetId 首次出现生效。只为图片引用生成图像；缺失引用给可见占位。图片与链接最终都经公共 renderer，不另做一套 HTML 清洗。
- 破损 XML、未声明 namespace prefix、未知实体不“猜测修复”；完整页原文件、fragment 安全占位。

## 6. CBZ 与图片子资源

### 6.1 归档与页序

使用 `File` + zip 的随机读取，不把整个漫画读入 Vec，不解压落盘。不把成员路径 join 到文件系统。

在 zip 初始化前，对最多 65,557 字节尾部执行 ZIP32 EOCD 预检：选定匹配 EOF 的记录，拒绝分卷、ZIP64 标记/locator，检查条目数与 central directory 字节/范围。只接受普通从 offset 0 起的 ZIP；SFX/ZIP64 不在首版承诺范围。

**预检与实际索引必须绑定**：从同一个已打开 File 读取最多 8 MiB 的 central directory 快照，逐条检查固定 header 和 name/extra/comment 声明长度，要求恰好消费已声明的条目数与目录范围。目录快照及归档 comment 中含其他 EOCD 签名的歧义输入拒绝。初始化时使用局部 `Read + Seek` 适配器：原目录以前的虚拟字节为零，目录来自已校验快照，尾部为所选 EOCD 的无 comment 副本，虚拟 EOF 紧随该 footer；`ArchiveOffset::Known(0)` 固定原始坐标。因此库看不到 comment/文件数据中的另一 footer，解析失败也不能 fallback 到未校验目录。初始化完成后，同一适配器切回同一个 File 的正常读取；不使用 unsafe reader/metadata 替换，不跨请求缓存。原始 local-header offset 不改写。

这是一次请求内的有界索引读取，不是自写解压器；实际 ZIP extra fields、CRC 和解压仍由 zip 处理。不能仅检查 footer 声明后把原 File 原样交给 `ZipArchive::new`，也不能把 `Known(0)` 误当作禁止 EOCD fallback。

建索引后复核 entry 数；若库按重复 raw name 折叠导致数量不符，拒绝歧义归档。目录、symlink、`__MACOSX` 与隐藏路径段不当作页面。支持图片名单与公共图片判定一致；未知文件/元数据不变成页面。加密或非 Stored/Deflated 的候选页拒绝预览。

按完整成员名做数字感知自然升序：比较数字段时去前导零后比较长度/内容，不能解析为可能溢出的整数；相等时名称及索引确定顺序。成员名称只在服务器排序，不进入 URL、DOM id、alt 或可见文件名。

### 6.2 阅读 HTML

全页序列，每页有可读页码、稳定锚点、真实上一页/下一页 fragment 链接和 `<img loading="lazy">`。预留图片空间以免加载前所有页折叠；不需要 JS 翻页、不增加布局/history 参数。不会为了无 JS 而一次性解压所有图片。

禁用 JS 时浏览器会忽略原生 lazy loading，可能请求全部页面；两任务等待门仍限制服务端并发。这是无 JS 阅读的资源取舍，不因此加入新分页状态。坏页在子资源返回 422，页面保留页码/替代文字及原文件入口；不提前解压整本来检测每张图片。

图片 URL：`/__/book-page/{encoded-book-path}?page={zip-index}`。索引是 zip 库的 index，不是用户可控 member 路径；重复成员已拒绝。archive 文件变化后的跨请求一致性不保证，重新加载恢复；不建 snapshot/cache/fence。

### 6.3 HTTP 子资源

新增 `GET /__/book-page/{*path}?page=N`，与 `/__/static` 同类，不是文件条目出口，不解释 `download/raw/fragment/view/sort/hidden`。不改变 `.cbz` 本身的响应模式优先级。

- 路径解码沿用 Axum，归档路径复用 `resolve_within_root`，保留现有跟随 symlink 语义；只接受普通 `.cbz` 文件。
- page 为非负 usize。路径提取失败 400；越界 403；缺失/类型不符/索引不存在 404；无效 page 404；损坏/不支持/超限归档或页 422；内部 job 失败 500。
- 成功体是一张图片，不是 HTML；`Content-Type` 为校验后的图片 MIME。所有出口附 `nosniff`、`Content-Security-Policy: sandbox`（无 allow-same-origin/allow-scripts）、`Cache-Control: no-store`。错误只返回简短文本/空体；无 token、XFO 或可信 UI 头。
- SVG 也受上述无脚本 sandbox；正文始终 `<img>` 使用，不能在可信 HTML 中内联。文件字节冒充图片不能获得 HTML 执行权限。
- 一个请求只读取一个条目；声明解压/压缩大小先检查，再以 `Read::take(cap + 1)` 限定实际输出并读尽合法页以触发 CRC 校验。
- 页子资源不提供 Range；原始归档 `?raw=1`/下载继续由 ServeFile 支持 Range。

## 7. 限额、并发与失败

| 预算 | 值 | 对照/原因 |
|---|---:|---|
| EPUB / FB2 原文件 | 32 MiB | 沿用 Office 输入上限；打开后及实际读取再次有界 |
| FB2 转码后的 XML | 64 MiB | 覆盖常见单字节编码转 UTF-8；仍有硬上限 |
| FB2 XML 节点 / 转换深度 | 200,000 / 128 | DOM 与递归映射预算；不声称硬进程内存隔离 |
| XML 初始容量估算 | `<`、`=` 各 200,000 | DOM 解析前检查，覆盖 CDATA/普通文本对预分配的放大 |
| 内联单图 / 内联资产总量 | 16 MiB / 24 MiB | 限制 data URI 放大；anydoc 本身最多保留 128 MiB 资产 |
| 生成 HTML | 64 MiB | 重复图片引用/文字转义也不能无限放大输出 |
| CBZ 原文件 | 512 MiB | 仅索引 + 单页读取，不套用整本 32 MiB 闸 |
| CBZ 条目 / central directory | 4,096 / 8 MiB | 在 zip 索引预分配之前检查 |
| CBZ 单页解压 / 压缩输入 | 32 MiB / 64 MiB | 同时限制输出分配与低输出大压缩流工作量 |

同步解析不在 Tokio worker 执行。普通文件读取用 `take(max + 1)`，不是只信 metadata 或 ZIP 声明。

- **文档转换门**：复用当前 `office_gate`，改名 `document_gate` 反映 Office/EPUB/FB2/CBZ 索引共享用途；容量 1，try-acquire，忙时不排队。permit 随真实 blocking job 持有至结束。
- **CBZ 图片门**：单独容量 2，在 async 层等待 permit，再创建 blocking job。正常 `<img>` 并发不能以“忙即 503”随机坏页。等待时不读 archive，不占 blocking worker；等待取消不启动 job。job 一旦开始，即使 handler 取消也持有 permit 到结束。
- 没有事务、数据库或多资源锁。锁序为：只获取一种 gate → 执行对应 job → 释放；不在持有 document_gate 时等待 page_gate，反之亦然。
- 不能把 timeout/abort 宣称为中止 spawn_blocking。慢文件系统仍会占容量，和现有 Office/search 同级；不为此新建 worker 服务。

条目出口保持 `download > raw > fragment > view > 默认协商`：

- Books 默认仅在 Accept 含 `text/html` 或 `?view=1` 时呈现；raw/download 不解析文件，返回原字节。
- Full 的解析失败/不支持/超限沿用 Office 回退 ServeFile；读取失败 404；忙 503 + Retry-After: 1。
- fragment 成功和任何失败只生成 `text/html; charset=utf-8` + nosniff；失败为可重试/可下载的安全占位，绝不把 ZIP/XML 原文塞入片段。忙占位同现有 Office。
- `.epub` 从 `office::format_for_path` 干净移交 books；不保留旧双路径。其他 Office/Markdown/Org/代码/媒体分派不变。

## 8. 验收 Seam 与证据

采用仓库已有公共 Seam，不测私有调用次数或源码字符串：

1. **`routes::app` HTTP 集成**：合法合成 EPUB/FB2/CBZ → Full/fragment/raw/download；检验内容、链接落点、图片原字节、状态和安全头。真实临时文件，无 mocked converter。
2. **实际二进制 + Chromium**：无 JS 独立阅读；列表/网格预览与树视图；图片 `naturalWidth > 0`、书内链接可定位、CBZ 数字页序和导航；恶意来源不能执行/访问父页面。
3. **既有测试与门禁**：cargo test；cargo fmt --check；cargo clippy --all-targets -- -D warnings；cargo build --locked --bins。平台不可用的检查必须明确报告，不冒充已验证。

必须覆盖的可证伪行为：

- EPUB 内嵌图片可见且 data URI 解码为原字节；跨章节/段落链接命中唯一 id；相同标题与源 id 碰撞不串跳；表格/列表/代码不因新 renderer 丢失。
- FB2 图片、notes body 引用、UTF-8 与 Windows-1251/UTF-16 输入；DTD 不触发网络或文件读取；恶意 scheme/markup 被禁；缺图明确占位。
- CBZ `1.bmp/2.jpg/10.png` 自然排序、内联 BMP、APNG/JPEG 后缀别名；嵌套目录、非图片/隐藏/目录/symlink 排除；page 读取校验成员和实际字节；多张并发图片全部完成而不因门忙丢失。浏览器覆盖无 JS、面板、树，实际点击书内链接并检查目标可见。
- CBZ `%2e%2e` 根穿越、平台路径、非法索引、伪 MIME、加密/不支持方法、EOCD 声明过量、实际解压超限与 CRC 错误 fail-closed。
- FB2 identifier-only DOCTYPE 仍可读；重复大内部实体、少节点但含大量 `<` 的 CDATA/含大量 `=` 的文本在 DOM 预分配前拒绝。CBZ comment 内另一 EOCD、末目录损坏且前部藏有超量目录的输入不能触发未校验索引分配。
- raw/download 优先级不回归；PDF/Office/HTML 既有行为不变；新书籍 Full 反嵌入，fragment 无 token，图片子资源无可信权限。
- 限额错误不冒充成功正文；取消等待与运行中 permit 生命周期沿用已有可测试 seam。

基线 EPUB 失败已有浏览器证据。实现时为该具体路径保留永久回归；子任务中不跑构建/格式化/测试，集成结束后统一运行，再针对实际失败修复。实际浏览器 smoke 不能由 Rust 测试替代。

## 9. 实现切片与交付

设计通过后才开始实现：

1. EPUB + 公共 renderer + Module Interface（拥有 mod.rs/html.rs）。
2. FB2 Adapter（拥有 fb2.rs），按既定 Document/Error Interface 对接。
3. CBZ（拥有 cbz.rs），按既定 Image/Error Interface 对接。
4. 主代理拥有 Cargo、lib/routes/render/Org 共用 helper、CSS、HTTP 集成测试与浏览器验收，避免共享文件多写者。

完成 smoke 后同步 README、未发布变更、第三方 notices 与仓库行为契约。评审日志记录独立 Standards/Spec 结论；每份设计或变更最多 7 轮，不收敛则明确阻断并交人工决策，不无限自动扩范围。通过后将本文件标为 accepted，完成实现/验收再记录实际结果。按当前 main 的普通 commit + push 发布，不 force push，不丢弃用户改动。
