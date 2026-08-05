//! 文件元信息.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};

/// URL path segment 的编码集: 仅保留 RFC 3986 unreserved 字符.
/// 必须用 percent-encoding (空格 -> `%20`); 表单语义编码 (空格 -> `+`) 在 path segment 中是错误行为.
const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'=')
    .add(b'\'')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'|')
    .add(b'&')
    .add(b'+')
    .add(b',');

#[derive(Debug)]
pub struct FileMeta {
    /// 相对于 root 的路径
    pub relative_to_root: PathBuf,
    /// 目录名或者文件名
    pub name: String,
    /// 是否是目录
    pub is_directory: bool,
    /// 文件大小, 单位 Byte
    pub size: u64,
    pub last_modified_time: SystemTime,
    pub last_access_time: SystemTime,
    pub creation_time: SystemTime,
}

impl FileMeta {
    /// 安全地构造 FileMeta. 当某个目录项无法读取 (例如悬空符号链接、权限不足) 时返回 None,
    /// 避免单个坏文件导致整个目录列表失败.
    ///
    /// 默认 `fs::metadata` 会跟随符号链接, 对于悬空的符号链接 (例如 Emacs 的 `.#xxx` 锁文件)
    /// 会失败; 此时回退到读取链接自身的属性 (`symlink_metadata`).
    pub fn from(root: &Path, path: &Path) -> Option<FileMeta> {
        let attrs = std::fs::metadata(path)
            .or_else(|_| std::fs::symlink_metadata(path))
            .ok()?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        let relative_to_root = path.strip_prefix(root).ok()?.to_path_buf();
        // 部分文件系统不支持 birth time, 回退到 mtime, 再退到 epoch
        let creation_time = attrs
            .created()
            .ok()
            .or_else(|| attrs.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        Some(FileMeta {
            relative_to_root,
            name,
            is_directory: attrs.is_dir(),
            size: attrs.len(),
            last_modified_time: attrs.modified().ok()?,
            last_access_time: attrs.accessed().ok()?,
            creation_time,
        })
    }

    /// 人类可读的文件大小
    pub fn human_size(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;
        match self.size {
            s if s < KB => format!("{s} B"),
            s if s < MB => format!("{} KB", s / KB),
            s if s < GB => format!("{} MB", s / MB),
            s => format!("{} GB", s / GB),
        }
    }

    pub fn type_str(&self) -> &'static str {
        if self.is_directory {
            "Directory"
        } else {
            "File"
        }
    }

    /// URL 编码后的路径, 用于超链接. 将路径中的每个部分分别编码, 避免 x/y/z -> x%2Fy%2Fz.
    pub fn href(&self) -> String {
        let segments = self
            .relative_to_root
            .iter()
            .map(|s| {
                percent_encode(s.to_string_lossy().as_bytes(), PATH_SEGMENT_ENCODE_SET).to_string()
            })
            .collect::<Vec<_>>()
            .join("/");
        format!("/{segments}")
    }
}

/// 生成适合显示的时间日期 (yyyy-MM-dd HH:mm:ss, 本地时区)
pub fn format_time(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}
