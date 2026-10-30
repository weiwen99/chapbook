# 书籍预览评审记录

基线：`d8ea4c4`。设计：[spec.md](spec.md)。设计与实现分别最多 7 轮；以下 verdict 是实际独立评审结果，不等于运行验收。

## 设计第 1 轮 — FAIL

2026-10-30，独立 reviewer：`DesignStandards`（安全/维护性）与 `DesignSpec`（已确认需求/消费端）。两者均 FAIL；未运行构建、测试或浏览器。

| Gate | 证据与影响 | 最小修正 / PASS 条件 | 成本 `[INFERENCE]` |
|---|---|---|---|
| S1 / P1：XML 实体展开 | roxmltree 0.21.2 顶层实体引用计数重置，输入/节点限制不能限制展开文字；anydoc 既有 XML 不支持任意实体展开 | DOM 前拒绝内部 DTD subset；identifier-only SYSTEM 声明仍可读，外部 resolver 始终关闭；重复大实体应安全失败 | 0.5–1 人日；0 组件/表 |
| S2 / P1：ZIP 预检与索引脱节 | zip 8.6.0 可接受未到 EOF 的 EOCD，并在目录失败后向前寻找另一 footer；Known(0) 不禁止该 fallback | 仅把已验证目录快照和单一 footer 暴露给索引；校验实际目录记录范围，拒绝歧义 EOCD；comment/前部恶意目录不能绕过预算 | 1–2 人日；0 组件/表 |
| S3 / P2：XML 预分配 | roxmltree 初始 node/attribute Vec 容量来自 `<`/`=` 字面数量，不受 nodes_limit 控制 | DOM 前对两类字符各设 200,000 上限；少节点 CDATA/equals-heavy 输入须在预分配前失败 | 0.25–0.5 人日；0 组件/表 |
| P1 / P2：BMP 静默缺页 | 原草案白名单缺 BMP，既有 Org 支持它；不符合已确认的浏览器可解码图片范围 | 补 BMP、APNG/JPEG 常见后缀别名；混合 CBZ 1.bmp/2.jpg/10.png 保留三页、自然排序、MIME/字节正确；实际浏览器可见 | 0.5 人日；0 组件/表 |

修订已进入设计 §4.3、§5、§6.1、§7、§8；第 2 轮全部关闭。

非阻断约束：无 JS 时原生 lazy loading 可能失效、浏览器会请求全部页，但两任务等待门仍限制服务端并发；单页损坏在资源级失败，不要求提前解压整本；归档修改需 reload，不引入缓存/快照一致性协议。普通 fragment 锚点不会被现有浏览器 click handler 吞掉，最终验收仍需真实点击。

