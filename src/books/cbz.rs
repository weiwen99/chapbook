//! CBZ（漫画压缩包）预览：受限归档索引、自然页序、无 JS 全页 HTML、按索引读取单页.
//!
//! 不把归档读入内存、不解压落盘、不把成员路径用于文件系统或 DOM. 索引只在本请求
//! 内读取一次有界 central directory 快照；页字节由 zip 的随机读取 + Stored/Deflated
//! 解压提供，并受本模块的输出/压缩输入预算约束。

use std::cell::Cell;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::str;

use super::html::HtmlWriter;
use super::{Error, Image};
use crate::render::TocEntry;
use zip::read::{ArchiveOffset, Config};
use zip::{CompressionMethod, ZipArchive};

/// CBZ 原文件上限. 公开给 `books::Format::max_file_bytes`, 供路由在获取
/// document gate 之前拒绝明显超限的归档 (与 Office 输入上限同一处理点).
pub(super) const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
/// 归档条目数上限: 在 zip 索引/IndexMap 预分配之前检查.
const MAX_ENTRIES: usize = 4096;
/// central directory 快照上限: 在读取文件字节与 zip 解析分配之前检查.
const MAX_CENTRAL_DIRECTORY_BYTES: usize = 8 * 1024 * 1024;
/// 单页解压输出上限.
const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;
/// 单页压缩输入上限.
const MAX_PAGE_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
/// ZIP32 EOCD 及其 comment 的最大扫描窗口.
const EOCD_SCAN_BYTES: u64 = 65_557;
const EOCD_SIZE: usize = 22;
const CENTRAL_HEADER_SIZE: usize = 46;
/// 最小 local file header 长度 (固定字段), 用于拒绝指向目录区域的页偏移.
const LOCAL_HEADER_SIZE: u64 = 30;

const SIG_CENTRAL: [u8; 4] = *b"PK\x01\x02";
const SIG_EOCD: [u8; 4] = *b"PK\x05\x06";
const SIG_EOCD64_LOCATOR: [u8; 4] = *b"PK\x06\x07";

/// 索引中的一个候选页. `name` 只用于服务器端排序/去重，绝不进入 URL、DOM id、
/// alt 或可见文件名. `index` 是 zip 库索引 (本模块保证与 EOCD 声明顺序一致).
#[derive(Debug)]
struct Page {
    name: Vec<u8>,
    index: usize,
    method: u16,
    compressed_size: u64,
    uncompressed_size: u64,
    header_start: u64,
}

/// 已通过预检的归档初始化材料. 只在本请求内存在，不跨请求缓存.
struct Opened<'a> {
    pages: Vec<Page>,
    archive: ZipArchive<GuardedReader<'a>>,
    cd_start: u64,
}

/// 同一 `File` 的受限初始化视图：
///
/// - `[0, cd_start)` 在初始化阶段返回零字节，库看不到文件数据中可能存在的另一
///   EOCD；
/// - `[cd_start, cd_start + cd.len())` 是已逐条校验的 central directory 快照；
/// - 其后是所选 EOCD 的无 comment 副本，虚拟 EOF 紧随其后。
///
/// `live` 由调用方持有并在 `ZipArchive::with_config` 成功后置位；之后同一个
/// wrapper 直接代理同一个 `File` 的读取，原始 local-header offset 不变。没有
/// `unsafe`、没有 reader/metadata 替换、没有第二次打开文件.
struct GuardedReader<'a> {
    file: File,
    cd_start: u64,
    cd: Vec<u8>,
    footer: [u8; EOCD_SIZE],
    pos: u64,
    live: &'a Cell<bool>,
}

impl<'a> GuardedReader<'a> {
    fn new(
        file: File,
        cd_start: u64,
        cd: Vec<u8>,
        footer: [u8; EOCD_SIZE],
        live: &'a Cell<bool>,
    ) -> Self {
        Self {
            file,
            cd_start,
            cd,
            footer,
            pos: 0,
            live,
        }
    }

    fn virtual_end(&self) -> u64 {
        self.cd_start + self.cd.len() as u64 + EOCD_SIZE as u64
    }

    fn read_virtual(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let end = self.virtual_end();
        if self.pos >= end {
            return Ok(0);
        }
        let want = usize::try_from(end - self.pos)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let cd_end = self.cd_start + self.cd.len() as u64;
        let mut filled = 0usize;
        while filled < want {
            let at = self.pos + filled as u64;
            if at < self.cd_start {
                let chunk =
                    (want - filled).min(usize::try_from(self.cd_start - at).unwrap_or(usize::MAX));
                buf[filled..filled + chunk].fill(0);
                filled += chunk;
            } else if at < cd_end {
                let offset = usize::try_from(at - self.cd_start).expect("snapshot offset");
                let chunk = (want - filled).min(self.cd.len() - offset);
                buf[filled..filled + chunk].copy_from_slice(&self.cd[offset..offset + chunk]);
                filled += chunk;
            } else {
                let offset = usize::try_from(at - cd_end).expect("footer offset");
                let chunk = (want - filled).min(self.footer.len() - offset);
                buf[filled..filled + chunk].copy_from_slice(&self.footer[offset..offset + chunk]);
                filled += chunk;
            }
        }
        self.pos += want as u64;
        Ok(want)
    }
}

