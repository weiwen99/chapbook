//! 目录遍历与排序, 以及确定性的递归文件名搜索核心.

use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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
        SortColumn::Name => files.sort_by(|a, b| a.display_name.cmp(&b.display_name)),
        SortColumn::Size => files.sort_by_key(|f| f.size),
        SortColumn::Type => files
            .sort_by(|a, b| (a.type_str(), &a.display_name).cmp(&(b.type_str(), &b.display_name))),
        SortColumn::LastModified => files.sort_by_key(|f| f.last_modified_time),
        SortColumn::LastAccess => files.sort_by_key(|f| f.last_access_time),
        SortColumn::Creation => files.sort_by_key(|f| f.creation_time),
    }
    if sort_by.order == SortOrder::Desc {
        files.reverse();
    }
    files
}

/// 搜索返回的最大结果数: 恰好 500 条不截断; 第 501 条匹配立即返回前 500 条并置 `truncated`.
pub const MAX_SEARCH_RESULTS: usize = 500;

/// 递归文件名搜索的结果: 按全局 BFS 顺序 (深度升序; 父级保持 BFS 序; 兄弟按原始文件名字节序)
/// 排列的匹配条目, 以及是否因达到 [`MAX_SEARCH_RESULTS`] 而截断.
pub struct SearchResult {
    pub entries: Vec<FileMeta>,
    pub truncated: bool,
}

/// 确定性的递归文件名搜索核心.
///
/// - 迭代式 BFS (`VecDeque<PathBuf>`), 绝不递归.
/// - 每个目录先收集所有可读 `DirEntry` (连同其 `OsString` 文件名, 各提取一次),
///   按原始文件名字节升序排序后再依序处理; Unix 上 `as_encoded_bytes()` 即原始字节,
///   其他平台是确定性的 WTF-8 顺序. 排序仅借用编码字节, 不在比较器内物化 `file_name`.
/// - 目录按 identity 去重 (Unix: `(dev, ino)`; 其他平台: 规范化路径), 只入队第一个
///   identity; 根目录 identity 在 metadata 成功且 identity 可得时预插入. 非 Unix 上
///   canonicalize 失败 ⇒ `None`, 拒绝入队该子树 (fail-closed), 绝不退回字面路径;
///   Unix `(dev, ino)` 恒可得 (infallible). 别名自身仍可能是结果, 即使其子树未被
///   入队. 普通文件从不进入 identity 集合, 硬链接的每个名字都是独立匹配.
/// - 每个成功构造的 `FileMeta` 独立匹配
///   `display_name.to_lowercase().contains(normalized_query)`, 包括目录与符号链接别名.
/// - 已跟随 metadata 失败的条目仍经 `FileMeta::from` 的既有符号链接回退产出可显示的
///   非目录条目; 但只有已跟随 metadata 能授权入队目录. 不可读子目录仅跳过该子树.
///
/// 调用方负责对查询做一次 trim/lowercase; 本函数不做逐候选归一化.
pub fn search(root: &Path, normalized_query: &str) -> SearchResult {
    if normalized_query.is_empty() {
        return SearchResult {
            entries: Vec::new(),
            truncated: false,
        };
    }
    let mut results: Vec<FileMeta> = Vec::new();
    // 根目录 identity 预插入 (仅当 identity 可得): 拦截 `loop -> .` / `parent -> ..`
    // 把根自身重新入队. 非 Unix 上 canonicalize 失败 ⇒ None ⇒ 不预插入 (fail-closed).
    let mut visited_dirs: HashSet<_> = HashSet::new();
    if let Ok(meta) = std::fs::metadata(root)
        && meta.is_dir()
        && let Some(identity) = dir_identity(root, &meta)
    {
        visited_dirs.insert(identity);
    }
    let mut queue: VecDeque<PathBuf> = VecDeque::from([root.to_path_buf()]);
    while let Some(dir_path) = queue.pop_front() {
        // 每个 DirEntry 连同其 OsString 文件名只收集一次; 排序仅借用编码字节,
        // 不在比较器内反复物化 file_name()
        let mut entries: Vec<(OsString, std::fs::DirEntry)> = match std::fs::read_dir(&dir_path) {
            Ok(read_dir) => read_dir
                .filter_map(|e| e.ok())
                .map(|e| (e.file_name(), e))
                .collect(),
            Err(err) => {
                // 只跳过该子树, 不影响其他分支
                tracing::debug!(
                    path = %dir_path.display(),
                    error = %err,
                    "search: skipping unreadable directory"
                );
                continue;
            }
        };
        // 兄弟目录项按原始文件名字节升序处理 (非 Unix 为确定性 WTF-8 编码序)
        entries.sort_by(|(a_name, _), (b_name, _)| {
            a_name.as_encoded_bytes().cmp(b_name.as_encoded_bytes())
        });
        for (file_name, _entry) in entries {
            let path = dir_path.join(&file_name);
            let followed = std::fs::metadata(&path);
            let Some(meta) = FileMeta::from(root, &path) else {
                continue;
            };
            if meta.display_name.to_lowercase().contains(normalized_query) {
                // 第 501 条匹配: 立即返回前 500 条, 不再扫描或入队
                if results.len() == MAX_SEARCH_RESULTS {
                    return SearchResult {
                        entries: results,
                        truncated: true,
                    };
                }
                results.push(meta);
            }
            // 只有已跟随 metadata 能授权入队目录; identity 不可得 (非 Unix
            // canonicalize 失败) 时拒绝入队该子树, 循环终止 fail-closed.
            // Unix (dev, ino) 恒可得, 语义不变.
            if let Ok(fm) = followed
                && fm.is_dir()
                && let Some(identity) = dir_identity(&path, &fm)
                && visited_dirs.insert(identity)
            {
                queue.push_back(path);
            }
        }
    }
    SearchResult {
        entries: results,
        truncated: false,
    }
}

