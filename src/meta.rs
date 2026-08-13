//! 文件元信息.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use maud::{Markup, html};
use percent_encoding::{AsciiSet, CONTROLS, percent_encode};

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
    /// 相对于 root 的路径; 唯一文件系统 identity, 始终保留原始 OsStr bytes, 绝不反推 Path
    pub relative_to_root: PathBuf,
    /// 目录名或者文件名; 仅用于显示/搜索文本, 经显示 codec 转义, 绝不用于反推 Path
    pub display_name: String,
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
        // display_name 只承载安全显示文本, 经显示 codec 转义, 绝不 lossy 转换
        let display_name = escape_os_name(path.file_name()?);
        let relative_to_root = path.strip_prefix(root).ok()?.to_path_buf();
        // 部分文件系统不支持 birth time, 回退到 mtime, 再退到 epoch
        let creation_time = attrs
            .created()
            .ok()
            .or_else(|| attrs.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        Some(FileMeta {
            relative_to_root,
            display_name,
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

    /// 整个 root-relative 路径的 UTF-8 视图; 任一祖先 segment 非 UTF-8 时返回 None,
    /// 表示该行 display-only, 不可通过浏览器操作.
    pub fn browser_path(&self) -> Option<&str> {
        self.relative_to_root.to_str()
    }

    /// 逐 segment RFC 3986 percent-encoding, 无前导斜杠, ASCII-only DOM transport.
    /// 仅在整个路径为 UTF-8 (browser_path 为 Some) 时编码.
    /// 委托共享编码器 [`encode_relative_path`], 不实现第二套 codec.
    pub fn browser_path_encoded(&self) -> Option<String> {
        encode_relative_path(&self.relative_to_root)
    }

    /// 在 encoded 值前加 `/`; 不实现第二套 path codec. root 返回 `Some("/")`.
    pub fn href(&self) -> Option<String> {
        self.browser_path_encoded()
            .map(|encoded| format!("/{encoded}"))
    }
}

/// 共享的 root-relative 路径编码器 (crate 可见, 唯一 relative-path codec):
/// 逐 segment RFC 3986 percent-encoding, segment 间保留 `/`, 无前导斜杠;
/// 任一 segment 非 UTF-8 时返回 None (整体不可浏览器操作). root (空路径) 返回
/// `Some("")` — 空字符串是合法 transport, 与属性缺失区分.
/// `FileMeta::browser_path_encoded` / 面包屑 / 目录 fragment wrapper / 搜索位置
/// 全部复用此函数, 不维护第二套 segment/path codec.
pub(crate) fn encode_relative_path(path: &Path) -> Option<String> {
    let mut out = String::new();
    for (i, seg) in path.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(&percent_encode_segment(seg.to_str()?));
    }
    Some(out)
}

/// RFC 3986 URL path segment 编码 (crate 可见, 供 render 面包屑等复用; 不对外暴露).
pub(crate) fn percent_encode_segment(segment: &str) -> String {
    percent_encode(segment.as_bytes(), PATH_SEGMENT_ENCODE_SET).to_string()
}

/// 显示 codec: 把单个 path segment 的原始 OsStr 转为安全显示文本.
/// 绝不使用 lossy 转换; 结果由 maud 再做 HTML 转义.
/// 输入为 `OsStr::as_encoded_bytes()`: Unix 是原始字节, Windows 是 WTF-8.
/// 严格 UTF-8 解码把每个非法字节逐字节转义 (`\xNN`) — 未配对代理等 WTF-8
/// 序列因此也逐编码字节转义, 每个不同输入得到不同显示文本, 周边合法字符
/// 原样保留; 而不是把整个名字坍缩成单一标记.
pub(crate) fn escape_os_name(name: &OsStr) -> String {
    escape_bytes(name.as_encoded_bytes())
}

/// 多 segment 路径的纯文本显示: 每个 segment 经 `escape_os_name` 后以 `/` join.
/// root (空路径) 返回空串, 由调用方决定显示形式.
pub(crate) fn display_path_text(path: &Path) -> String {
    let mut out = String::new();
    for (i, seg) in path.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(&escape_os_name(seg));
    }
    out
}

/// 逐 segment 显示: 精确输出 `<bdi dir="auto">escaped label</bdi>`.
/// 路径分隔符与其它 UI 元素必须保持在 bdi 之外.
pub(crate) fn display_segment(name: &OsStr) -> Markup {
    html! { bdi dir="auto" { (escape_os_name(name)) } }
}