impl Read for GuardedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.live.get() {
            return self.file.read(buf);
        }
        self.read_virtual(buf)
    }
}

impl Seek for GuardedReader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        if self.live.get() {
            return self.file.seek(from);
        }
        let next = match from {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(offset) => self.virtual_end().checked_add_signed(offset),
            SeekFrom::Current(offset) => self.pos.checked_add_signed(offset),
        };
        match next {
            Some(next) => {
                self.pos = next;
                Ok(next)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }
}

/// CBZ 阅读正文：无 JS 的全页序列（TOC + 每页标题锚点/图片/页码/前后跳转）.
/// 不返回 `.cb-doc` 包装或整页骨架；成员名不进入任何可见文本或 id.
pub(super) fn render(path: &Path, encoded: &str) -> Result<String, Error> {
    debug_assert!(
        !encoded.bytes().any(|b| matches!(
            b,
            b'"' | b'\'' | b'<' | b'>' | b'&' | b' ' | b'\r' | b'\n' | b'\t' | 0
        )),
        "encoded book identity must already be canonical RFC3986"
    );
    // 父级 codec 已保证 canonical identity；仍经共享 escaper 转义一次，随后按页
    // 复用结果，避免逐页重复转义与把来源字符串直接拼进属性.
    let encoded_html = {
        let mut escaped = HtmlWriter::new(super::MAX_HTML_BYTES);
        escaped.push_escaped(encoded)?;
        escaped.finish()
    };
    let live = Cell::new(false);
    let mut opened = open(path, &live)?;
    if opened.pages.is_empty() {
        return Err(Error::Unsupported("archive has no supported page images"));
    }
    opened.pages.sort_by(|a, b| {
        natural_cmp(&a.name, &b.name)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.index.cmp(&b.index))
    });
    let total = opened.pages.len();

    let toc_entries: Vec<TocEntry> = opened
        .pages
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let number = i + 1;
            TocEntry {
                level: 1,
                text: format!("Page {number}"),
                slug: format!("page-{number}"),
            }
        })
        .collect();
    // 与 Markdown/Org/EPUB 共用同一 TOC 结构与浏览器 TOC 驱动 (#TOC + hN > a[id]).
    let toc = crate::render::toc_html(&toc_entries, None);

    let mut out = HtmlWriter::new(super::MAX_HTML_BYTES);
    out.push(&toc)?;
    out.push("<div class=\"cb-pages\">")?;
    for (i, page) in opened.pages.iter().enumerate() {
        let number = i + 1;
        out.push("<figure class=\"cb-page\">")?;
        out.push("<h2 class=\"cb-page-title\"><a id=\"page-")?;
        out.push_u64(number as u64)?;
        out.push("\" href=\"#page-")?;
        out.push_u64(number as u64)?;
        out.push("\">Page ")?;
        out.push_u64(number as u64)?;
        out.push("</a></h2>")?;
        // 预留给父级 CSS 的稳定框位; 不在渲染期解码图片, 也不解压整本.
        out.push("<div class=\"cb-page-frame\"><img class=\"cb-page-image\" src=\"/__/book-page/")?;
        out.push(&encoded_html)?;
        out.push("?page=")?;
        out.push_u64(page.index as u64)?;
        out.push("\" alt=\"Page ")?;
        out.push_u64(number as u64)?;
        out.push("\" loading=\"lazy\" decoding=\"async\"></div>")?;
        out.push("<figcaption>Page ")?;
        out.push_u64(number as u64)?;
        out.push(" of ")?;
        out.push_u64(total as u64)?;
        out.push("</figcaption>")?;
        out.push("<nav class=\"cb-page-nav\" aria-label=\"Page navigation\">")?;
        if i > 0 {
            out.push("<a rel=\"prev\" href=\"#page-")?;
            out.push_u64((number - 1) as u64)?;
            out.push("\">Previous page</a>")?;
        }
        if i + 1 < total {
            out.push("<a rel=\"next\" href=\"#page-")?;
            out.push_u64((number + 1) as u64)?;
            out.push("\">Next page</a>")?;
        }
        out.push("</nav></figure>")?;
    }
    out.push("</div>")?;
    Ok(out.finish())
}

