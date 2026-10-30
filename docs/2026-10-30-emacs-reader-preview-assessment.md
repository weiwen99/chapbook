# chapbook 与 emacs-reader 格式预览能力评估

日期：2026-10-30。本文件保留实施前的比较基线与四项已确认决策。后续 EPUB 修补、FB2/CBZ 新增已按[定稿设计](../.scratch/book-previews/spec.md)实施，验收见[评审记录](../.scratch/book-previews/review.md)；下文缺口、规模与候选成本均指评估阶段。

## 1. 规模边界

| 维度 | 用户需求 | 本次产出 |
|---|---|---|
| 数据表 / 持久化结构 | 评估格式缺口，不要求新增 | 0 |
| 实现组件 | 比较支持路径，随后决定 | 0；不接入任何渲染器 |
| 配置项 | 未要求变更 | 0 |
| 工程师日 | 希望了解支持代价 | 仅给候选范围的粗估，不排实施工期 |
| 发布波次 | 决定后再实施 | 0 |
| 文档 | 有证据的评估与已确认术语 | 本文件及 GLOSSARY 的“可读预览”条目 |

`[FACT]` 为文档/源码直接证据，`[实测]` 为本次运行观察，`[INFERENCE]` 为推断或工程估算。估算不是交付承诺，未运行的候选库不写成已验证能力。

## 2. 结论与比较口径

- **明确缺少的格式预览：MOBI、FB2、XPS/OpenXPS、CBZ。** FB2 可能显示 XML 源码，但不等于书籍预览。
- **EPUB 是质量缺口，不是扩展名缺口。** 已能读正文，但本次样本的内嵌图片丢失、跨章节链接失效。
- **ODG 是声明缺口。** emacs-reader README 宣称支持，但 MuPDF 开源读取器与此声明不一致，不能据此认定它实际可用。
- PDF 已交给浏览器原生查看；DOCX/PPTX/XLSX、ODT/ODS/ODP 已有正文转换。emacs-reader 对 Office 的自述也是“快速查看文字”，不承诺原版式。
- **不必把“扩大格式覆盖”绑定到 MuPDF。** Rust 格式库、现有 anydoc 文档模型、MIT 的 foliate-js 都是可复用候选。MuPDF 的价值主要是统一页面渲染，尤其 XPS；代价是 AGPL/商业许可、构建或 WASM 集成。

以下为评估口径。第一轮已确认以“可读预览”为目标，该术语已记入 [GLOSSARY](../GLOSSARY.md)；区分其他维度不表示要求全部实现：

- **格式覆盖**：识别并呈现该格式的内容；只显示二进制、XML 源码或下载按钮不计入。
- **可读预览**：正文、图片和基本文档结构可读，书内导航不被无故丢弃；不要求复制完整阅读器功能。
- **版式预览**：对固定版式文件保留页面上的文字、图片与图形位置。EPUB 等可重排书籍不以固定页码一致为目标。

格式、呈现质量与阅读交互是三个不同维度。不能用“扩展名已列入”代替质量验收，也不能把 Emacs 的书签、阅读位置、旋转等全部暗中加入本次范围。

## 3. 对标基线与完整差距

