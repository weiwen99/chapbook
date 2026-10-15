//! 目录遍历与排序, 以及确定性的递归文件名搜索核心.

use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::meta::FileMeta;
use crate::sort::{SortBy, SortColumn, SortOrder};

/// 所有条目出口共用的显示规则; 隐藏仅过滤浏览条目, 不限制直接访问.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListingOptions {
    pub sort_by: SortBy,
    pub show_hidden: bool,
}

fn visible_name(name: &OsStr, options: ListingOptions) -> bool {
    options.show_hidden || !name.as_encoded_bytes().starts_with(b".")
}

/// 目录优先, 主键按指定方向; 同值固定原始名称/完整相对路径升序.
/// 比较器只借用原始字节, 不物化名称或小写扩展名.
pub fn sort_entries(files: &mut [FileMeta], sort_by: SortBy) {
    files.sort_unstable_by(|a, b| {
        let grouping = b.is_directory.cmp(&a.is_directory);
        if grouping != Ordering::Equal {
            return grouping;
        }
        let primary = match sort_by.column {
            SortColumn::Name => a
                .raw_name()
                .as_encoded_bytes()
                .cmp(b.raw_name().as_encoded_bytes()),
            SortColumn::Type => match (a.extension(), b.extension()) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(a), Some(b)) => a
                    .as_encoded_bytes()
                    .iter()
                    .map(u8::to_ascii_lowercase)
                    .cmp(b.as_encoded_bytes().iter().map(u8::to_ascii_lowercase)),
            },
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::LastModified => a.last_modified_time.cmp(&b.last_modified_time),
            SortColumn::LastAccess => a.last_access_time.cmp(&b.last_access_time),
            SortColumn::Creation => a.creation_time.cmp(&b.creation_time),
        };
        let primary = if sort_by.order == SortOrder::Desc {
            primary.reverse()
        } else {
            primary
        };
        primary
            .then_with(|| {
                a.raw_name()
                    .as_encoded_bytes()
                    .cmp(b.raw_name().as_encoded_bytes())
            })
            .then_with(|| {
                a.relative_to_root
                    .as_os_str()
                    .as_encoded_bytes()
                    .cmp(b.relative_to_root.as_os_str().as_encoded_bytes())
            })
    });
}

/// 列出目录内容. 跳过隐藏或不可读条目, 不因单个坏文件让整个列表失败.
pub fn list_dir(path: &Path, root: &Path, options: ListingOptions) -> Vec<FileMeta> {
    let mut files: Vec<FileMeta> = match std::fs::read_dir(path) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|path| {
                visible_name(path.file_name().unwrap_or_else(|| OsStr::new("")), options)
            })
            .filter_map(|path| FileMeta::from(root, &path))
            .collect(),
        Err(_) => Vec::new(),
    };
    sort_entries(&mut files, options.sort_by);
    files
}

/// 搜索返回的最大结果数: 恰好 500 条不截断; 第 501 条匹配立即返回前 500 条并置 `truncated`.
pub const MAX_SEARCH_RESULTS: usize = 500;

/// 过滤后的确定性 BFS 候选集按显示规则排列, 并报告是否达到上限后仍有合格匹配.
pub struct SearchResult {
    pub entries: Vec<FileMeta>,
    pub truncated: bool,
}