/// 多 segment 路径的 markup 显示: 每 segment 独立 bdi 隔离 (display_segment),
/// 分隔符 `/` 是 bdi 外的普通文本节点. root (空路径) 输出空 markup.
pub(crate) fn display_path_markup(path: &Path) -> Markup {
    let segs: Vec<&OsStr> = path.iter().collect();
    html! {
        @for (i, seg) in segs.iter().enumerate() {
            @if i > 0 { "/" }
            (display_segment(seg))
        }
    }
}

/// 显示 codec 核心: 按原始 byte stream 增量解码 UTF-8.
/// 平台无关: Unix 喂 OsStr 原始字节, Windows 喂 WTF-8 字节.
/// 分支顺序固定:
///   1. U+0020 run 预扫描 (段内恰好一个的 interior 空格保留, 其余逐字节 `\x20`);
///   2. 非法 UTF-8 byte -> 大写 `\xNN`;
///   3. 字面反斜杠 -> `\\`;
///   4. 除 U+0020 外的 `char::is_whitespace()` -> 大写 `\u{NNNN}`;
///   5. 其余 ASCII control/DEL -> `\xNN`;
///   6. 其余 `char::is_control()` -> `\u{NNNN}`;
///   7. Default_Ignorable -> `\u{NNNN}`;
///   8. 其余合法 UTF-8 scalar 原样保留.
fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            // 恰好一个且不位于段首/段尾的 U+0020 原样保留
            if start > 0 && i < bytes.len() && i - start == 1 {
                out.push(' ');
            } else {
                for _ in 0..(i - start) {
                    out.push_str("\\x20");
                }
            }
            continue;
        }
        match utf8_scalar_at(bytes, i) {
            Some((c, n)) => {
                i += n;
                escape_scalar(c, &mut out);
            }
            None => {
                write!(out, "\\x{:02X}", bytes[i]).unwrap();
                i += 1;
            }
        }
    }
    out
}

fn escape_scalar(c: char, out: &mut String) {
    if c == '\\' {
        out.push_str("\\\\");
    } else if c != ' ' && c.is_whitespace() {
        push_u_escape(c as u32, out);
    } else if c.is_ascii_control() {
        write!(out, "\\x{:02X}", c as u32).unwrap();
    } else if c.is_control() || is_default_ignorable(c) {
        push_u_escape(c as u32, out);
    } else {
        out.push(c);
    }
}

/// 大写 hex、至少 4 位的 `\u{NNNN}` 转义
fn push_u_escape(cp: u32, out: &mut String) {
    write!(out, "\\u{{{cp:04X}}}").unwrap();
}

/// 在 `bytes[i..]` 处解码一个合法 UTF-8 scalar; 非法则返回 None.
/// 严格校验 overlong / surrogate / 超出 U+10FFFF.
fn utf8_scalar_at(bytes: &[u8], i: usize) -> Option<(char, usize)> {
    let b0 = *bytes.get(i)?;
    if b0 < 0x80 {
        return Some((b0 as char, 1));
    }
    let (len, mut cp) = match b0 {
        0xC2..=0xDF => (2, u32::from(b0 & 0x1F)),
        0xE0..=0xEF => (3, u32::from(b0 & 0x0F)),
        0xF0..=0xF4 => (4, u32::from(b0 & 0x07)),
        _ => return None,
    };
    if i + len > bytes.len() {
        return None;
    }
    for k in 1..len {
        let b = bytes[i + k];
        if b & 0xC0 != 0x80 {
            return None;
        }
        cp = (cp << 6) | u32::from(b & 0x3F);
    }
    // overlong / surrogate / 越界校验
    match len {
        3 if cp < 0x800 || (0xD800..=0xDFFF).contains(&cp) => return None,
        4 if !(0x10000..=0x10FFFF).contains(&cp) => return None,
        _ => {}
    }
    Some((char::from_u32(cp).unwrap(), len))
}

/// Unicode 17.0.0 `Default_Ignorable_Code_Point` (UCD DerivedCoreProperties.txt): 27 个源范围,
/// 共 4,174 个 scalar. 升级 Unicode 版本时必须按对应 DerivedCoreProperties.txt 更新
/// 此 matcher、总数与边界测试, 不跟随 Rust toolchain 隐式漂移.
fn is_default_ignorable(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD | 0x034F | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180D
            | 0x180E
            | 0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2065
            | 0x2066..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFF8
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000
            | 0xE0001
            | 0xE0002..=0xE001F
            | 0xE0020..=0xE007F
            | 0xE0080..=0xE00FF
            | 0xE0100..=0xE01EF
            | 0xE01F0..=0xE0FFF
    )
}

