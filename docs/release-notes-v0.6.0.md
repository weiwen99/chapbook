# chapbook v0.6.0 — EPUB、FB2 与 CBZ 书籍预览

新增 FB2、CBZ 可读预览，修复 EPUB 的内嵌图片与书内跳转。三种格式均由服务端生成内容：
无 JavaScript 也能独立阅读，启用 JavaScript 后继续使用现有列表／网格预览面板和树内容区。
保持单二进制分发、零运行时外部工具依赖。

## EPUB：修复图片与书内链接

- 直接渲染 anydoc Document，不再经过会丢失图片资产的 Markdown 转换。
- 保留内嵌图片、跨章节／段落链接，重复标题与源 ID 使用唯一目标，避免串跳。
- 保留基本列表、表格和代码结构；修正跨表头 rowspan、零／负数起点列表的显示。

## FB2：新增正文、图片与注释预览

- 支持正文、封面／内嵌图片、基本样式、表格、诗歌与注释链接。
- 各 body（含 notes）保持源文件顺序；百分号编码的图片和注释引用恰好解码一次。
- 严格按 BOM／XML 声明解码，支持 UTF-8、UTF-16、Windows-1251 等编码；缺图显示占位文字。

## CBZ：新增漫画图片页预览

- 按完整成员名自然排序，提供页目录和前后页链接；支持嵌套目录与 Unicode Path extra fields。
- 支持 PNG/APNG、JPEG、GIF、BMP、WebP、AVIF、ICO、SVG，实际图片解码能力取决于浏览器。
- 忽略隐藏成员、`__MACOSX`、目录、符号链接和非图片；页图片按需读取，不解包到磁盘。
- 多张图片请求通过独立的两任务并发门等待，不因转换门忙而直接丢页；无 JS 时浏览器可能请求全部图片。

## 支持边界与安全

| 格式 | 主要限制 |
| --- | --- |
| EPUB / FB2 | 源文件最多 32 MiB；内嵌图片单张最多 16 MiB、去重后合计最多 24 MiB |
| CBZ | 单卷 ZIP32、Stored/Deflated；归档最多 512 MiB、4,096 个成员；单页解压后最多 32 MiB |

- XML、归档目录、实际解压量及输出均有限额；拒绝 XML 内部实体展开，关闭外部实体解析。
- SVG 只作为图片显示，不注入正文 DOM；CBZ 图片资源携带隔离 CSP、`nosniff` 和 `no-store`。
- 格式不支持、损坏或超限时保留原文件；fragment 返回安全占位，不回退不可信原始字节。
- 本次提供可读预览，不复刻原 CSS／固定分页版式，不新增书签、标注或阅读进度同步。
- 不包含 DRM、MOBI、XPS/OXPS、ODG、CBR/CBT 或 `.fb2.zip`。

完整限额与行为约定见 [README](https://github.com/weiwen99/chapbook/blob/v0.6.0/README.md#书籍预览)。

## 兼容与升级

- CLI 参数不变；`?raw=1`、`?download=1` 和 Range 保留原始文件语义。
- PDF 仍使用浏览器原生查看器；Markdown、Org、Office/CSV、代码和 HTML 预览保持既有管线。
- Windows 继续提供原生 CLI 与可选图形启动器；两个程序均可独立运行。
- 升级只需替换二进制并重启原有进程，无配置或数据迁移。

## 发布资产

| 平台 | 安装包 |
| --- | --- |
| Linux x86_64（musl 静态链接） | `chapbook-x86_64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `chapbook-aarch64-apple-darwin.tar.gz` |
| Windows x86_64（GNU） | `chapbook-x86_64-pc-windows-gnu.zip` |

每个平台提供 SHA256 校验文件。Windows zip 同时包含 `chapbook.exe` 与 `chapbook-launcher.exe`。
升级后运行 `chapbook --version`，应返回 `chapbook 0.6.0`。