/// 按 zip 库索引读取一页原始图片字节. 索引不是成员路径：不查询/不拼接路径，
/// 只接受索引重建时确认过的候选页，并在读取前重新校验当前库条目.
pub(super) fn page(path: &Path, index: usize) -> Result<Image, Error> {
    let live = Cell::new(false);
    let Opened {
        pages,
        mut archive,
        cd_start,
    } = open(path, &live)?;
    let expected = pages
        .iter()
        .find(|candidate| candidate.index == index)
        .ok_or(Error::NotFound)?;

    let mut file = archive.by_index(index).map_err(page_zip_error)?;
    if file.encrypted() {
        return Err(Error::Unsupported("encrypted archive"));
    }
    if !file.is_file() || !page_name_ok(file.name_raw()) {
        return Err(Error::NotFound);
    }
    let compression = file.compression();
    let compression_matches = match expected.method {
        0 => compression == CompressionMethod::Stored,
        8 => compression == CompressionMethod::Deflated,
        _ => false,
    };
    if !compression_matches {
        return Err(Error::Unsupported("page compression method"));
    }
    // zip 可用 Unicode Path extra field 重写文件名；identity 按索引及位置/大小校验。
    if file.header_start() != expected.header_start
        || file.header_start() >= cd_start
        || file.compressed_size() != expected.compressed_size
        || file.size() != expected.uncompressed_size
    {
        return Err(Error::Invalid("archive entry changed".into()));
    }
    if file.size() > MAX_PAGE_BYTES || file.compressed_size() > MAX_PAGE_COMPRESSED_BYTES {
        return Err(Error::Limit("cbz page"));
    }
    let declared = usize::try_from(file.size()).map_err(|_| Error::Limit("cbz page"))?;
    let mut bytes = Vec::with_capacity(declared);
    // `take(cap + 1)`：先按声明大小分配上限，再以实际输出兜底；读尽合法页
    // 直到 EOF 以触发 zip 的 CRC 校验。超过上限的记录会在扩容前失败.
    let mut limited = (&mut file).take(MAX_PAGE_BYTES + 1);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = limited.read(&mut chunk).map_err(page_io_error)?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > MAX_PAGE_BYTES as usize {
            return Err(Error::Limit("cbz page"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let media_type = super::image_media_type(&bytes)?
        .ok_or(Error::Unsupported("page is not a supported image"))?;
    Ok(Image { bytes, media_type })
}

/// 打开并初始化归档：预检 EOCD/CD/comment，绑定快照，初始化 zip 索引，然后
/// 切回原始 `File` 读取. 任何预检失败都发生在 zip 索引分配与页读取之前.
fn open<'a>(path: &Path, live: &'a Cell<bool>) -> Result<Opened<'a>, Error> {
    let mut file = File::open(path).map_err(open_error)?;
    let metadata = file.metadata().map_err(Error::Io)?;
    if !metadata.is_file() {
        return Err(Error::NotFound);
    }
    let file_len = metadata.len();
    if file_len > MAX_ARCHIVE_BYTES {
        return Err(Error::Limit("cbz archive"));
    }

    let Preflight {
        cd_start,
        cd,
        footer,
        entries,
    } = preflight(&mut file, file_len)?;
    let pages = parse_central_directory(&cd, entries, cd_start)?;

    let reader = GuardedReader::new(file, cd_start, cd, footer, live);
    // `Known(0)` 固定原始坐标；虚拟流中只有已校验目录和单一无 comment footer,
    // 因此库无法回退到文件数据中的其他 EOCD.
    let archive = ZipArchive::with_config(
        Config {
            archive_offset: ArchiveOffset::Known(0),
        },
        reader,
    )
    .map_err(zip_error)?;
    if archive.len() != entries {
        return Err(Error::Invalid("archive entry count mismatch".into()));
    }
    if archive.central_directory_start() != cd_start {
        return Err(Error::Invalid("archive central directory mismatch".into()));
    }
    live.set(true);
    Ok(Opened {
        pages,
        archive,
        cd_start,
    })
}

#[derive(Debug)]
struct Preflight {
    cd_start: u64,
    cd: Vec<u8>,
    footer: [u8; EOCD_SIZE],
    entries: usize,
}

/// ZIP32 EOCD 预检与 central directory 快照. 只信任与 EOF 精确匹配的单个无歧义
/// footer；拒绝分卷/ZIP64 标记、分卷目录、超量条目/目录、CD 或 comment 中的
/// 其他 EOCD 签名.
fn preflight(file: &mut File, file_len: u64) -> Result<Preflight, Error> {
    if file_len < EOCD_SIZE as u64 {
        return Err(Error::Invalid("archive is too short for ZIP".into()));
    }
    let tail_len =
        usize::try_from(file_len.min(EOCD_SCAN_BYTES)).map_err(|_| Error::Limit("cbz archive"))?;
    let tail_start = file_len - tail_len as u64;
    let mut tail = vec![0u8; tail_len];
    read_exact_at(file, tail_start, &mut tail)?;

    let mut chosen = None;
    for pos in 0..=tail_len - EOCD_SIZE {
        if !tail[pos..].starts_with(&SIG_EOCD) {
            continue;
        }
        let comment_len = usize::from(le16(&tail, pos + 20));
        let absolute = tail_start + pos as u64;
        let record_end = absolute + (EOCD_SIZE + comment_len) as u64;
        if record_end == file_len {
            if chosen.is_some() {
                return Err(Error::Invalid("ambiguous archive footer".into()));
            }
            chosen = Some(pos);
        }
    }
    let pos = chosen.ok_or_else(|| Error::Invalid("missing end of central directory".into()))?;
    let comment_len = usize::from(le16(&tail, pos + 20));
    let comment = &tail[pos + EOCD_SIZE..pos + EOCD_SIZE + comment_len];
    if contains_sig(comment, &SIG_EOCD) {
        return Err(Error::Invalid("ambiguous archive comment".into()));
    }

    let block = &tail[pos..pos + EOCD_SIZE];
    let disk_number = le16(block, 4);
    let disk_with_central_directory = le16(block, 6);
    let entries_on_disk = le16(block, 8);
    let entries_total = le16(block, 10);
    let cd_size = u64::from(le32(block, 12));
    let cd_offset = u64::from(le32(block, 16));
    let eocd_start = tail_start + pos as u64;

    if disk_number != 0 || disk_with_central_directory != 0 || entries_on_disk != entries_total {
        return Err(Error::Unsupported("multi-disk archive"));
    }
    if entries_total == u16::MAX
        || entries_on_disk == u16::MAX
        || cd_size == u64::from(u32::MAX)
        || cd_offset == u64::from(u32::MAX)
    {
        return Err(Error::Unsupported("zip64 archive"));
    }
    let entries = usize::from(entries_total);
    if entries > MAX_ENTRIES {
        return Err(Error::Limit("cbz entries"));
    }
    if cd_size > MAX_CENTRAL_DIRECTORY_BYTES as u64 {
        return Err(Error::Limit("cbz central directory"));
    }
    if cd_offset
        .checked_add(cd_size)
        .is_none_or(|end| end != eocd_start)
    {
        return Err(Error::Invalid(
            "central directory does not end at archive footer".into(),
        ));
    }
    // ZIP64 EOCD locator 紧邻 EOCD 之前；即使 EOCD 字段未置哨兵也拒绝.
    if eocd_start >= 20 {
        let mut locator = [0u8; 20];
        read_exact_at(file, eocd_start - 20, &mut locator)?;
        if locator.starts_with(&SIG_EOCD64_LOCATOR) {
            return Err(Error::Unsupported("zip64 archive"));
        }
    }

    let cd_len = usize::try_from(cd_size).map_err(|_| Error::Limit("cbz central directory"))?;
    let mut cd = vec![0u8; cd_len];
    read_exact_at(file, cd_offset, &mut cd)?;
    if contains_sig(&cd, &SIG_EOCD) {
        return Err(Error::Invalid("ambiguous central directory".into()));
    }
    // 归档 comment 被剥离；footer 其余字段保持原坐标 (Known(0) 索引不变).
    let mut footer = [0u8; EOCD_SIZE];
    footer.copy_from_slice(block);
    footer[20] = 0;
    footer[21] = 0;

    Ok(Preflight {
        cd_start: cd_offset,
        cd,
        footer,
        entries,
    })
}

/// 逐条校验 central directory 固定 header 与 name/extra/comment 声明长度，要求
/// 恰好消费声明条目数与目录范围. 同时拒绝重复 raw 名、ZIP64/多卷标记、与候选页
/// 明确不兼容的压缩/加密, 并建立候选页列表 (页序只由 `render` 按需排序).
fn parse_central_directory(cd: &[u8], count: usize, cd_start: u64) -> Result<Vec<Page>, Error> {
    let mut pages = Vec::with_capacity(count.min(MAX_ENTRIES));
    let mut names: HashSet<&[u8]> = HashSet::with_capacity(count);
    let mut offset = 0usize;
    for index in 0..count {
        let header = cd
            .get(offset..offset + CENTRAL_HEADER_SIZE)
            .ok_or_else(|| Error::Invalid("truncated central directory".into()))?;
        if !header.starts_with(&SIG_CENTRAL) {
            return Err(Error::Invalid("invalid central directory header".into()));
        }
        let flags = le16(header, 8);
        let method = le16(header, 10);
        let compressed_size = u64::from(le32(header, 20));
        let uncompressed_size = u64::from(le32(header, 24));
        let name_len = usize::from(le16(header, 28));
        let extra_len = usize::from(le16(header, 30));
        let comment_len = usize::from(le16(header, 32));
        let disk_number = le16(header, 34);
        let external_attributes = le32(header, 38);
        let local_offset = u64::from(le32(header, 42));
        let record_len = CENTRAL_HEADER_SIZE
            .checked_add(name_len)
            .and_then(|n| n.checked_add(extra_len))
            .and_then(|n| n.checked_add(comment_len))
            .ok_or_else(|| Error::Invalid("central directory length overflow".into()))?;
        let record = cd
            .get(offset..offset + record_len)
            .ok_or_else(|| Error::Invalid("truncated central directory record".into()))?;
        let name = &record[CENTRAL_HEADER_SIZE..CENTRAL_HEADER_SIZE + name_len];
        let extra_start = CENTRAL_HEADER_SIZE + name_len;
        let extra = &record[extra_start..extra_start + extra_len];

        if !names.insert(name) {
            return Err(Error::Invalid("duplicate member names".into()));
        }
        if name.is_empty() {
            return Err(Error::Invalid("empty member name".into()));
        }
        if disk_number != 0 {
            return Err(Error::Unsupported("multi-disk archive"));
        }
        if compressed_size == u64::from(u32::MAX)
            || uncompressed_size == u64::from(u32::MAX)
            || local_offset == u64::from(u32::MAX)
        {
            return Err(Error::Unsupported("zip64 archive"));
        }
        reject_zip64_extra(extra)?;

        if page_candidate(name, external_attributes) {
            if flags & 0x0001 != 0
                || flags & 0x0040 != 0
                || method == 99
                || extra_has(extra, 0x9901)
            {
                return Err(Error::Unsupported("encrypted archive"));
            }
            if method != 0 && method != 8 {
                return Err(Error::Unsupported("page compression method"));
            }
            // 页大小预算在 `page` 读取该页时执行：坏页/超限页只在子资源 422,
            // 不因单页声明超限让整本索引/阅读页失败.
            if local_offset
                .checked_add(LOCAL_HEADER_SIZE)
                .is_none_or(|end| end > cd_start)
            {
                return Err(Error::Invalid("invalid page offset".into()));
            }
            pages.push(Page {
                name: name.to_vec(),
                index,
                method,
                compressed_size,
                uncompressed_size,
                header_start: local_offset,
            });
        }
        offset += record_len;
    }
    if offset != cd.len() {
        return Err(Error::Invalid(
            "central directory length does not match declaration".into(),
        ));
    }

    Ok(pages)
}

/// 拒绝 CD extra 中的 ZIP64 extended information 字段；其余 extra 由 zip 解析，
/// 不影响候选页元数据校验.
fn reject_zip64_extra(extra: &[u8]) -> Result<(), Error> {
    if extra_has(extra, 0x0001) {
        return Err(Error::Unsupported("zip64 archive"));
    }
    Ok(())
}

/// 在 CD extra 中查找字段 ID. 只解析 zip extra 的 4 字节头；字段内容交给库.
fn extra_has(extra: &[u8], wanted: u16) -> bool {
    let mut offset = 0usize;
    while offset + 4 <= extra.len() {
        let id = le16(extra, offset);
        let len = usize::from(le16(extra, offset + 2));
        if id == wanted {
            return true;
        }
        offset += 4 + len;
    }
    false
}

/// 候选页的路径判定：排除目录、symlink、隐藏/空路径段、`__MACOSX`、NUL 与
/// 非图片扩展名. 成员分隔符按 `/` 或 `\` 处理，绝不 join 到文件系统.
fn page_candidate(name: &[u8], external_attributes: u32) -> bool {
    page_name_ok(name) && !directory_or_symlink(external_attributes)
}

fn page_name_ok(name: &[u8]) -> bool {
    components_allowed(name) && has_image_extension(name)
}

fn components_allowed(name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut start = 0usize;
    for i in 0..=name.len() {
        if i != name.len() && name[i] != b'/' && name[i] != b'\\' {
            continue;
        }
        let component = &name[start..i];
        if component.is_empty()
            || component[0] == b'.'
            || component.contains(&0)
            || component.eq_ignore_ascii_case(b"__MACOSX")
        {
            return false;
        }
        start = i + 1;
    }
    true
}

fn directory_or_symlink(external_attributes: u32) -> bool {
    let mode = external_attributes >> 16;
    let kind = mode & 0o170000;
    kind == 0o040000 || kind == 0o120000 || (mode == 0 && external_attributes & 0x10 != 0)
}

fn has_image_extension(name: &[u8]) -> bool {
    match str::from_utf8(name) {
        Ok(text) => super::image_extension(text),
        Err(_) => {
            let text = String::from_utf8_lossy(name);
            super::image_extension(text.as_ref())
        }
    }
}

/// 数字感知自然升序：数字段去前导零后按长度/内容比较，不解析为整数（无溢出）.
/// 数字段相等时继续比较后续段；完全相等由调用方用 raw name/index 决定全序.
fn natural_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = 0usize;
    let mut bi = 0usize;
    while ai < a.len() && bi < b.len() {
        let left = a[ai];
        let right = b[bi];
        if left.is_ascii_digit() && right.is_ascii_digit() {
            let a_end = digit_run_end(a, ai);
            let b_end = digit_run_end(b, bi);
            let a_digits = trim_leading_zeroes(&a[ai..a_end]);
            let b_digits = trim_leading_zeroes(&b[bi..b_end]);
            let ordering = a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits));
            if ordering != Ordering::Equal {
                return ordering;
            }
            ai = a_end;
            bi = b_end;
        } else {
            if left != right {
                return left.cmp(&right);
            }
            ai += 1;
            bi += 1;
        }
    }
    if ai == a.len() && bi == b.len() {
        std::cmp::Ordering::Equal
    } else if ai == a.len() {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Greater
    }
}