/// 目录 identity seam: 统一返回 `Option`. 失败 (非 Unix 的 canonicalize) 时调用方
/// 必须拒绝入队该子树, 使循环终止 fail-closed; 绝不退回字面路径 (否则两个拼写可同时
/// 入队并无限循环). Unix 用跟随 metadata 的 `(dev, ino)`, 恒为 `Some` (infallible
/// 语义保持, 不引入失败路径).
#[cfg(unix)]
fn dir_identity(_path: &Path, meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

/// 非 Unix: 用规范化路径作为确定性目录 identity (无需新 crate). canonicalize 失败
/// (不存在、权限不足、符号链接环等) 时返回 `None`, 由调用方拒绝入队该子树.
#[cfg(not(unix))]
fn dir_identity(path: &Path, _meta: &std::fs::Metadata) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::Path;

    fn rels(result: &SearchResult) -> Vec<String> {
        result
            .entries
            .iter()
            .map(|m| m.relative_to_root.to_string_lossy().into_owned())
            .collect()
    }

    fn names(result: &SearchResult) -> Vec<String> {
        result
            .entries
            .iter()
            .map(|m| m.display_name.clone())
            .collect()
    }

    #[test]
    fn search_matches_case_insensitive_substring() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Alpha.txt"), b"x").unwrap();
        std::fs::write(root.join("alpine.md"), b"x").unwrap();
        std::fs::write(root.join("Beta.txt"), b"x").unwrap();
        let result = search(root, "al");
        assert!(!result.truncated);
        // 大小写不敏感子串: Alpha.txt 与 alpine.md; 字节序 0x41 < 0x61
        assert_eq!(rels(&result), ["Alpha.txt", "alpine.md"]);
        assert_eq!(names(&result), ["Alpha.txt", "alpine.md"]);
    }

    #[test]
    fn search_matches_directories_as_results() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("Target")).unwrap();
        std::fs::write(root.join("Target").join("inner.txt"), b"x").unwrap();
        let result = search(root, "target");
        assert!(!result.truncated);
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].is_directory);
        assert_eq!(result.entries[0].display_name, "Target");
        assert_eq!(result.entries[0].relative_to_root, Path::new("Target"));
    }

    #[test]
    fn search_is_breadth_first_byte_order_independent_of_creation_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 刻意按相反顺序创建: z-dir 先, a-dir 后; 结果必须无视创建顺序
        std::fs::create_dir(root.join("z-dir")).unwrap();
        std::fs::write(root.join("z-dir").join("q3.txt"), b"x").unwrap();
        std::fs::create_dir(root.join("a-dir")).unwrap();
        std::fs::write(root.join("a-dir").join("q2.txt"), b"x").unwrap();
        std::fs::write(root.join("q1.txt"), b"x").unwrap();
        let result = search(root, "q");
        assert!(!result.truncated);
        // 深度升序 (q1 先于 q2/q3); 同深度父级按 BFS 顺序 (a-dir 字节序先于 z-dir)
        assert_eq!(rels(&result), ["q1.txt", "a-dir/q2.txt", "z-dir/q3.txt"]);
    }

    #[test]
    fn search_empty_query_returns_empty_without_touching_root() {
        // root 不存在也必须是空结果: 证明空查询不读取根目录
        let result = search(Path::new("/nonexistent/chapbook-search-test"), "");
        assert!(result.entries.is_empty());
        assert!(!result.truncated);
        // 真实目录同样为空
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("anything.txt"), b"x").unwrap();
        let result = search(dir.path(), "");
        assert!(result.entries.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn search_exactly_500_matches_is_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..MAX_SEARCH_RESULTS {
            std::fs::write(root.join(format!("x-{i:03}.txt")), b"x").unwrap();
        }
        let result = search(root, "x");
        assert_eq!(result.entries.len(), MAX_SEARCH_RESULTS);
        assert!(!result.truncated);
        assert_eq!(result.entries[0].display_name, "x-000.txt");
        assert_eq!(
            result.entries[MAX_SEARCH_RESULTS - 1].display_name,
            "x-499.txt"
        );
    }

    #[test]
    fn search_501st_match_truncates_at_500() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..=MAX_SEARCH_RESULTS {
            std::fs::write(root.join(format!("x-{i:03}.txt")), b"x").unwrap();
        }
        let result = search(root, "x");
        assert_eq!(result.entries.len(), MAX_SEARCH_RESULTS);
        assert!(result.truncated);
        assert_eq!(result.entries[0].display_name, "x-000.txt");
        assert_eq!(
            result.entries[MAX_SEARCH_RESULTS - 1].display_name,
            "x-499.txt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_hard_links_keep_both_names() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("link-A.txt"), b"x").unwrap();
        std::fs::hard_link(root.join("link-A.txt"), root.join("link-B.txt")).unwrap();
        // 前提: 两个名字确为同一 inode
        assert_eq!(
            std::fs::metadata(root.join("link-A.txt")).unwrap().ino(),
            std::fs::metadata(root.join("link-B.txt")).unwrap().ino()
        );
        let result = search(root, "link");
        assert!(!result.truncated);
        // 普通文件不进 identity 集合: 硬链接的两个名字都是独立匹配
        assert_eq!(rels(&result), ["link-A.txt", "link-B.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn search_symlink_loop_terminates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("file1.txt"), b"x").unwrap();
        std::fs::write(root.join("sub").join("file2.txt"), b"x").unwrap();
        // sub/loop -> .  指向 sub 自身 (sub 已入队, identity 已记录)
        symlink(".", root.join("sub").join("loop")).unwrap();
        // sub/parent -> ..  指向 root (根 identity 已预插入)
        symlink("..", root.join("sub").join("parent")).unwrap();
        // 若任一循环被跟随, 遍历将无限进行, 测试会挂起; 通过即证明终止
        let result = search(root, "file");
        assert!(!result.truncated);
        assert_eq!(rels(&result), ["file1.txt", "sub/file2.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn search_aliases_shallowest_then_bytewise_first_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 真实目录 + 两个深度 1 别名 (字节序 a 先于 z) + 一个深度 2 别名
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target").join("file.txt"), b"x").unwrap();
        symlink("target", root.join("a-alias")).unwrap();
        symlink("target", root.join("z-alias")).unwrap();
        std::fs::create_dir(root.join("deep")).unwrap();
        symlink("../target", root.join("deep").join("d-alias")).unwrap();

        // 所有别名行本身都是匹配, 且按全局 BFS 顺序保留
        let result = search(root, "alias");
        assert!(!result.truncated);
        assert_eq!(rels(&result), ["a-alias", "z-alias", "deep/d-alias"]);
        assert!(result.entries.iter().all(|m| m.is_directory));

        // 子树 (target 目录内容) 只经最浅且字节序最靠前的 identity 展开一次:
        // 真实 target 与 a-alias 是同一 (dev, ino), a-alias 字节序先到先得,
        // z-alias 与 deep/d-alias 因 identity 去重不再展开
        let result = search(root, "file");
        assert!(!result.truncated);
        assert_eq!(rels(&result), ["a-alias/file.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn search_dangling_symlink_appears_as_non_directory_match() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        symlink("does-not-exist", root.join("broken-link")).unwrap();
        std::fs::write(root.join("other.txt"), b"x").unwrap();
        // 已跟随 metadata 失败时, FileMeta::from 的符号链接回退仍产出可显示的非目录条目
        let result = search(root, "broken");
        assert!(!result.truncated);
        assert_eq!(result.entries.len(), 1);
        assert!(!result.entries[0].is_directory);
        assert_eq!(result.entries[0].display_name, "broken-link");
        assert_eq!(result.entries[0].relative_to_root, Path::new("broken-link"));
        // 悬空链接不影响其他匹配
        let result = search(root, "other");
        assert_eq!(rels(&result), ["other.txt"]);
    }

    // 目录 identity seam: 返回 Option; 失败必须由调用方 fail-closed (拒绝入队),
    // 绝不退回字面路径. Unix (dev, ino) 恒可得 (infallible 语义保持).
    #[cfg(unix)]
    #[test]
    fn dir_identity_unix_is_infallible() {
        let dir = tempfile::tempdir().unwrap();
        let meta = std::fs::metadata(dir.path()).unwrap();
        assert!(dir_identity(dir.path(), &meta).is_some());
    }

    // 非 Unix: canonicalize 失败 (不存在的路径 ⇒ 必失败, 无平台竞态) 必须返回 None;
    // 真实目录 ⇒ Some.
    #[cfg(not(unix))]
    #[test]
    fn dir_identity_canonicalize_failure_is_none() {
        let dir = tempfile::tempdir().unwrap();
        // 非 Unix 分支忽略 metadata, 此处只需一个真实 Metadata 值
        let meta = std::fs::metadata(dir.path()).unwrap();
        assert!(dir_identity(Path::new("/nonexistent/chapbook-search-seam"), &meta).is_none());
        assert!(dir_identity(dir.path(), &meta).is_some());
    }
}