依据：[roxmltree tokenizer](https://docs.rs/roxmltree/0.21.2/src/roxmltree/tokenizer.rs.html)、[roxmltree parser](https://docs.rs/roxmltree/0.21.2/src/roxmltree/parse.rs.html)、锁定依赖 `zip-8.6.0/src/read/zip_archive.rs` 与 `src/spec.rs`；现有 `src/org.rs`、`src/routes.rs`、`assets/chapbook-browser.js`。

## 设计第 2 轮 — PASS，定稿

2026-10-30，原两个独立 reviewer 对修订稿复审：

- `DesignStandards`：PASS，阻断项 0，置信度 0.97。确认实体预检、DOM 预分配计数和绑定快照的 ZIP 索引入口分别关闭 S1/S2/S3；修正均局限于已计划 Module，没有新增平台。
- `DesignSpec`：PASS，阻断项 0，置信度 0.95。确认 BMP/别名和三表面实际浏览器验收关闭 P1；EPUB、FB2、CBZ 范围及响应契约未回退。

设计接受，开始按定稿切片实现。两份结论均为只读设计/源码评审，未冒充构建或运行验收；HTTP、浏览器及仓库门禁仍是实现交付条件。

## 实现第 1 轮 — FAIL

2026-10-30，独立 reviewer：`CodeStandards` 与 `CodeSpec`，均只读评审；基线仍为 `d8ea4c4`，包含未提交的新模块与测试。

### Standards

2 个 P2 阻断项：CBZ 把中央目录原始文件名字节与 zip 经 Unicode Path extra field 改写后的名字节比较，合法图片页误报 422；
EPUB 显式 marker label 没有抑制浏览器原生编号，零/负数起点列表出现重复编号。
其余 ZIP 快照/解压、XML 预分配/实体、转义和 permit 生命周期 gate 无新阻断。

### Spec

4 个 P2 阻断项：上述 Unicode Path 与 marker label 问题；表头 `thead`/`tbody` 人为切断跨界 rowspan，
使下一行内容落入错误列；FB2 未对 URI fragment 解码，百分号编码的注释和图片引用失效。
每项最小修正估计小于 0.25 人日、0 表、0 组件 `[INFERENCE]`，均在原范围内。

PASS 条件：Stored/Deflated Unicode Path 图片返回原字节；跨表头跨度保持第二列坐标；编码注释/图片正确解析且不双重解码；
显式列表只显示源编号。HTTP 回归复现 422/缺链，实际 Chromium 复现错列、重复编号与缺图后才修复。

## 实现第 2 轮 — PASS

### Standards

`CodeStandards`：PASS，剩余硬性违规 0、新发现 0，置信度 0.98。
确认取消跨表示的名字节比较后，索引/绑定快照/条目数/offset/method/size 校验仍在；
显式 marker 的样式仅作用于对应列表项；同一表格行组保留跨度和 `th` 语义；
FB2 三个引用消费点共用严格 UTF-8、恰好一次的 fragment 解码及 canonical binary key 去重。

### Spec

`CodeSpec`：PASS，剩余阻断项 0、范围外行为 0，置信度 0.99。四项原 finding 全部关闭，
原始成员名仍用于自然排序，源 id 不改写；新增公共 HTTP 回归覆盖 Stored/Deflated Unicode Path、
编码 notes/images 与含字面 `%` 的不同目标。

两轴分别通过；以上为源码/回归用例复审，不代替下面的实际运行证据。

## 运行验收 — PASS（macOS arm64）

- `cargo test --locked`：289 通过（含 16 项书籍 HTTP 集成）；真实临时 EPUB/FB2/CBZ，无 mocked converter。
- `cargo clippy --locked --all-targets -- -D warnings`、`cargo build --locked --bins`、`cargo fmt --check` 通过。
- 启动实际 `target/debug/chapbook`，使用独立临时目录与真实 Chromium。关闭 JS 时，EPUB 内嵌 PNG、
  FB2 Windows-1251/BMP、CBZ BMP/JPEG/PNG 均有实际像素宽度；点击跨章节、注释、前后页链接可见目标。
- 启用 JS 后在列表面板、网格键盘预览（选中后 `p`）及树内容区验证书籍内容、图片与跳转；
  网格文件名仍按既有语义打开独立阅读页。截图确认正文、漫画页和修正后的列表/表格实际显示。
- Unicode Path CBZ 页由 422 修复为 200，图片 `naturalWidth = 8`；编码 FB2 注释恢复可点击目标，图片恢复。
  rowspan 第二列的表头/数据横坐标一致；零/负数列表只显示源编号，编号与首段同一基线。
  这些版式检查覆盖无 JS Full、网格 fragment 和树 fragment。
- 恶意 SVG 作为漫画图片及直接打开资源时均未执行脚本；资源响应为 `image/svg+xml`，
  CSP `sandbox`、`nosniff`、`no-store`。既有 CSV 经 Office 管线仍显示原表格内容。
- HTTP/模块回归覆盖响应模式优先级、Range、超限、XML 内部实体/预分配、ZIP fallback/CRC/实际解压限额、
  编码路径、页等待并发和转换忙后的恢复；现有 Org/PDF/Office/HTML 回归保留。

本机未运行 Linux/Windows 构建或 Windows GUI 交互；平台构建交由现有 CI，不以 macOS 结果冒充实机验证。
最终未扩大支持范围，也未引入缓存、解包目录、外部工具或新前端解析器。

## 跨平台 CI 与测试样本修正

首次推送 `3fceef8` 的 [Rust CI](https://github.com/weiwen99/chapbook/actions/runs/41010740381)：
Linux、macOS 全部通过；Windows 默认 CLI 构建、154 项库测试、4 项启动器测试及 15 项书籍 HTTP 测试通过。
剩余书籍测试在创建 `漫画 ?#%.CBZ` 时报 `InvalidFilename`（Windows error 123），未进入请求处理。

仅修正测试样本的平台适用性：Windows 使用合法的中文、空格、`#`、`%` 文件名，Unix 仍保留 `?` 的
查询分隔符边界；自然排序、混合图片、过滤、资源字节与安全头的全部断言不变。生产实现未改。
各提交的平台门禁结果见其关联的 [Rust workflow](https://github.com/weiwen99/chapbook/actions/workflows/rust.yml)；
CI 的启动器交互回归不冒充手工 Windows GUI 验收。