/// RFC 8187 Content-Disposition value: `attachment; filename="<fallback>"; filename*=UTF-8''<encoded>`.
/// - fallback 只保留 `[A-Za-z0-9._-]`, 其余 (含 `"`/`\`/非 ASCII) 一律替换为 `_`,
///   保证 quoted-string 内无引号/反斜杠, 且对任意字节输入确定性安全.
/// - filename* 对每个 UTF-8 scalar 的字节按 RFC 8187 attr-char 编码, 非 ASCII 恒编码.
/// - 非 Unicode basename (非法 UTF-8 字节, 或 Windows WTF-8 的未配对代理序列)
///   无法无损放入 `UTF-8''` 值域: 省略 filename*, fallback 由同一编码字节
///   逐字节派生 (`_`), 不同输入仍得到确定性不同输出, 绝不坍缩成空串.
pub(crate) fn content_disposition(basename: &OsStr) -> String {
    content_disposition_bytes(basename.as_encoded_bytes())
}

/// RFC 8187 attr-char: ALPHA / DIGIT / "!" / "#" / "$" / "&" / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
const RFC8187_ATTR_CHAR: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$&+-.^_`|~";

fn content_disposition_bytes(bytes: &[u8]) -> String {
    let mut fallback = String::new();
    let mut encoded = String::new();
    let mut valid_utf8 = true;
    let mut i = 0;
    while i < bytes.len() {
        match utf8_scalar_at(bytes, i) {
            Some((c, n)) => {
                i += n;
                if c.is_ascii() && matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-')
                {
                    fallback.push(c);
                } else {
                    fallback.push('_');
                }
                let mut buf = [0u8; 4];
                for &b in c.encode_utf8(&mut buf).as_bytes() {
                    if RFC8187_ATTR_CHAR.contains(&b) {
                        encoded.push(b as char);
                    } else {
                        write!(encoded, "%{b:02X}").unwrap();
                    }
                }
            }
            None => {
                // 非法 byte: fallback 逐字节 '_', 并整体省略 filename*
                fallback.push('_');
                valid_utf8 = false;
                i += 1;
            }
        }
    }
    let mut value = String::from("attachment; filename=\"");
    value.push_str(&fallback);
    value.push('"');
    if valid_utf8 {
        value.push_str("; filename*=UTF-8''");
        value.push_str(&encoded);
    }
    value
}

/// 生成适合显示的时间日期 (yyyy-MM-dd HH:mm:ss, 本地时区)
pub fn format_time(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::ffi::OsString;

    /// Unicode 17.0.0 Default_Ignorable_Code_Point 的 27 个源范围 (UCD DerivedCoreProperties.txt),
    /// 与生产 matcher 对照: 端点、相邻非成员与总数 4,174.
    const DI_RANGES: [(u32, u32); 27] = [
        (0x00AD, 0x00AD),
        (0x034F, 0x034F),
        (0x061C, 0x061C),
        (0x115F, 0x1160),
        (0x17B4, 0x17B5),
        (0x180B, 0x180D),
        (0x180E, 0x180E),
        (0x180F, 0x180F),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x2064),
        (0x2065, 0x2065),
        (0x2066, 0x206F),
        (0x3164, 0x3164),
        (0xFE00, 0xFE0F),
        (0xFEFF, 0xFEFF),
        (0xFFA0, 0xFFA0),
        (0xFFF0, 0xFFF8),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0000, 0xE0000),
        (0xE0001, 0xE0001),
        (0xE0002, 0xE001F),
        (0xE0020, 0xE007F),
        (0xE0080, 0xE00FF),
        (0xE0100, 0xE01EF),
        (0xE01F0, 0xE0FFF),
    ];

    fn meta(rel: &Path) -> FileMeta {
        FileMeta {
            relative_to_root: rel.to_path_buf(),
            display_name: String::new(),
            is_directory: false,
            size: 0,
            last_modified_time: SystemTime::UNIX_EPOCH,
            last_access_time: SystemTime::UNIX_EPOCH,
            creation_time: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn space_run_prescan_rules() {
        // 恰好一个且不在段首/段尾的 U+0020 保留
        assert_eq!(escape_os_name(OsStr::new("My File.txt")), "My File.txt");
        // 段首 / 段尾 / 长度 >= 2 的 run 逐字节转义
        assert_eq!(escape_os_name(OsStr::new(" file")), "\\x20file");
        assert_eq!(escape_os_name(OsStr::new("file ")), "file\\x20");
        assert_eq!(escape_os_name(OsStr::new("a  b")), "a\\x20\\x20b");
        assert_eq!(escape_os_name(OsStr::new("a   b")), "a\\x20\\x20\\x20b");
        assert_eq!(escape_os_name(OsStr::new(" ")), "\\x20");
        assert_eq!(escape_os_name(OsStr::new("  ")), "\\x20\\x20");
        assert_eq!(escape_os_name(OsStr::new("  a  ")), "\\x20\\x20a\\x20\\x20");
        assert_eq!(escape_os_name(OsStr::new("")), "");
    }

    #[test]
    fn all_25_unicode_white_space_values() {
        // Unicode 17.0 White_Space 的全部 25 个值
        let ws: &[char] = &[
            '\u{0009}', '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0020}', '\u{0085}',
            '\u{00A0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}',
            '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{2028}',
            '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}',
        ];
        assert_eq!(ws.len(), 25);
        // 锁定 Rust is_whitespace 与 Unicode White_Space 一致
        assert_eq!(
            (0u32..=0x10FFFF)
                .filter_map(char::from_u32)
                .filter(|c| c.is_whitespace())
                .count(),
            25
        );
        for &c in ws {
            assert!(c.is_whitespace());
            if c == ' ' {
                // U+0020 走空间 run 规则而不是 \u{} 转义
                assert_eq!(escape_os_name(OsStr::new("x x")), "x x");
                assert_eq!(escape_os_name(OsStr::new(" x")), "\\x20x");
                assert_eq!(escape_os_name(OsStr::new("x ")), "x\\x20");
            } else {
                let input = format!("x{c}x");
                let expected = format!("x\\u{{{:04X}}}x", c as u32);
                assert_eq!(escape_os_name(OsStr::new(&input)), expected);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn invalid_byte_vs_fffd_vs_literal_escape_collisions() {
        use std::os::unix::ffi::OsStrExt;
        // 原始非法字节 -> 大写 \xNN
        assert_eq!(escape_os_name(OsStr::from_bytes(b"a\xFFb")), "a\\xFFb");
        assert_eq!(escape_os_name(OsStr::from_bytes(b"\xFF\xFF")), "\\xFF\\xFF");
        // 截断多字节序列: 逐字节 \xNN
        assert_eq!(
            escape_os_name(OsStr::from_bytes(b"a\xE4\xB8b")),
            "a\\xE4\\xB8b"
        );
        // 合法 U+FFFD 原样保留
        assert_eq!(escape_os_name(OsStr::new("a\u{FFFD}b")), "a\u{FFFD}b");
        // 字面 "\xFF" / "\u{FFFD}" 先转义反斜杠, 不碰撞
        assert_eq!(escape_os_name(OsStr::new("a\\xFFb")), "a\\\\xFFb");
        assert_eq!(escape_os_name(OsStr::new("a\\u{FFFD}b")), "a\\\\u{FFFD}b");
    }

    #[test]
    fn literal_backslash_is_doubled() {
        assert_eq!(escape_os_name(OsStr::new("a\\b")), "a\\\\b");
        assert_eq!(escape_os_name(OsStr::new("\\")), "\\\\");
    }

    #[test]
    fn ascii_control_and_del_use_x_escapes() {
        assert_eq!(escape_os_name(OsStr::new("\u{0000}")), "\\x00");
        assert_eq!(escape_os_name(OsStr::new("\u{0001}")), "\\x01");
        assert_eq!(escape_os_name(OsStr::new("\u{001F}")), "\\x1F");
        assert_eq!(escape_os_name(OsStr::new("\u{007F}")), "\\x7F");
    }

    #[test]
    fn non_ascii_controls_use_u_escapes() {
        assert_eq!(escape_os_name(OsStr::new("\u{0080}")), "\\u{0080}");
        assert_eq!(escape_os_name(OsStr::new("\u{009F}")), "\\u{009F}");
    }

    #[test]
    fn whitespace_branches_before_control() {
        // U+0009-U+000D 与 U+0085 是 White_Space, 必须走 \u{} 分支而非 \xNN
        assert_eq!(escape_os_name(OsStr::new("\u{0009}x")), "\\u{0009}x");
        assert_eq!(escape_os_name(OsStr::new("\u{000D}x")), "\\u{000D}x");
        assert_eq!(escape_os_name(OsStr::new("\u{0085}x")), "\\u{0085}x");
    }

    #[test]
    fn arabic_and_hebrew_preserved() {
        assert_eq!(escape_os_name(OsStr::new("عربي")), "عربي");
        assert_eq!(escape_os_name(OsStr::new("עברית")), "עברית");
        assert_eq!(
            escape_os_name(OsStr::new("ملف-تجريبي.txt")),
            "ملف-تجريبي.txt"
        );
        assert_eq!(escape_os_name(OsStr::new("aעבb")), "aעבb");
    }

    #[test]
    fn default_ignorable_total_count_is_4174() {
        let count = (0u32..=0x10FFFF)
            .filter_map(char::from_u32)
            .filter(|&c| is_default_ignorable(c))
            .count();
        assert_eq!(count, 4174);
    }

    fn in_di_ranges(cp: u32) -> bool {
        DI_RANGES.iter().any(|&(lo, hi)| (lo..=hi).contains(&cp))
    }

    #[test]
    fn default_ignorable_range_endpoints_and_adjacent_nonmembers() {
        for (lo, hi) in DI_RANGES {
            let start = char::from_u32(lo).unwrap();
            let end = char::from_u32(hi).unwrap();
            assert!(is_default_ignorable(start), "range start U+{lo:04X}");
            assert!(is_default_ignorable(end), "range end U+{hi:04X}");
            // 相邻位置与生产 matcher 一致: 相邻区间 (如 180D/180E/180F) 本身是成员,
            // 真正落在 gap 里的相邻非成员必须不匹配
            if lo > 0 {
                let before = char::from_u32(lo - 1).unwrap();
                assert_eq!(
                    is_default_ignorable(before),
                    in_di_ranges(lo - 1),
                    "U+{:04X} neighbor of range start",
                    lo - 1
                );
            }
            if hi < 0x10FFFF {
                let after = char::from_u32(hi + 1).unwrap();
                assert_eq!(
                    is_default_ignorable(after),
                    in_di_ranges(hi + 1),
                    "U+{:04X} neighbor of range end",
                    hi + 1
                );
            }
        }
        // 关键 gap 非成员: 每个真实间隔处的相邻位置
        for gap in [
            0x00ACu32, 0x0350, 0x061D, 0x1161, 0x17B6, 0x1810, 0x2010, 0x202F, 0x205F, 0x2070,
            0x3165, 0xFE10, 0xFFA1, 0xFFF9, 0x1BCA4, 0x1D17B, 0xDFFFF, 0xE1000,
        ] {
            assert!(
                !is_default_ignorable(char::from_u32(gap).unwrap()),
                "U+{gap:04X}"
            );
        }
    }

    #[test]
    fn bidi_control_subset_is_default_ignorable() {
        // Unicode Bidi_Control 共 12 个, 全部落在 Default_Ignorable matcher 内
        const BIDI_CONTROL: &[u32] = &[
            0x061C, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2066, 0x2067, 0x2068,
            0x2069,
        ];
        assert_eq!(BIDI_CONTROL.len(), 12);
        for &cp in BIDI_CONTROL {
            assert!(
                is_default_ignorable(char::from_u32(cp).unwrap()),
                "U+{cp:04X}"
            );
        }
    }

    #[test]
    fn default_ignorables_are_escaped_in_display() {
        assert_eq!(escape_os_name(OsStr::new("a\u{200B}b")), "a\\u{200B}b");
        assert_eq!(escape_os_name(OsStr::new("\u{202E}evil")), "\\u{202E}evil");
        assert_eq!(escape_os_name(OsStr::new("a\u{FEFF}b")), "a\\u{FEFF}b");
        assert_eq!(escape_os_name(OsStr::new("a\u{00AD}b")), "a\\u{00AD}b");
        assert_eq!(escape_os_name(OsStr::new("a\u{180E}b")), "a\\u{180E}b");
        assert_eq!(escape_os_name(OsStr::new("a\u{E0000}b")), "a\\u{E0000}b");
    }

    #[cfg(unix)]
    #[test]
    fn display_codec_fixtures_do_not_collide() {
        use std::os::unix::ffi::OsStrExt;
        let cases: &[(&OsStr, &str)] = &[
            (OsStr::from_bytes(b"\xFF"), "\\xFF"),
            (OsStr::new("\u{FFFD}"), "\u{FFFD}"),
            (OsStr::new("\\xFF"), "\\\\xFF"),
            (OsStr::new("\u{00A0}"), "\\u{00A0}"),
            (OsStr::new("\\u{00A0}"), "\\\\u{00A0}"),
            (OsStr::new("\u{200B}"), "\\u{200B}"),
            (OsStr::new("\\u{200B}"), "\\\\u{200B}"),
            (OsStr::new("\u{202E}"), "\\u{202E}"),
            (OsStr::new("\\u{202E}"), "\\\\u{202E}"),
            (OsStr::new(" "), "\\x20"),
            (OsStr::new("\\x20"), "\\\\x20"),
        ];
        let outputs: Vec<String> = cases.iter().map(|(_, e)| e.to_string()).collect();
        for i in 0..outputs.len() {
            assert_eq!(escape_os_name(cases[i].0), outputs[i]);
            for j in i + 1..outputs.len() {
                assert_ne!(outputs[i], outputs[j]);
            }
        }
    }

    #[test]
    fn browser_path_is_exact_utf8_view() {
        assert_eq!(meta(Path::new("a/b.txt")).browser_path(), Some("a/b.txt"));
        assert_eq!(meta(Path::new("")).browser_path(), Some(""));
    }

    #[test]
    fn browser_path_encoded_and_href_exact_transport() {
        let cases: &[(&str, &str)] = &[
            ("a b/c", "a%20b/c"),
            ("a+b", "a%2Bb"),
            ("100%.txt", "100%25.txt"),
            ("a=b", "a%3Db"),
            ("a#b", "a%23b"),
            ("a?b", "a%3Fb"),
            ("a\r\nb", "a%0D%0Ab"),
            ("报告.md", "%E6%8A%A5%E5%91%8A.md"),
            ("dir/sub/x y", "dir/sub/x%20y"),
            ("a%2Fb", "a%252Fb"),
            ("a;b@c&d,e", "a%3Bb%40c%26d%2Ce"),
            ("", ""),
        ];
        for (path, encoded) in cases {
            let m = meta(Path::new(path));
            assert_eq!(m.browser_path_encoded(), Some(encoded.to_string()));
            assert_eq!(m.href(), Some(format!("/{encoded}")));
        }
    }

    #[cfg(unix)]
    #[test]
    fn backslash_in_segment_encoded_on_unix() {
        // Unix 上 `\` 是普通文件名 byte: 按 RFC 3986 segment 编码为 %5C
        let m = meta(Path::new("a\\b"));
        assert_eq!(m.browser_path_encoded(), Some("a%5Cb".to_string()));
        assert_eq!(m.href(), Some("/a%5Cb".to_string()));
        assert_eq!(display_path_text(Path::new("a\\b/c")), "a\\\\b/c");
    }

    #[cfg(windows)]
    #[test]
    fn windows_backslash_separates_components_in_browser_path() {
        // Windows 上 `\` 是路径分隔符: browser transport 必须按 `/` join
        let m = meta(Path::new("a\\b"));
        assert_eq!(m.browser_path_encoded(), Some("a/b".to_string()));
        assert_eq!(m.href(), Some("/a/b".to_string()));
        assert_eq!(display_path_text(Path::new("a\\b")), "a/b");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_have_no_browser_identity() {
        use std::os::unix::ffi::OsStringExt;
        let m = meta(&PathBuf::from(OsString::from_vec(b"a\xFFb".to_vec())));
        assert!(m.browser_path().is_none());
        assert!(m.browser_path_encoded().is_none());
        assert!(m.href().is_none());
        // 祖先 segment 非 UTF-8 (basename 本身是合法 UTF-8) 同样不可操作
        let m2 = meta(&PathBuf::from(OsString::from_vec(
            b"bad\xFF/leaf.txt".to_vec(),
        )));
        assert!(m2.browser_path().is_none());
        assert!(m2.browser_path_encoded().is_none());
        assert!(m2.href().is_none());
    }

    #[test]
    fn percent_encode_segment_is_rfc3986() {
        assert_eq!(percent_encode_segment("a b"), "a%20b");
        assert_eq!(percent_encode_segment("x/y"), "x%2Fy");
        assert_eq!(percent_encode_segment("a+b"), "a%2Bb");
        assert_eq!(percent_encode_segment("报告"), "%E6%8A%A5%E5%91%8A");
    }

    #[test]
    fn display_segment_markup_is_bdi_auto() {
        assert_eq!(
            display_segment(OsStr::new("My File.txt")).into_string(),
            "<bdi dir=\"auto\">My File.txt</bdi>"
        );
        assert_eq!(
            display_segment(OsStr::new("a<b&c")).into_string(),
            "<bdi dir=\"auto\">a&lt;b&amp;c</bdi>"
        );
        assert_eq!(
            display_segment(OsStr::new("עברית")).into_string(),
            "<bdi dir=\"auto\">עברית</bdi>"
        );
    }

    #[cfg(unix)]
    #[test]
    fn display_segment_escapes_invalid_bytes_before_markup() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            display_segment(OsStr::from_bytes(b"x\xFF")).into_string(),
            "<bdi dir=\"auto\">x\\xFF</bdi>"
        );
    }

    #[test]
    fn display_path_text_joins_escaped_segments() {
        assert_eq!(display_path_text(Path::new("a b/c")), "a b/c");
        assert_eq!(display_path_text(Path::new("dir/sub")), "dir/sub");
        assert_eq!(display_path_text(Path::new("")), "");
    }

    #[cfg(unix)]
    #[test]
    fn display_path_text_non_utf8_segment() {
        use std::os::unix::ffi::OsStringExt;
        let p = PathBuf::from(OsString::from_vec(b"x\xFF/y".to_vec()));
        assert_eq!(display_path_text(&p), "x\\xFF/y");
    }

    /// Windows OsStr 的 WTF-8 编码里, 未配对代理 (U+D800..U+DFFF) 以 3 字节序列
    /// (ED A0 80 .. ED BF BF) 表示. 显示 codec 必须逐编码字节转义 (输入可区分),
    /// 同时保留周边合法字符 — 而不是把整个名字坍缩成一个固定标记.
    /// `from_encoded_bytes_unchecked` 的输入: Unix 任意字节合法; Windows 需 WTF-8,
    /// 未配对代理序列是合法 WTF-8, 故两平台均满足安全契约.
    #[test]
    fn wtf8_unpaired_surrogates_are_input_distinguishing() {
        let d800 = unsafe { OsStr::from_encoded_bytes_unchecked(b"x\xED\xA0\x80y") };
        let dc00 = unsafe { OsStr::from_encoded_bytes_unchecked(b"x\xED\xB0\x80y") };
        let with_ascii = unsafe { OsStr::from_encoded_bytes_unchecked(b"x\xED\xA0\x80.txt") };
        assert_eq!(escape_os_name(d800), "x\\xED\\xA0\\x80y");
        assert_eq!(escape_os_name(dc00), "x\\xED\\xB0\\x80y");
        // 不同未配对代理 -> 不同显示文本, 不坍缩成同一标记
        assert_ne!(escape_os_name(d800), escape_os_name(dc00));
        // 周边合法字符保留; 全名不再是单个固定占位
        assert_eq!(escape_os_name(with_ascii), "x\\xED\\xA0\\x80.txt");
        // 字面 escape 文本不碰撞
        let literal = unsafe { OsStr::from_encoded_bytes_unchecked(b"x\\xED\\xA0\\x80y") };
        assert_eq!(escape_os_name(literal), "x\\\\xED\\\\xA0\\\\x80y");
        assert_ne!(escape_os_name(literal), escape_os_name(d800));
    }

    #[test]
    fn wtf8_surrogate_surrounding_unicode_is_preserved() {
        // 报告 + 未配对代理: 合法 scalar 原样保留, 仅代理序列逐字节转义
        // "报告" 的 UTF-8 字节 + 未配对代理序列 + ".txt"
        let name = unsafe {
            OsStr::from_encoded_bytes_unchecked(b"\xE6\x8A\xA5\xE5\x91\x8A\xED\xA0\x80.txt")
        };
        assert_eq!(escape_os_name(name), "报告\\xED\\xA0\\x80.txt");
    }

    #[test]
    fn content_disposition_wtf8_surrogate_omits_filename_star() {
        let name = unsafe { OsStr::from_encoded_bytes_unchecked(b"\xED\xA0\x80a.txt") };
        let value = content_disposition(name);
        // fallback 由同一字节逐字节派生, filename* 省略 (非 Unicode basename)
        assert_eq!(value, "attachment; filename=\"___a.txt\"");
        assert!(!value.contains("filename*"));
    }

    #[test]
    fn content_disposition_rfc8187_example() {
        assert_eq!(
            content_disposition(OsStr::new("报告 (final)*.md")),
            "attachment; filename=\"____final__.md\"; \
             filename*=UTF-8''%E6%8A%A5%E5%91%8A%20%28final%29%2A.md"
        );
    }

    #[test]
    fn content_disposition_ascii_passthrough() {
        assert_eq!(
            content_disposition(OsStr::new("report.txt")),
            "attachment; filename=\"report.txt\"; filename*=UTF-8''report.txt"
        );
    }

    #[test]
    fn content_disposition_fallback_sanitizes_quotes_and_backslashes() {
        assert_eq!(
            content_disposition(OsStr::new("a\"b\\c")),
            "attachment; filename=\"a_b_c\"; filename*=UTF-8''a%22b%5Cc"
        );
    }

    #[test]
    fn content_disposition_attr_chars_kept_in_star() {
        assert_eq!(
            content_disposition(OsStr::new("a~!b")),
            "attachment; filename=\"a__b\"; filename*=UTF-8''a~!b"
        );
    }

    #[test]
    fn content_disposition_empty_basename() {
        assert_eq!(
            content_disposition(OsStr::new("")),
            "attachment; filename=\"\"; filename*=UTF-8''"
        );
    }

    #[cfg(unix)]
    #[test]
    fn content_disposition_non_utf8_omits_filename_star() {
        use std::os::unix::ffi::OsStrExt;
        let value = content_disposition(OsStr::from_bytes(b"\xFFa"));
        assert_eq!(value, "attachment; filename=\"_a\"");
        assert!(!value.contains("filename*"));
    }

    #[test]
    fn from_sets_escaped_display_name_and_exact_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let p = root.join("My File.txt");
        std::fs::write(&p, b"x").unwrap();
        let m = FileMeta::from(root, &p).unwrap();
        assert_eq!(m.display_name, "My File.txt");
        assert_eq!(m.relative_to_root, Path::new("My File.txt"));
        assert_eq!(m.href(), Some("/My%20File.txt".to_string()));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn from_non_utf8_name_escapes_losslessly() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let name = OsStr::from_bytes(b"\xFFna\xE4\xB8me");
        let p = root.join(name);
        std::fs::write(&p, b"x").unwrap();
        let m = FileMeta::from(root, &p).unwrap();
        assert_eq!(m.display_name, "\\xFFna\\xE4\\xB8me");
        // identity 保留原始字节
        assert_eq!(
            m.relative_to_root.as_os_str().as_bytes(),
            b"\xFFna\xE4\xB8me"
        );
        assert!(m.href().is_none());
        // 字面 escape 名字不碰撞
        let p2 = root.join(OsStr::from_bytes(b"\\xFFna\\xE4\\xB8me"));
        std::fs::write(&p2, b"x").unwrap();
        let m2 = FileMeta::from(root, &p2).unwrap();
        assert_eq!(m2.display_name, "\\\\xFFna\\\\xE4\\\\xB8me");
        assert_ne!(m.display_name, m2.display_name);
    }

    /// 共享 relative-path 编码器 (唯一 path codec): FileMeta::browser_path_encoded /
    /// 面包屑 / 目录 fragment wrapper / 搜索位置全部复用, 不维护第二套 segment/path codec.
    /// 逐 segment RFC 3986 编码, 无前导斜杠; root (空路径) 是合法空串.
    #[test]
    fn encode_relative_path_is_the_single_path_codec() {
        assert_eq!(
            encode_relative_path(Path::new("a b/c+d")),
            Some("a%20b/c%2Bd".to_string())
        );
        assert_eq!(
            encode_relative_path(Path::new("e=f/g#h")),
            Some("e%3Df/g%23h".to_string())
        );
        assert_eq!(
            encode_relative_path(Path::new("dir/中文/报告.md")),
            Some("dir/%E4%B8%AD%E6%96%87/%E6%8A%A5%E5%91%8A.md".to_string())
        );
        assert_eq!(encode_relative_path(Path::new("")), Some(String::new()));
        assert_eq!(
            encode_relative_path(Path::new("a%2Fb")),
            Some("a%252Fb".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn encode_relative_path_non_utf8_returns_none() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(
            encode_relative_path(&PathBuf::from(OsString::from_vec(b"a\xFFb".to_vec()))),
            None
        );
        // 祖先 segment 非 UTF-8 (basename 本身合法) 同样 None: 整体不可浏览器操作
        assert_eq!(
            encode_relative_path(&PathBuf::from(OsString::from_vec(
                b"x\xFF/leaf.txt".to_vec()
            ))),
            None
        );
    }

    /// browser_path_encoded 必须委托共享编码器 (同一输出, 无第二套 codec).
    #[test]
    fn browser_path_encoded_matches_shared_encoder() {
        for p in ["a b", "c+d/e=f", "中文/报告.md", ""] {
            let m = meta(Path::new(p));
            assert_eq!(m.browser_path_encoded(), encode_relative_path(Path::new(p)));
        }
    }
}
