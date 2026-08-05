//! 目录遍历与排序.

use std::path::Path;

use crate::meta::FileMeta;
use crate::sort::{SortBy, SortColumn, SortOrder};

/// 列出目录内容. 用安全构造器跳过无法读取的目录项 (悬空符号链接、权限不足等),
/// 避免单个坏文件导致整个目录列表 500.
pub fn list_dir(path: &Path, root: &Path, sort_by: SortBy) -> Vec<FileMeta> {
    let mut files: Vec<FileMeta> = match std::fs::read_dir(path) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| FileMeta::from(root, &e.path()))
            .collect(),
        Err(_) => Vec::new(),
    };
    match sort_by.column {
        SortColumn::Name => files.sort_by(|a, b| a.name.cmp(&b.name)),
        SortColumn::Size => files.sort_by_key(|f| f.size),
        SortColumn::Type => {
            files.sort_by(|a, b| (a.type_str(), &a.name).cmp(&(b.type_str(), &b.name)))
        }
        SortColumn::LastModified => files.sort_by_key(|f| f.last_modified_time),
        SortColumn::LastAccess => files.sort_by_key(|f| f.last_access_time),
        SortColumn::Creation => files.sort_by_key(|f| f.creation_time),
    }
    if sort_by.order == SortOrder::Desc {
        files.reverse();
    }
    files
}