/// 确定性的递归文件名搜索核心.
///
/// - 迭代式 BFS (`VecDeque<PathBuf>`), 绝不递归.
/// - 每个目录先收集可读条目的 `OsString` 文件名 (各提取一次),
///   按原始文件名字节升序排序后再依序处理; Unix 上 `as_encoded_bytes()` 即原始字节,
///   其他平台是确定性的 WTF-8 顺序. 排序仅借用编码字节, 不在比较器内物化 `file_name`.
/// - 目录按 identity 去重 (Unix: `(dev, ino)`; 其他平台: 规范化路径), 只入队第一个
///   identity; 根目录 identity 在 metadata 成功且 identity 可得时预插入. 非 Unix 上
///   canonicalize 失败 ⇒ `None`, 拒绝入队该子树 (fail-closed), 绝不退回字面路径;
///   Unix `(dev, ino)` 恒可得 (infallible). 别名自身仍可能是结果, 即使其子树未被
///   入队. 普通文件从不进入 identity 集合, 硬链接的每个名字都是独立匹配.
/// - 每个成功构造的 `FileMeta` 独立匹配
///   `display_name.to_lowercase().contains(normalized_query)`, 包括目录与符号链接别名.
/// - 跟随 metadata 失败时回退 symlink_metadata, 仍产出可显示的
///   非目录条目; 但只有已跟随 metadata 能授权入队目录. 不可读子目录仅跳过该子树.
///
/// 调用方负责对查询做一次 trim/lowercase; 本函数不做逐候选归一化.
pub fn search(root: &Path, normalized_query: &str, options: ListingOptions) -> SearchResult {
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
        // 只保留排序需要的原始名字; 路径按需 join, 不保留无用的 DirEntry.
        let mut entries: Vec<OsString> = match std::fs::read_dir(&dir_path) {
            Ok(read_dir) => read_dir
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .filter(|name| visible_name(name, options))
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
        entries.sort_unstable_by(|a_name, b_name| {
            a_name.as_encoded_bytes().cmp(b_name.as_encoded_bytes())
        });
        for file_name in entries {
            let path = dir_path.join(&file_name);
            let followed = std::fs::metadata(&path);
            let may_descend = followed.is_ok();
            let Ok(attrs) = followed.or_else(|_| std::fs::symlink_metadata(&path)) else {
                continue;
            };
            let Some(meta) = FileMeta::from_metadata(root, &path, &attrs) else {
                continue;
            };
            if meta.display_name.to_lowercase().contains(normalized_query) {
                // 第 501 条匹配: 立即返回前 500 条, 不再扫描或入队
                if results.len() == MAX_SEARCH_RESULTS {
                    sort_entries(&mut results, options.sort_by);
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
            if may_descend
                && attrs.is_dir()
                && let Some(identity) = dir_identity(&path, &attrs)
                && visited_dirs.insert(identity)
            {
                queue.push_back(path);
            }
        }
    }
    sort_entries(&mut results, options.sort_by);
    SearchResult {
        entries: results,
        truncated: false,
    }
}

/// 目录 identity (seam): 搜索遍历去重与树端点环检测共用.
/// Unix 为跟随 metadata 的 `(dev, ino)`; 其他平台为规范化路径.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DirIdentity {
    #[cfg(unix)]
    DevIno(u64, u64),
    #[cfg(not(unix))]
    Canonical(PathBuf),
}

/// 目录 identity seam: 统一返回 `Option`. 失败 (非 Unix 的 canonicalize) 时调用方
/// 必须 fail-closed (拒绝入队该子树 / 标记不可展开); 绝不退回字面路径 (否则两个拼写
/// 可同时入队并无限循环). Unix 用跟随 metadata 的 `(dev, ino)`, 恒为 `Some`
/// (infallible 语义保持, 不引入失败路径).
pub(crate) fn dir_identity(path: &Path, meta: &std::fs::Metadata) -> Option<DirIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Some(DirIdentity::DevIno(meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        std::fs::canonicalize(path).ok().map(DirIdentity::Canonical)
    }
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

    fn sortable_entries() -> Vec<FileMeta> {
        use std::time::{Duration, SystemTime};
        [
            ("z.txt", false, 2),
            ("two/same.md", false, 3),
            ("z-dir", true, 2),
            ("a.txt", false, 1),
            ("one/same.md", false, 3),
            ("a-dir", true, 1),
        ]
        .into_iter()
        .map(|(path, is_directory, rank)| {
            let path = PathBuf::from(path);
            let (modified, accessed, created) = if is_directory {
                (rank, rank, rank)
            } else {
                match rank {
                    1 => (3, 3, 2),
                    2 => (1, 2, 1),
                    _ => (2, 1, 3),
                }
            };
            let time = |n| SystemTime::UNIX_EPOCH + Duration::from_secs(n);
            FileMeta {
                display_name: crate::meta::escape_os_name(path.file_name().unwrap()),
                relative_to_root: path,
                is_directory,
                size: rank,
                last_modified_time: time(modified),
                last_access_time: time(accessed),
                creation_time: time(created),
            }
        })
        .collect()
    }

    #[test]
    fn all_sort_keys_keep_directories_first_and_fixed_ties() {
        let cases = [
            (
                SortColumn::Name,
                ["a.txt", "one/same.md", "two/same.md", "z.txt"],
                ["z.txt", "one/same.md", "two/same.md", "a.txt"],
            ),
            (
                SortColumn::Type,
                ["one/same.md", "two/same.md", "a.txt", "z.txt"],
                ["a.txt", "z.txt", "one/same.md", "two/same.md"],
            ),
            (
                SortColumn::Size,
                ["a.txt", "z.txt", "one/same.md", "two/same.md"],
                ["one/same.md", "two/same.md", "z.txt", "a.txt"],
            ),
            (
                SortColumn::LastModified,
                ["z.txt", "one/same.md", "two/same.md", "a.txt"],
                ["a.txt", "one/same.md", "two/same.md", "z.txt"],
            ),
            (
                SortColumn::LastAccess,
                ["one/same.md", "two/same.md", "z.txt", "a.txt"],
                ["a.txt", "z.txt", "one/same.md", "two/same.md"],
            ),
            (
                SortColumn::Creation,
                ["z.txt", "a.txt", "one/same.md", "two/same.md"],
                ["one/same.md", "two/same.md", "a.txt", "z.txt"],
            ),
        ];
        for (column, asc, desc) in cases {
            for (order, expected_files) in [(SortOrder::Asc, asc), (SortOrder::Desc, desc)] {
                let mut entries = sortable_entries();
                sort_entries(&mut entries, SortBy { column, order });
                let directories = if order == SortOrder::Desc && column != SortColumn::Type {
                    ["z-dir", "a-dir"]
                } else {
                    ["a-dir", "z-dir"]
                };
                let expected: Vec<_> = directories.into_iter().chain(expected_files).collect();
                let actual: Vec<_> = entries
                    .iter()
                    .map(|entry| entry.relative_to_root.to_str().unwrap())
                    .collect();
                assert_eq!(actual, expected, "{column}:{order}");
            }
        }
    }

    #[test]
    fn type_groups_use_last_extension_and_ascii_case_insensitive_ties() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "no-extension",
            ".env",
            "b.txt",
            "a.TXT",
            "a.tar.gz",
            "z.md",
            "trailing.",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("folder.pdf")).unwrap();
        let entries = list_dir(
            dir.path(),
            dir.path(),
            ListingOptions {
                sort_by: SortBy {
                    column: SortColumn::Type,
                    order: SortOrder::Asc,
                },
                show_hidden: true,
            },
        );
        let names: Vec<_> = entries
            .iter()
            .map(|entry| entry.raw_name().to_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "folder.pdf",
                ".env",
                "no-extension",
                "trailing.",
                "a.tar.gz",
                "z.md",
                "a.TXT",
                "b.txt"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn name_order_uses_raw_bytes_not_visible_escapes() {
        let dir = tempfile::tempdir().unwrap();
        // 合法但显示时必须转义的名称，同样能区分 raw bytes 与 escape 排序；
        // APFS 不允许创建含 0xFF 的文件名。
        let escaped = OsStr::new("\u{200b}");
        std::fs::write(dir.path().join(escaped), b"x").unwrap();
        std::fs::write(dir.path().join("z"), b"x").unwrap();
        let entries = list_dir(dir.path(), dir.path(), ListingOptions::default());
        assert_eq!(entries[0].raw_name(), OsStr::new("z"));
        assert_eq!(entries[1].raw_name(), escaped);
        assert_eq!(entries[1].display_name, "\\u{200B}");
    }

    #[cfg(unix)]
    #[test]
    fn hidden_directory_does_not_claim_visible_alias_identity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".target")).unwrap();
        std::fs::write(dir.path().join(".target/needle.txt"), b"x").unwrap();
        symlink(".target", dir.path().join("visible")).unwrap();
        let result = search(dir.path(), "needle", ListingOptions::default());
        assert_eq!(rels(&result), ["visible/needle.txt"]);
        let direct = list_dir(
            &dir.path().join(".target"),
            dir.path(),
            ListingOptions::default(),
        );
        assert_eq!(direct[0].raw_name(), OsStr::new("needle.txt"));
    }

    #[test]
    fn search_matches_case_insensitive_substring() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Alpha.txt"), b"x").unwrap();
        std::fs::write(root.join("alpine.md"), b"x").unwrap();
        std::fs::write(root.join("Beta.txt"), b"x").unwrap();
        let result = search(root, "al", ListingOptions::default());
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
        let result = search(root, "target", ListingOptions::default());
        assert!(!result.truncated);
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].is_directory);
        assert_eq!(result.entries[0].display_name, "Target");
        assert_eq!(result.entries[0].relative_to_root, Path::new("Target"));
    }

    #[test]
    fn search_sorts_bfs_candidates_independent_of_creation_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 刻意按相反顺序创建: z-dir 先, a-dir 后; 结果必须无视创建顺序
        std::fs::create_dir(root.join("z-dir")).unwrap();
        std::fs::write(root.join("z-dir").join("q3.txt"), b"x").unwrap();
        std::fs::create_dir(root.join("a-dir")).unwrap();
        std::fs::write(root.join("a-dir").join("q2.txt"), b"x").unwrap();
        std::fs::write(root.join("q1.txt"), b"x").unwrap();
        let result = search(root, "q", ListingOptions::default());
        assert!(!result.truncated);
        // 候选集从 byte-ordered BFS 收集, 此处按原始 basename 排列展示.
        assert_eq!(rels(&result), ["q1.txt", "a-dir/q2.txt", "z-dir/q3.txt"]);
    }

    #[test]
    fn search_empty_query_returns_empty_without_touching_root() {
        // root 不存在也必须是空结果: 证明空查询不读取根目录
        let result = search(
            Path::new("/nonexistent/chapbook-search-test"),
            "",
            ListingOptions::default(),
        );
        assert!(result.entries.is_empty());
        assert!(!result.truncated);
        // 真实目录同样为空
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("anything.txt"), b"x").unwrap();
        let result = search(dir.path(), "", ListingOptions::default());
        assert!(result.entries.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn search_exactly_500_matches_is_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..MAX_SEARCH_RESULTS {
            std::fs::write(root.join(format!("x-{i:03}.txt")), b"x").unwrap();
            std::fs::write(root.join(format!(".x-{i:03}.txt")), b"x").unwrap();
        }
        let result = search(root, "x", ListingOptions::default());
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
        let result = search(
            root,
            "x",
            ListingOptions {
                sort_by: SortBy {
                    column: SortColumn::Name,
                    order: SortOrder::Desc,
                },
                show_hidden: false,
            },
        );
        assert_eq!(result.entries.len(), MAX_SEARCH_RESULTS);
        assert!(result.truncated);
        // 候选集仍是 BFS 先收集的 x-000..499, 降序不能变成全局 top-N.
        assert_eq!(result.entries[0].display_name, "x-499.txt");
        assert_eq!(
            result.entries[MAX_SEARCH_RESULTS - 1].display_name,
            "x-000.txt"
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
        let result = search(root, "link", ListingOptions::default());
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
        let result = search(root, "file", ListingOptions::default());
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

        // 别名行全部保留, 展示按名称排序; 子树选择仍按 BFS identity 规则.
        let result = search(root, "alias", ListingOptions::default());
        assert!(!result.truncated);
        assert_eq!(rels(&result), ["a-alias", "deep/d-alias", "z-alias"]);
        assert!(result.entries.iter().all(|m| m.is_directory));

        // 子树 (target 目录内容) 只经最浅且字节序最靠前的 identity 展开一次:
        // 真实 target 与 a-alias 是同一 (dev, ino), a-alias 字节序先到先得,
        // z-alias 与 deep/d-alias 因 identity 去重不再展开
        let result = search(root, "file", ListingOptions::default());
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
        let result = search(root, "broken", ListingOptions::default());
        assert!(!result.truncated);
        assert_eq!(result.entries.len(), 1);
        assert!(!result.entries[0].is_directory);
        assert_eq!(result.entries[0].display_name, "broken-link");
        assert_eq!(result.entries[0].relative_to_root, Path::new("broken-link"));
        // 悬空链接不影响其他匹配
        let result = search(root, "other", ListingOptions::default());
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