chapbook：当前工作树，Cargo.toml 版本 0.5.0，Apache-2.0，anydoc 固定 0.1.8。emacs-reader：提交 [`4fca3aaedddeea7f672ee97e5a91ef3002240260`](https://codeberg.org/MonadicSheep/emacs-reader/commit/4fca3aaedddeea7f672ee97e5a91ef3002240260)，2026-10-21。

emacs-reader 的 [README](https://codeberg.org/MonadicSheep/emacs-reader/src/commit/4fca3aaedddeea7f672ee97e5a91ef3002240260/README.org#L31-L59) 列出 6 个主要格式族和 7 个 Office 类型；XPS/OpenXPS 按一个族计。实际 [自动关联常量](https://codeberg.org/MonadicSheep/emacs-reader/src/commit/4fca3aaedddeea7f672ee97e5a91ef3002240260/reader.el#L138-L142) 有 `xps`，没有 `oxps`。声明、自动关联与底层库能打开的格式不能混为一谈。

| 格式 | chapbook 当前能力 | 对标差距 |
|---|---|---|
| PDF | 浏览器原生新 tab；原字节与 Range；fragment 按约定占位 | 格式已覆盖；若要求留在内容区，是交互需求而非格式支持 |
| EPUB | anydoc → Markdown → 文档页；按 spine 顺序；标题生成 TOC | 内嵌图片、原始书内锚点存在实际损失；书籍 nav/NCX、CSS/固定布局不是当前管线保证 |
| MOBI | 无专用转换路径 | 缺正文/图片预览；旧 MOBI、KF8/AZW3、DRM 必须分开界定 |
| FB2 | 满足 UTF-8/大小条件时是纯文本 XML | 缺正文、图片、注释与书内导航的结构化呈现 |
| XPS / OpenXPS | 无专用转换路径 | 缺固定页面渲染；只抽文字不能视为版式对齐 |
| CBZ | ZIP 漫画包无专用预览；独立图片已有预览 | 缺包内页列表、按页读图、顺序/翻页与归档资源限额 |
| DOCX / PPTX / XLSX | anydoc 正文/表格转换 | 已覆盖对标的“文字速览”档；不表示图片、幻灯片布局或图表保真 |
| ODT / ODS / ODP | anydoc 正文/表格转换 | chapbook 已覆盖；不要为了统一 MuPDF 丢掉现有路径 |
| ODG | 无专用转换路径 | emacs-reader 声明支持，但开源 MuPDF 读取能力未证实，见下文 |

格式表依据：[src/office.rs](../src/office.rs)、[src/routes.rs](../src/routes.rs)、[anydoc 0.1.8 Format](https://docs.rs/anydoc/0.1.8/anydoc/enum.Format.html)。chapbook 还支持旧 DOC/PPT/XLS、RTF、CSV、Markdown、Org、源代码、HTML 及浏览器可播放媒体；不是整体上落后于 emacs-reader。

### 上游声明的两个限制

1. **ODF 不一致。** MuPDF 1.26.0 的 [office handler](https://github.com/ArtifexSoftware/mupdf/blob/1.26.0/source/html/office.c#L1255-L1342) 只注册 DOCX/XLSX/PPTX/HWPX，内容识别同样不含 ODF；[全局 handler 表](https://github.com/ArtifexSoftware/mupdf/blob/1.26.0/source/fitz/document-all.c#L24-L76) 无独立 ODF 读取器。其 [CHANGES](https://github.com/ArtifexSoftware/mupdf/blob/1.26.0/CHANGES#L196-L214) 把 SmartOffice 商业插件与开源 OOXML 文本读取分开，并明确后者不保留原版式。`[INFERENCE]` 普通开源 MuPDF 构建不足以兑现 README 的 ODG 声明；本次没有运行 Emacs 验证具体报错。
2. **阅读能力并非全部更强。** emacs-reader 用位图页显示内容，提供跳页、缩放、旋转、outline，以及 Emacs 书签/阅读位置集成；当前 [C 模块接口](https://codeberg.org/MonadicSheep/emacs-reader/src/commit/4fca3aaedddeea7f672ee97e5a91ef3002240260/render/render-core.c#L1159-L1210) 没有全文搜索、文本选择、链接点击或密码接口。不能把 MuPDF 本身有的 API 当作 emacs-reader 已提供的功能。参见 [reader-bookmark.el](https://codeberg.org/MonadicSheep/emacs-reader/src/commit/4fca3aaedddeea7f672ee97e5a91ef3002240260/reader-bookmark.el)、[reader-saveplace.el](https://codeberg.org/MonadicSheep/emacs-reader/src/commit/4fca3aaedddeea7f672ee97e5a91ef3002240260/reader-saveplace.el)。

DjVu、CBR、AZW/AZW3 不在 emacs-reader 的上述正式声明范围内。CBR 在 MuPDF 中受可选 libarchive 构建控制，不能因为支持 CBZ 就连带承诺 CBR。MuPDF 官方还限定 [EPUB 为无 DRM 的 2.0、3.0 有限支持](https://mupdf.readthedocs.io/en/latest/guide/what-is-mupdf.html)。

## 4. 本次运行证据与 EPUB 缺口根因

### 实际运行

`cargo build --bin chapbook` 成功。以当前 debug 二进制、loopback、临时根目录启动；用 Chromium 打开页面并检查实际 DOM/响应。样本为自行构造，非真实书籍兼容性语料。

| 场景 | 观察 |
|---|---|
| 两章节 EPUB2，含内嵌 PNG 和跨章节链接 | 正文和标题 TOC 正常；`.cb-doc img` 为 0，只显示图片 alt；链接 href 为 `#oebps-ch2-xhtml-destination`，对应 DOM id 不存在，页面可见被转义的 `<a id="..."></a>` |
| 同一 EPUB 的 fragment | 正文存在，图片仍缺失，锚点仍以文本出现 |
| FB2 XML | fragment 显示带行号 XML，不是书籍正文 |
| 两张 PNG 的 CBZ | fragment 返回安全的“不支持预览”占位 |
| 独立 PNG | fragment 有 `<img>`；预览面板截图可见图片 |
| PDF | fragment 保持占位；原文件 Range `bytes=0-7` 返回 206、`application/pdf`、`%PDF-1.4`，无 document sandbox CSP |

临时服务、浏览器 tab 与样本目录已清理。未运行测试套件、格式化或 lint：本次只有研究文档，没有实现改动。没有运行 emacs-reader、MuPDF 或候选电子书库；没有测 Windows/Linux、真实 MOBI/KF8、ODG、复杂 EPUB3 或峰值内存。

### EPUB 不需要重写解包器

- anydoc [EPUB parser](https://docs.rs/crate/anydoc/0.1.8/source/src/formats/epub/mod.rs) 已按 spine 解析章节，保留图片 bytes 与章节锚点。
- [Markdown serializer](https://docs.rs/crate/anydoc/0.1.8/source/src/render/markdown/inline.rs) 对内嵌 `ImageSource::Asset` 只输出 alt，对锚点输出 raw `<a id>`。
- chapbook [convert_office_document](../src/routes.rs) 只调用 `to_markdown_bytes`，拿不到 `Document.assets`；[markdown.rs](../src/markdown.rs) 的 `options.render.escape = true` 又把 raw 锚点转义。
- [anydoc::to_document](https://docs.rs/anydoc/0.1.8/anydoc/fn.to_document.html) 与 Document/Block/Inline/Asset 模型公开，但可配置 Markdown serializer 没有公开。`[INFERENCE]` 可复用现有解析结果补安全呈现/资产出口，或评估受控的依赖改动；不是加一个选项，也不必从零解析 EPUB。依赖升级、补丁均未获实施授权。
- **不能为了修锚点全局关闭 raw HTML 转义。** EPUB/Office 来源不因经过转换库就成为可信脚本。

## 5. 可复用路线与粗略成本

以下人日均为 `[INFERENCE]`：按熟悉仓库的单个工程师估算，含对应范围的实现、必要安全边界、回归与浏览器验收、使用说明；不含商业许可采购、独立安全审计、大型兼容语料建设。各行目标不同，不可相加成全部对齐报价。候选库只读过文档/源码，尚未接入验证。

### A. 延续 Rust 转换与浏览器原生显示

| 范围 | 粗估 | 主要工作与维护代价 |
|---|---:|---|
| EPUB 图片 + 安全书内锚点 | 5–8 人日 | 复用 anydoc model；资产呈现/URL、锚点与既有文档管线接合；不承诺原 CSS/分页 |
| CBZ：ZIP + 浏览器可解码静态图片 | 3–5 人日 | 复用图片显示；成员读取、页序、翻页、归档限额；不自动支持 CBR/TIFF/JPEG2000 |
| FB2：正文、图片、基本注释/链接 | 4–7 人日 | 可复用 MIT 的 `fb2` 模型；仍需呈现、图片引用关联、编码与 XML 资源限制 |
| 旧版无 DRM MOBI：正文与插图 | 5–10 人日 | 可评估 MIT 的 `mobi`；内容/图片接口与压缩变体需样本确认，不据此承诺 KF8 |
| ODG：仅提取文字 | 5–10 人日 | anydoc 不认 `office:drawing`，不能简单映射 ODP；丢图形布局，价值可能很低 |
| XPS/ODG 原版式渲染 | 不给可靠交付区间 | 本次未找到已验证的可直接替换 MuPDF 的完整 Rust 渲染器；自行实现不是“小补一个格式” |

复用候选：[fb2 0.4.4](https://docs.rs/crate/fb2/0.4.4)（MIT，2023-10-07 发布；模型不自动关联 binary/image、不验证 ID 唯一性），[mobi 0.8.0](https://docs.rs/crate/mobi/0.8.0)（MIT，2022-12-11 发布；提供内容与头部解析）。这是解析库，不是接入即完成的安全阅读器；发布日期说明升级节奏，不足以单独判定不可用。

XPS 选型的具体反例：[justpdf-formats 0.1.2](https://docs.rs/crate/justpdf-formats/0.1.2) 声明 MIT/Apache-2.0 与 XPS 支持，但其 [xps/mod.rs](https://docs.rs/crate/justpdf-formats/0.1.2/source/src/xps/mod.rs) 的 `render_page` 明确返回正确尺寸的白页，`to_pdf` 仅用 Courier 重排抽取文字。因此不能依据 crate 格式表就把它算作现成保版式渲染器；也不能由这一个样本推断整个 Rust 生态没有其他方案。

### B. 内嵌前端电子书库：foliate-js

[foliate-js](https://github.com/johnfactotum/foliate-js) 是 [MIT](https://github.com/johnfactotum/foliate-js/blob/main/LICENSE) 许可的纯 JS 库；声明支持 EPUB、MOBI、KF8/AZW3、FB2、CBZ。具有重排/固定布局渲染、TOC/导航、搜索等接口，不需要 Node 作为运行时，也不要求构建步骤。ZIP 适配采用 BSD-3-Clause 的 zip.js，KF8 压缩字体使用 MIT 的 fflate；PDF 适配是实验性的，此评估建议继续沿用浏览器原生 PDF。

**这是需要认真比较的路线，不是“只有 MuPDF 才能分页”。** 它的 [mobi.js](https://github.com/johnfactotum/foliate-js/blob/main/mobi.js) 有 KF8、组合 MOBI 与 HUFF/CDIC 分支；README 也明确 HUFF/CDIC 可能较慢。该能力比 MuPDF 简单 MOBI 读取器的承诺更广，但本次没有真实书运行验证。

`[INFERENCE]` EPUB/MOBI/FB2/CBZ 共用该库的接入量级约 **10–20 人日**，包含阅读表面与三种浏览视图的生命周期接合、错误/无 JS 退路、资源释放和不可信内容隔离；不是四种格式分别乘四。若安全隔离不能在现有部署边界内成立，区间失效，必须重新选路。

不能直接复制 demo，原因有明确证据：

- **安全边界不同。** [paginator.js](https://github.com/johnfactotum/foliate-js/blob/main/paginator.js) 与 [fixed-layout.js](https://github.com/johnfactotum/foliate-js/blob/main/fixed-layout.js) 使用 `sandbox="allow-same-origin allow-scripts"`，依赖父页 CSP 禁止书中脚本。chapbook 的原始 HTML 则刻意禁止 `allow-same-origin`。`[INFERENCE]` chapbook 同源还会服务用户 JS 文件，所以不能仅照抄上游的 `script-src 'self'` 示例；必须证明书中脚本不能执行或触达可信 UI。不能以放宽现有 raw HTML sandbox 来完成接入。
- **稳定性。** README 明说库尚无稳定发布、API 可随时改变；需要固定提交与维护本地适配，而不是追随 main 自动更新。
- **大文件。** README 的底层接口可以适配随机读取，但 [view.js](https://github.com/johnfactotum/foliate-js/blob/main/view.js) 默认 URL 路径会 `fetch` 后 `res.blob()`，不能宣传“默认 HTTP 按需读取”；真正利用现有 Range 需要适配 loader。
- **局域网 HTTP。** README 说明 IDPF 字体混淆还原默认依赖 Web Crypto SHA-1，要求 secure context。非 localhost 的普通 HTTP 分享需要替代实现或明确降级，不应要求用户为此安装额外服务。
- 不支持 XPS/ODG；依赖浏览器 JS；无 JS 时 EPUB 可保留当前正文路径，新格式至少仍能下载。是否接受新增格式必须启用 JS，是产品取舍。

### C. MuPDF 原生或 WASM

[MuPDF 官方文档](https://mupdf.readthedocs.io/en/latest/guide/what-is-mupdf.html) 明确支持 XPS/OpenXPS、EPUB、MOBI、FB2、CBZ 等；这是覆盖 XPS 的成熟候选。它不是无条件优于路线 A/B，也不解决 ODG 的已发现证据缺口。

| 路线 | 接入量级 `[INFERENCE]` | 主要代价 |
|---|---:|---|
| 官方 mupdf.js / WASM | 13–27 人日 | 同源内嵌资源、页显示/翻页/缩放/outline、浏览器内存、CSP 与脚本生命周期；AGPL 或商业许可 |
| 服务端 MuPDF 原生链接 | 18–36 人日 | Rust/C 绑定、跨平台构建、页请求/渲染生命周期、并发/内存边界、安全升级；AGPL 或商业许可 |

以上是共用引擎与基本页阅读的量级，不是每增加一种格式都重付一次；不包含标注、账号同步或完整阅读器复刻，也不应据此替换已工作的 anydoc/原生 PDF 路径。

- [mupdf-rs](https://github.com/messense/mupdf-rs) 是社区 Rust 绑定，自身 AGPL；原生构建还涉及 C/C++/libclang。静态集成可不依赖外部可执行程序，但是否真的保持目标平台单二进制需构建验证。
- [mupdf.js](https://github.com/ArtifexSoftware/mupdf.js) 为官方 JS/WASM 包。调研读取的 [1.28.1 发布包](https://registry.npmjs.org/mupdf/-/mupdf-1.28.1.tgz) 中 WASM 为 10,409,826 字节，预压缩 Brotli 为 3,632,923 字节；这是发布物大小，不是 chapbook 二进制增量或实测网络传输。
- 可信页目前 `script-src 'self'`；[CSP3](https://www.w3.org/TR/CSP3/#grammardef-wasm-unsafe-eval) 对 WebAssembly 编译有单独限制。`[INFERENCE]` WASM 接入需要显式处理 `'wasm-unsafe-eval'`、正确 MIME，以及实际使用 Worker 时的策略；不应放宽为一般 `'unsafe-eval'`。
- MuPDF 的 [MOBI 读取器](https://github.com/ArtifexSoftware/mupdf/blob/master/source/html/mobi.c) 支持范围有限；不能从“MOBI”扩展名推导 Kindle/KF8/DRM 全支持。DRM 处理不计入任何报价。

## 6. 许可证与资源安全的实际取舍

### 许可证

chapbook 当前 Apache-2.0。MuPDF 官方提供 [AGPL 与商业双许可](https://mupdf.readthedocs.io/en/latest/license.html)；[Artifex 的解释](https://artifex.com/licensing) 与 mupdf.js README 对分发/网络服务有源码披露要求。

`[INFERENCE]` 引入 AGPL 依赖后不能把组合分发物简单视为仍只有 Apache-2.0 义务；既有 Apache 部分也不因此自动改写版权许可。需要决定是否承担组合物的 AGPL 义务，或另获商业授权。购买 MuPDF 商业许可不自动覆盖社区 `mupdf-rs` 绑定自身的 AGPL。以上是技术选型风险，不是法律意见；商业报价未取得。

MIT/BSD 候选避免这一类强 copyleft 取舍，但仍需保留 notices 并核对所选依赖链。也不应复制 GPL 的 emacs-reader 代码来实现 Apache 项目。

### 复用既有安全模式，不预建平台

- 沿用 `ResponseMode` 优先级、严格 HTML fragment、编码后的文件 identity、原字节/下载出口；新增预览不能把失败二进制注入可信页面。
- 新增 ZIP/XML 路径应对照现有 anydoc 的大小/解压/节点限额，控制成员数量与解压字节，避免把包内路径当作任意文件系统路径；没有需要落盘的理由。不必为此先建立通用归档平台或持久化缓存。
- 复用现有阻塞任务并发门的模式。线程与 async timeout 不等于能强制终止任意 C 渲染；若选原生 MuPDF，不能假称已有硬内存/时间隔离。这是需量清的运行风险，不默认新增子进程服务。
- 对浏览器库，主要风险转为文档脚本/外部资源隔离、客户端内存与销毁时资源释放；保留已有可信 UI 与不可信文件之间的边界。
- 当前 Office/EPUB 整包上限 32 MiB；它不是所有新格式应机械照搬的阈值。是否需要大型画册/按页加载，在选定格式后再定，不以扩格式为由放开所有限制。

## 7. 已确认决策

### 第一轮：已确认

1. **Q1：可读预览优先。** 正文、图片与基本文档结构优先，不复刻完整 emacs-reader；PDF 保留浏览器原生查看，Office 不追原版式，不自动加入书签、标注、阅读进度同步。
2. **Q2：保持许可与部署约束。** Apache-2.0、单二进制、零运行时外部工具为硬约束，允许内嵌前端 JS。MuPDF 与外部转换器退出本轮候选，Rust 与 MIT 的 foliate-js 仍可比较。

### 第二轮：已确认

3. **Q3：修补 EPUB，新增 CBZ 与 FB2。** MOBI、XPS/OpenXPS、ODG 不纳入本轮；原文件下载保持可用。

### 第三轮：已确认

4. **Q4：采用服务端可读预览。** 沿用服务端生成 HTML/图片入口的路径，复用 anydoc 与合适的 Rust 解析库，本轮不引入 foliate-js。独立阅读页在无 JS 时仍可读，启用 JS 时继续接入现有预览面板与树视图；不追原始 CSS、分页版式或完整阅读器交互。

已确认格式边界：EPUB 修补内嵌图片与书内锚点；CBZ 为 ZIP 容器中的浏览器可解码图片，不连带支持 CBR/CBT；FB2 为正文、图片与基本注释/链接，不连带承诺其他压缩包后缀。加密/DRM 文件、超限与损坏输入明确降级，保留原文件，不宣称全部变体兼容。具体资源阈值属于实现设计，不在本次评估中任意定值。

评估阶段以这四项决策结束；随后用户授权设计评审、实现、验收与提交推送，结果记录在上述设计与评审文档。选择延续既有服务端呈现模式，没有引入难以逆转的新技术路线，因此不新增 ADR。
