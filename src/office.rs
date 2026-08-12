//! Office 文档与 CSV 渲染 (anydoc 0.1, 纯 Rust, 无子进程).
//!
//! 管线: anydoc 把 doc/docx/odt/rtf/epub/ppt/xls/ods/csv 等转成 GFM markdown,
//! 再复用 `markdown::render` (comrak) 渲染为文档页 — TOC/标题锚点/代码高亮
//! 与 .md 完全一致 (anydoc 输出与手写 markdown 无差别).
//!
//! PDF 不在范围内: 浏览器原生打开 PDF, 渲染反而丢排版 (见
//! docs/2026-08-12-proposal-render-office-documents.org). anydoc 的 PDF 转换
//! (pdf-inspector) 也随格式表排除而不会进到转换路径.
//!
//! 提案: docs/2026-08-12-proposal-render-office-documents.org (方案 A).

use std::path::Path;

pub use anydoc::Format;

/// 超过该大小的 Office/CSV 文件不做转换, 直接走 ServeFile.
/// 转换是 CPU 活 (zip 解压 + 解析), 超大文件耗时无界; anydoc 内部另有
/// 解压比/嵌套深度/节点数限额兜底 (ResourceLimit), 此处是服务端第一道闸.
/// 代码文件同阈值逻辑见 `highlight::MAX_RENDER_BYTES` (1 MiB, 但文档页
/// 渲染比代码渲染便宜, 这里放宽到 32 MiB).
pub const MAX_RENDER_BYTES: u64 = 32 * 1024 * 1024;

/// 扩展名 -> anydoc 格式 (大小写不敏感, 表源为 anydoc::Format::from_extension).
/// 排除 PDF: 浏览器原生打开, 不转 markdown.
pub fn format_for_path(path: &Path) -> Option<Format> {
    Format::from_path(path).filter(|f| *f != Format::Pdf)
}