fn digit_run_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    end
}

fn trim_leading_zeroes(digits: &[u8]) -> &[u8] {
    let start = digits
        .iter()
        .position(|&b| b != b'0')
        .unwrap_or(digits.len());
    &digits[start..]
}

fn read_exact_at(file: &mut File, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    file.seek(SeekFrom::Start(offset)).map_err(Error::Io)?;
    file.read_exact(buf).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Error::Invalid("truncated CBZ archive".into())
        } else {
            Error::Io(error)
        }
    })
}

fn contains_sig(haystack: &[u8], signature: &[u8; 4]) -> bool {
    haystack
        .windows(4)
        .any(|window| window == signature.as_slice())
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn open_error(error: io::Error) -> Error {
    if error.kind() == io::ErrorKind::NotFound {
        Error::NotFound
    } else {
        Error::Io(error)
    }
}

fn zip_error(error: zip::result::ZipError) -> Error {
    use zip::result::ZipError;
    match error {
        ZipError::Io(error) => Error::Io(error),
        ZipError::InvalidArchive(message) => Error::Invalid(message.into_owned()),
        ZipError::UnsupportedArchive(message) => Error::Unsupported(message),
        ZipError::FileNotFound => Error::NotFound,
        ZipError::InvalidPassword => Error::Unsupported("encrypted archive"),
        ZipError::CompressionMethodNotSupported(_) => Error::Unsupported("compression method"),
        _ => Error::Invalid("invalid zip archive".into()),
    }
}

/// 页读取中的损坏 (CRC/解压/截断) 映射为可重试的 Invalid；真实文件 I/O 仍是 Io.
fn page_zip_error(error: zip::result::ZipError) -> Error {
    match error {
        zip::result::ZipError::Io(error) => page_io_error(error),
        other => zip_error(other),
    }
}

fn page_io_error(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidInput => {
            Error::Invalid(format!("corrupt page: {error}"))
        }
        _ => Error::Io(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use tempfile::NamedTempFile;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
                )
                .expect("start zip entry");
            writer.write_all(data).expect("write zip entry");
        }
        writer.finish().expect("finish zip").into_inner()
    }

    fn file_with(bytes: &[u8]) -> (NamedTempFile, File) {
        let mut tmp = NamedTempFile::new().expect("temp file");
        tmp.write_all(bytes).expect("write temp");
        tmp.flush().expect("flush temp");
        let file = File::open(tmp.path()).expect("open temp");
        (tmp, file)
    }

    fn eocd(entries: u16, cd_size: u32, cd_offset: u32) -> [u8; EOCD_SIZE] {
        let mut block = [0u8; EOCD_SIZE];
        block[..4].copy_from_slice(&SIG_EOCD);
        block[8..10].copy_from_slice(&entries.to_le_bytes());
        block[10..12].copy_from_slice(&entries.to_le_bytes());
        block[12..16].copy_from_slice(&cd_size.to_le_bytes());
        block[16..20].copy_from_slice(&cd_offset.to_le_bytes());
        block
    }

    #[test]
    fn natural_order_does_not_overflow_huge_numeric_runs() {
        use std::cmp::Ordering;
        let huge = b"page-18446744073709551616.png";
        let nine = b"page-9.png";
        assert_eq!(natural_cmp(nine, huge), Ordering::Less);
        assert_eq!(natural_cmp(huge, nine), Ordering::Greater);
        let huge_plus = b"page-18446744073709551617.png";
        assert_eq!(natural_cmp(huge, huge_plus), Ordering::Less);
        assert_eq!(
            natural_cmp(b"01.bmp", b"1.bmp"),
            Ordering::Equal,
            "numeric runs compare by value before the raw tie-break"
        );
    }

    #[test]
    fn guarded_reader_masks_prefix_and_cd_and_switches_to_file() {
        let (_tmp, file) = file_with(b"REAL-DATA");
        let live = Cell::new(false);
        let mut reader =
            GuardedReader::new(file, 8, b"CDSNAPSHOT".to_vec(), [0x5Au8; EOCD_SIZE], &live);
        let mut probe = [0u8; 8];
        reader.seek(SeekFrom::Start(0)).expect("seek start");
        reader.read_exact(&mut probe[..4]).expect("read prefix");
        assert_eq!(&probe[..4], b"\0\0\0\0");
        reader.seek(SeekFrom::Start(8)).expect("seek cd");
        reader.read_exact(&mut probe[..2]).expect("read cd");
        assert_eq!(&probe[..2], b"CD");
        reader.seek(SeekFrom::Start(8 + 10)).expect("seek footer");
        reader.read_exact(&mut probe[..2]).expect("read footer");
        assert_eq!(&probe[..2], &[0x5A, 0x5A]);
        assert_eq!(
            reader.seek(SeekFrom::End(0)).expect("virtual eof"),
            8 + 10 + EOCD_SIZE as u64
        );

        live.set(true);
        reader.seek(SeekFrom::Start(0)).expect("seek real");
        reader.read_exact(&mut probe).expect("read real");
        assert_eq!(&probe, b"REAL-DAT");
        assert_eq!(reader.seek(SeekFrom::End(0)).expect("real eof"), 9);
    }

    #[test]
    fn guarded_init_cannot_fall_back_to_an_unvalidated_footer() {
        // 前缀里放一个完整可解析的 ZIP；只把坏掉的 CD 快照和其后的单一 footer
        // 暴露给索引器。若库回退到前缀中的 EOCD，就会成功并读到伪条目.
        let decoy = stored_zip(&[("decoy.png", b"decoy")]);
        let bad_cd = b"not a central directory";
        let cd_start = decoy.len() as u64;
        let mut bytes = decoy;
        bytes.extend_from_slice(bad_cd);
        bytes.extend_from_slice(&eocd(
            1,
            bad_cd.len() as u32,
            u32::try_from(cd_start).expect("offset"),
        ));
        let (_tmp, file) = file_with(&bytes);
        let live = Cell::new(false);
        let reader = GuardedReader::new(
            file,
            cd_start,
            bad_cd.to_vec(),
            eocd(
                1,
                bad_cd.len() as u32,
                u32::try_from(cd_start).expect("offset"),
            ),
            &live,
        );
        let result = ZipArchive::with_config(
            Config {
                archive_offset: ArchiveOffset::Known(0),
            },
            reader,
        );
        assert!(
            result.is_err(),
            "guarded index must fail closed instead of falling back to the decoy footer"
        );
    }

    #[test]
    fn preflight_rejects_archive_comment_containing_eocd_signature() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "1.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("start entry");
        writer.write_all(b"page").expect("write entry");
        writer
            .set_raw_comment(Box::new(*b"hidden PK\x05\x06 footer"))
            .expect("set raw comment");
        let bytes = writer.finish().expect("finish zip").into_inner();
        let (_tmp, mut file) = file_with(&bytes);
        let error = preflight(&mut file, bytes.len() as u64).expect_err("ambiguous comment");
        assert!(
            matches!(&error, Error::Invalid(_)),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn preflight_rejects_eocd_signature_inside_central_directory() {
        let mut bytes = stored_zip(&[("evil.png", b"page")]);
        let central = bytes
            .windows(4)
            .position(|window| window == SIG_CENTRAL.as_slice())
            .expect("central directory");
        // 把 CD 条目的 name 前缀改成 EOCD 签名；声明长度不变，预检必须在索引前拒绝.
        bytes[central + CENTRAL_HEADER_SIZE..central + CENTRAL_HEADER_SIZE + 4]
            .copy_from_slice(&SIG_EOCD);
        let (_tmp, mut file) = file_with(&bytes);
        let error = preflight(&mut file, bytes.len() as u64).expect_err("ambiguous directory");
        assert!(
            matches!(&error, Error::Invalid(_)),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn preflight_limits_entries_before_reading_directory() {
        let bytes = eocd(MAX_ENTRIES as u16 + 1, 0, 0);
        let (_tmp, mut file) = file_with(&bytes);
        let error = preflight(&mut file, bytes.len() as u64).expect_err("entry limit");
        assert!(
            matches!(&error, Error::Limit(_)),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn preflight_rejects_zip64_markers() {
        let bytes = eocd(u16::MAX, 0, 0);
        let (_tmp, mut file) = file_with(&bytes);
        let error = preflight(&mut file, bytes.len() as u64).expect_err("zip64 marker");
        assert!(
            matches!(&error, Error::Unsupported(_)),
            "unexpected error: {error:?}"
        );
    }

    fn central_record(name: &[u8], local_offset: u32) -> Vec<u8> {
        let mut record = vec![0u8; CENTRAL_HEADER_SIZE];
        record[..4].copy_from_slice(&SIG_CENTRAL);
        record[20..24].copy_from_slice(&1u32.to_le_bytes());
        record[24..28].copy_from_slice(&1u32.to_le_bytes());
        record[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        record[42..46].copy_from_slice(&local_offset.to_le_bytes());
        record.extend_from_slice(name);
        record
    }

    #[test]
    fn central_directory_must_match_declared_count_and_extent() {
        let one = central_record(b"1.png", 0);
        let mut duplicate = one.clone();
        duplicate.extend_from_slice(&one);
        let error = parse_central_directory(&duplicate, 2, 100).expect_err("duplicate names");
        assert!(
            matches!(&error, Error::Invalid(_)),
            "unexpected error: {error:?}"
        );

        let mut padded = one.clone();
        padded.push(0);
        let error = parse_central_directory(&padded, 1, 100).expect_err("trailing byte");
        assert!(
            matches!(&error, Error::Invalid(_)),
            "unexpected error: {error:?}"
        );

        let error = parse_central_directory(&one, 2, 100).expect_err("short count");
        assert!(
            matches!(&error, Error::Invalid(_)),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn page_candidates_exclude_metadata_and_nonimages() {
        assert!(!page_candidate(b"pages/", 0), "directory name");
        assert!(!page_candidate(b"a/../b.png", 0), "parent segment");
        assert!(!page_candidate(b".hidden/1.png", 0), "hidden segment");
        assert!(!page_candidate(b"__MACOSX/x.png", 0), "macOS metadata");
        assert!(!page_candidate(b"1.png", 0o120777 << 16), "unix symlink");
        assert!(!page_candidate(b"1.png", 0o040755 << 16), "unix directory");
        assert!(!page_candidate(b"1.txt", 0), "non-image");
        assert!(page_candidate(b"nested/01.JPG", 0));
    }

    #[test]
    fn render_sorts_mixed_images_and_keeps_member_names_out_of_html() {
        let bytes = stored_zip(&[
            ("10.png", b"ten"),
            ("2.jpg", b"two"),
            ("1.bmp", b"one"),
            ("sub/3.PNG", b"three"),
            ("readme.txt", b"ignored"),
            (".hidden/4.png", b"hidden"),
            ("__MACOSX/._5.png", b"mac"),
        ]);
        let (tmp, _file) = file_with(&bytes);
        let html = render(tmp.path(), "comics%2Fmixed.cbz").expect("render");
        let anchors: Vec<usize> = (1..=4)
            .map(|n| {
                html.find(&format!("id=\"page-{n}\""))
                    .unwrap_or_else(|| panic!("missing page-{n}: {html}"))
            })
            .collect();
        assert!(anchors.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(html.matches("class=\"cb-page\"").count(), 4);
        assert_eq!(html.matches("loading=\"lazy\"").count(), 4);
        let first = html.find("?page=2\"").expect("1.bmp is zip index 2");
        let second = html.find("?page=1\"").expect("2.jpg is zip index 1");
        let third = html.find("?page=0\"").expect("10.png is zip index 0");
        let fourth = html.find("?page=3\"").expect("sub/3.PNG is zip index 3");
        assert!(first < second && second < third && third < fourth);
        assert!(html.contains("src=\"/__/book-page/comics%2Fmixed.cbz?page=2\""));
        assert!(!html.contains("1.bmp"));
        assert!(!html.contains("sub/3"));
        assert!(!html.contains("readme.txt"));
        assert!(html.contains("id=\"TOC\""), "shared TOC markup: {html}");
    }

    #[test]
    fn page_returns_original_image_bytes_and_index_limits() {
        // 最小 1x1 24bpp BMP：14 字节文件头 + 40 字节 DIB + 4 字节像素.
        let mut bmp = vec![0u8; 58];
        bmp[0] = b'B';
        bmp[1] = b'M';
        bmp[2..6].copy_from_slice(&58u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        bmp[34..38].copy_from_slice(&4u32.to_le_bytes());
        bmp[57] = 0xff;
        let bytes = stored_zip(&[("1.bmp", bmp.as_slice()), ("2.png", b"not an image")]);
        let (tmp, _file) = file_with(&bytes);
        let image = page(tmp.path(), 0).expect("page bytes");
        assert_eq!(image.bytes, bmp);
        assert_eq!(image.media_type, "image/bmp");
        assert!(matches!(page(tmp.path(), 1), Err(Error::Unsupported(_))));
        assert!(matches!(page(tmp.path(), 9), Err(Error::NotFound)));
    }
}
