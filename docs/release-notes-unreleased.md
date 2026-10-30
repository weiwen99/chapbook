# 未发布

## 书籍可读预览

- **EPUB**：改为直接渲染 anydoc Document，保留内嵌图片、跨章节/段落链接及基本结构，不再经过会丢失资产的 Markdown serializer。
- **FB2**：新增正文、封面/内嵌图片、源顺序 notes 与链接预览；支持严格声明编码解码，包括 UTF-8、UTF-16、Windows-1251。
- **CBZ**：新增自然页序、目录和前后页导航；支持单卷 ZIP32 Stored/Deflated、混合 PNG/JPEG/BMP 等浏览器图片与 Unicode Path extra fields。
- 三种格式均支持无 JS 独立阅读，以及既有列表/网格预览面板与树内容区；raw/download/Range、PDF 浏览器原生查看和 Office 管线保持原语义。
- 图片、XML、归档索引、实际解压与输出均有限额；CBZ 页独立等待式并发门，SVG 仅作隔离图片。超限/不支持时保留原文件，fragment 不回退不可信字节。

边界：不追原 CSS/分页版式，不支持 DRM、MOBI、XPS/OXPS、ODG、CBR/CBT 或 `.fb2.zip`；
不增加运行时外部工具、书签、标注或进度同步。

设计与实现均经两轮独立 Standards / Spec 评审通过；本机 HTTP 回归、仓库门禁和真实 Chromium
无 JS/面板/树验收已完成。详见[设计](../.scratch/book-previews/spec.md)、
[评审与验收记录](../.scratch/book-previews/review.md)及 [README 的能力/限额说明](../README.md#书籍预览)。

构建与浏览器行为验收入口见 [README](../README.md#development)。
