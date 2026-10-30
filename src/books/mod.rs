//! 书籍 (EPUB / FB2 / CBZ) 可读预览: 格式识别、类型化错误、共同限额与受限
//! 解码. 内容渲染在 `html`, FB2 解析在 `fb2`, CBZ 归档与分页在 `cbz`.
//!
//! 这里是三种格式唯一的共同入口: 路由只经 [`format_for_path`] 识别格式,
//! 经 [`render`] 取安全正文, 经 [`cbz_page`] 取单张漫画页. 所有限额是硬
//! 上限, 超限返回 [`Error::Limit`], 不冒充完整成功.

use std::borrow::Cow;
use std::fmt;
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use quick_xml::Reader as XmlReader;
use quick_xml::events::Event as XmlEvent;

mod cbz;
mod fb2;
mod html;

/// 单张内嵌图片上限: 限制 `data:` URI 放大.
pub(crate) const MAX_INLINE_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// 一次预览引用到的内嵌资产总量上限 (按唯一资产计).
pub(crate) const MAX_ASSET_BYTES: usize = 24 * 1024 * 1024;
/// 生成的正文 HTML 上限: 重复图片引用 / 文字转义也不能无限放大.
pub(crate) const MAX_HTML_BYTES: usize = 64 * 1024 * 1024;
/// 解码后的 XML (UTF-8) 上限.
pub(crate) const MAX_XML_BYTES: usize = 64 * 1024 * 1024;
/// roxmltree 节点数上限, 也是 `<` / `=` 字面量预检上限.
pub(crate) const MAX_XML_NODES: u32 = 200_000;
/// 模型遍历 / 转换的嵌套深度上限.
pub(crate) const MAX_DEPTH: usize = 128;

/// 单个 EPUB / FB2 原文件上限: 沿用 Office 输入闸.
pub(crate) const MAX_BOOK_BYTES: u64 = crate::office::MAX_RENDER_BYTES;

/// 书籍格式. 与 `anydoc::Format` 分开: 这里是预览支持面, 不是转换器格式表.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Epub,
    Fb2,
    Cbz,
}

impl Format {
    /// 路由在启动转换前按 metadata 预检的大小上限. 预检只是快速拒绝; 实际
    /// 读取仍以 `take(max + 1)` 复核.
    pub(crate) fn max_file_bytes(self) -> u64 {
        match self {
            Format::Epub | Format::Fb2 => MAX_BOOK_BYTES,
            Format::Cbz => cbz::MAX_ARCHIVE_BYTES,
        }
    }
}

/// 书籍预览的类型化错误; 路由据此区分 404 / 422 / 回退原文.
#[derive(Debug)]
pub(crate) enum Error {
    /// 底层读取失败.
    Io(std::io::Error),
    /// 内容损坏或结构不受支持 (含非法 base64 / XML).
    Invalid(String),
    /// 已知但不做预览的格式或能力.
    Unsupported(&'static str),
    /// 超过固定资源上限; 载荷是稳定的限额名.
    Limit(&'static str),
    /// 文件不存在.
    NotFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::Invalid(detail) => write!(f, "invalid book: {detail}"),
            Error::Unsupported(what) => write!(f, "unsupported book: {what}"),
            Error::Limit(what) => write!(f, "book resource limit exceeded: {what}"),
            Error::NotFound => f.write_str("book not found"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// 一张从归档中读出的图片 (CBZ 子资源出口用).
#[derive(Debug)]
pub(crate) struct Image {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
}

/// 路径 -> 书籍格式 (扩展名 ASCII 大小写不敏感). 不读文件内容.
pub(crate) fn format_for_path(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("epub") {
        Some(Format::Epub)
    } else if ext.eq_ignore_ascii_case("fb2") {
        Some(Format::Fb2)
    } else if ext.eq_ignore_ascii_case("cbz") {
        Some(Format::Cbz)
    } else {
        None
    }
}

/// 渲染书籍正文 (含 TOC, 不含 `.cb-doc` 外壳). `encoded` 是父级已编码的
/// canonical 根相对 identity, 原样用于 CBZ 图片子资源 URL, 从不 decode.
pub(crate) fn render(path: &Path, format: Format, encoded: &str) -> Result<String, Error> {
    match format {
        Format::Epub => {
            let bytes = read_book(path)?;
            let doc = anydoc::to_document(&bytes, anydoc::Format::Epub).map_err(convert_error)?;
            html::render(&doc)
        }
        Format::Fb2 => {
            let bytes = read_book(path)?;
            let doc = fb2::parse(&bytes)?;
            html::render(&doc)
        }
        Format::Cbz => cbz::render(path, encoded),
    }
}

/// 读取 CBZ 的单页图片 (zip index, 不是成员路径).
pub(crate) fn cbz_page(path: &Path, index: usize) -> Result<Image, Error> {
    cbz::page(path, index)
}

/// 有界读取 EPUB / FB2 原文件: metadata 只是快速闸, 实际读取再 `take(max+1)`.
fn read_book(path: &Path) -> Result<Vec<u8>, Error> {
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > MAX_BOOK_BYTES {
        return Err(Error::Limit("max_book_bytes"));
    }
    let cap = usize::try_from(MAX_BOOK_BYTES)
        .unwrap_or(usize::MAX)
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(
        usize::try_from(len)
            .unwrap_or(usize::MAX)
            .min(cap.saturating_sub(1))
            .saturating_add(1),
    );
    file.take(MAX_BOOK_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BOOK_BYTES {
        return Err(Error::Limit("max_book_bytes"));
    }
    Ok(bytes)
}

/// anydoc 错误 -> 本模块错误. ResourceLimit 的稳定限额名原样保留.
fn convert_error(e: anydoc::ConvertError) -> Error {
    match e {
        anydoc::ConvertError::Io(e) => Error::Io(e),
        anydoc::ConvertError::Unsupported(_) => Error::Unsupported("unsupported epub"),
        anydoc::ConvertError::Malformed { part, detail } => match part {
            Some(part) => Error::Invalid(format!("{part}: {detail}")),
            None => Error::Invalid(detail),
        },
        anydoc::ConvertError::Encrypted => Error::Unsupported("encrypted book"),
        anydoc::ConvertError::ResourceLimit { limit, .. } => Error::Limit(limit),
        anydoc::ConvertError::MissingPart { part } => {
            Error::Invalid(format!("missing part: {part}"))
        }
        _ => Error::Invalid("book conversion failed".to_string()),
    }
}

/* ---------- XML (FB2 / SVG 共用) ---------- */

/// 严格解码 XML 字节: BOM 优先; 无 BOM 时识别 XML 声明中的 encoding,
/// 默认 UTF-8. 只用 encoding_rs 的无 replacement 解码, 转码后仍受
/// [`MAX_XML_BYTES`] 约束. UTF-16 声明缺 BOM 时拒绝 (字节序不可猜).
pub(super) fn decode_xml(bytes: &[u8]) -> Result<Cow<'_, str>, Error> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::str::from_utf8(rest)
            .map_err(|_| Error::Invalid("invalid UTF-8 after BOM".to_string()))
            .and_then(|s| bound_xml_text(s).map(Cow::Borrowed));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_bounded(encoding_rs::UTF_16LE, rest).map(Cow::Owned);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_bounded(encoding_rs::UTF_16BE, rest).map(Cow::Owned);
    }

    let Some(label) = declared_encoding(bytes) else {
        return std::str::from_utf8(bytes)
            .map_err(|_| Error::Invalid("XML is not UTF-8 and declares no encoding".to_string()))
            .and_then(|s| bound_xml_text(s).map(Cow::Borrowed));
    };
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes())
        .ok_or_else(|| Error::Invalid(format!("unknown XML encoding: {label}")))?;
    if encoding == encoding_rs::UTF_8 {
        return std::str::from_utf8(bytes)
            .map_err(|_| Error::Invalid("invalid UTF-8 XML".to_string()))
            .and_then(|s| bound_xml_text(s).map(Cow::Borrowed));
    }
    if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
        return Err(Error::Invalid(
            "UTF-16 XML requires a byte order mark".to_string(),
        ));
    }
    if encoding == encoding_rs::REPLACEMENT {
        return Err(Error::Invalid(format!("unknown XML encoding: {label}")));
    }
    decode_bounded(encoding, bytes).map(Cow::Owned)
}

fn bound_xml_text(text: &str) -> Result<&str, Error> {
    if text.len() > MAX_XML_BYTES {
        Err(Error::Limit("max_xml_bytes"))
    } else {
        Ok(text)
    }
}

/// 无 BOM 的 XML 声明里 `encoding='…'` 的值. 只解析开标签的 ASCII
/// 伪属性, 不猜测非 ASCII 内容; 找不到或格式异常时返回 None.
fn declared_encoding(bytes: &[u8]) -> Option<&str> {
    if !bytes.starts_with(b"<?xml") {
        return None;
    }
    // XML 声明实际很短; 只在头部窗口里找 `?>`, 避免扫描整个文件.
    let window = bytes.len().min(4096);
    let end = bytes[..window].windows(2).position(|w| w == b"?>")?;
    let decl = &bytes[..end];
    let mut i = b"<?xml".len();
    loop {
        while i < decl.len() && is_xml_whitespace(decl[i]) {
            i += 1;
        }
        if i >= decl.len() {
            return None;
        }
        let name_start = i;
        while i < decl.len() && !is_xml_whitespace(decl[i]) && decl[i] != b'=' {
            i += 1;
        }
        if i == name_start {
            return None;
        }
        let name = &decl[name_start..i];
        while i < decl.len() && is_xml_whitespace(decl[i]) {
            i += 1;
        }
        if decl.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while i < decl.len() && is_xml_whitespace(decl[i]) {
            i += 1;
        }
        let quote = *decl.get(i)?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        i += 1;
        let value_start = i;
        while i < decl.len() && decl[i] != quote {
            i += 1;
        }
        if i >= decl.len() {
            return None;
        }
        if name == b"encoding" {
            return std::str::from_utf8(&decl[value_start..i]).ok();
        }
        i += 1;
    }
}

/// encoding_rs 严格增量解码: 不替换非法序列, 输出以 String 容量为界,
/// 达到 [`MAX_XML_BYTES`] 即 `Limit`, 不会先分配完整个超限结果.
fn decode_bounded(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Result<String, Error> {
    const CHUNK: usize = 64 * 1024;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut out = String::new();
    // 固定小块接收解码输出: String 的容量就是本轮输出上限, 超限时先追加
    // 已解码部分再清空, 不会为 64 MiB 上限一次性放大分配.
    let mut chunk = String::with_capacity(CHUNK.min(MAX_XML_BYTES));
    let mut src = bytes;
    loop {
        if chunk.len() == chunk.capacity() {
            append_decoded(&mut out, &chunk)?;
            chunk.clear();
        }
        let (result, read) = decoder.decode_to_string_without_replacement(src, &mut chunk, true);
        src = &src[read..];
        match result {
            encoding_rs::DecoderResult::InputEmpty => {
                if src.is_empty() {
                    append_decoded(&mut out, &chunk)?;
                    break;
                }
                if read == 0 {
                    return Err(Error::Invalid("XML decoding made no progress".to_string()));
                }
            }
            encoding_rs::DecoderResult::OutputFull => {
                if chunk.is_empty() {
                    return Err(Error::Invalid("XML decoding made no progress".to_string()));
                }
                append_decoded(&mut out, &chunk)?;
                chunk.clear();
            }
            encoding_rs::DecoderResult::Malformed(_, _) => {
                return Err(Error::Invalid(
                    "XML contains bytes invalid for its declared encoding".to_string(),
                ));
            }
        }
    }
    Ok(out)
}

fn append_decoded(out: &mut String, chunk: &str) -> Result<(), Error> {
    let Some(total) = out.len().checked_add(chunk.len()) else {
        return Err(Error::Limit("max_xml_bytes"));
    };
    if total > MAX_XML_BYTES {
        return Err(Error::Limit("max_xml_bytes"));
    }
    out.push_str(chunk);
    Ok(())
}

/// 受限 XML 入口 (FB2 / SVG 共用).
///
/// 在 roxmltree 建 DOM 之前:
/// 1. 拒绝超过 [`MAX_XML_BYTES`] 的文本;
/// 2. 字面 `<`、`=` 各不得超过 [`MAX_XML_NODES`] (roxmltree 会用它们估算
///    node/attribute Vec 容量, nodes_limit 管不到);
/// 3. 借用 quick-xml 只扫 prolog, 引号外出现 `[` 的内部 DTD subset 一律
///    拒绝; 只有外部 identifier 的 DOCTYPE 保留. 注释 / CDATA 里的示例
///    不算 DTD 声明 (CDATA 在 prolog 本身非法).
///
/// 随后 `allow_dtd = true` (否则 identifier-only DOCTYPE 也会失败),
/// `entity_resolver = None` (绝不读外部实体), `nodes_limit` 兜底.
pub(super) fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, Error> {
    // decode_xml 已去 BOM; 直接调用时也接受一个 UTF-8 BOM.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.len() > MAX_XML_BYTES {
        return Err(Error::Limit("max_xml_bytes"));
    }
    let mut angles = 0u32;
    let mut equals = 0u32;
    for &byte in text.as_bytes() {
        if byte == b'<' {
            angles += 1;
            if angles > MAX_XML_NODES {
                return Err(Error::Limit("max_xml_nodes"));
            }
        } else if byte == b'=' {
            equals += 1;
            if equals > MAX_XML_NODES {
                return Err(Error::Limit("max_xml_nodes"));
            }
        }
    }
    scan_prolog(text)?;
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        nodes_limit: MAX_XML_NODES,
        entity_resolver: None,
    };
    match roxmltree::Document::parse_with_options(text, options) {
        Ok(doc) => Ok(doc),
        Err(roxmltree::Error::NodesLimitReached) => Err(Error::Limit("max_xml_nodes")),
        Err(roxmltree::Error::AttributesLimitReached) => Err(Error::Limit("max_xml_attributes")),
        Err(roxmltree::Error::NamespacesLimitReached) => Err(Error::Limit("max_xml_namespaces")),
        Err(e) => Err(Error::Invalid(format!("invalid XML: {e}"))),
    }
}

/// 用借用事件只消费 prolog: 允许声明 / 注释 / PI / identifier-only
/// DOCTYPE, 拒绝内部 subset. 一到根元素就停止, 不做整篇 streaming 解析.
fn scan_prolog(text: &str) -> Result<(), Error> {
    let mut reader = XmlReader::from_str(text);
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Decl(_)) | Ok(XmlEvent::Comment(_)) | Ok(XmlEvent::PI(_)) => {}
            Ok(XmlEvent::DocType(doctype)) => {
                if has_internal_subset(&doctype) {
                    return Err(Error::Invalid(
                        "internal DTD subset is not allowed".to_string(),
                    ));
                }
            }
            Ok(XmlEvent::Start(_)) | Ok(XmlEvent::Empty(_)) => return Ok(()),
            Ok(XmlEvent::Text(text)) => {
                if !text.iter().all(|&b| is_xml_whitespace(b)) {
                    return Err(Error::Invalid(
                        "text before the XML root element".to_string(),
                    ));
                }
            }
            Ok(XmlEvent::CData(_)) | Ok(XmlEvent::GeneralRef(_)) => {
                return Err(Error::Invalid(
                    "unexpected markup in the XML prolog".to_string(),
                ));
            }
            Ok(XmlEvent::End(_)) | Ok(XmlEvent::Eof) => return Ok(()),
            Err(e) => {
                return Err(Error::Invalid(format!("invalid XML prolog: {e}")));
            }
        }
    }
}

/// DOCTYPE 内容里引号外的 `[` = 内部 subset. 引号内的字面 `[` (SYSTEM /
/// PUBLIC 标识符) 不触发.
fn has_internal_subset(doctype: &[u8]) -> bool {
    let mut quote = 0u8;
    for &byte in doctype {
        if quote == 0 {
            match byte {
                b'\'' | b'"' => quote = byte,
                b'[' => return true,
                _ => {}
            }
        } else if byte == quote {
            quote = 0;
        }
    }
    false
}

fn is_xml_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/* ---------- 图片 (EPUB / FB2 / CBZ 共用) ---------- */

/// 按字节签名识别图片 MIME. 不支持的 / 签名无效的返回 None; XML 解析的
/// 资源上限错误向上传播 (不能让超限输入被当成"缺图"静默降级).
/// 不在这里施加内联 16 MiB 上限 — CBZ 单页有自己的 32 MiB 预算.
pub(super) fn image_media_type(bytes: &[u8]) -> Result<Option<&'static str>, Error> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(png_kind(bytes));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(Some("image/jpeg"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(Some("image/gif"));
    }
    if bytes.starts_with(b"BM") && bytes.len() >= 14 {
        return Ok(Some("image/bmp"));
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(Some("image/webp"));
    }
    if is_avif(bytes) {
        return Ok(Some("image/avif"));
    }
    if bytes.starts_with(&[0, 0, 1, 0]) && ico_has_image(bytes) {
        return Ok(Some("image/x-icon"));
    }

    // 其余候选只可能是 XML (SVG): 与 FB2 共用同一受限解码 / DOM 入口.
    let text = match decode_xml(bytes) {
        Ok(text) => text,
        Err(Error::Limit(limit)) => return Err(Error::Limit(limit)),
        Err(_) => return Ok(None),
    };
    let doc = match parse_xml(&text) {
        Ok(doc) => doc,
        Err(Error::Limit(limit)) => return Err(Error::Limit(limit)),
        Err(_) => return Ok(None),
    };
    if doc.root_element().tag_name().name() == "svg" {
        Ok(Some("image/svg+xml"))
    } else {
        Ok(None)
    }
}

/// PNG / APNG: `acTL` 在首个 `IDAT` 之前出现即 APNG. 只做结构上界检查,
/// 不校验 CRC (真正解码交给浏览器); 结构明显断裂时视为无效.
fn png_kind(bytes: &[u8]) -> Option<&'static str> {
    let mut pos = 8usize;
    while pos.checked_add(8)? <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        if kind == b"acTL" {
            return Some("image/apng");
        }
        if kind == b"IDAT" {
            return Some("image/png");
        }
        if kind == b"IEND" {
            return None;
        }
        pos = pos.checked_add(12)?.checked_add(len)?;
        if pos > bytes.len() {
            return None;
        }
    }
    None
}

/// ISO BMFF `ftyp` 的 major brand 或 compatible brands 含 AVIF.
fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let major = &bytes[8..12];
    if major == b"avif" || major == b"avis" {
        return true;
    }
    let box_size = u32::from_be_bytes(bytes[..4].try_into().unwrap_or_default()) as usize;
    // compatible brands 只在 ftyp 头里; 即使 box_size 异常也不扫描整页.
    let end = box_size.min(bytes.len()).min(64);
    let mut pos = 16usize;
    while pos.checked_add(4).is_some_and(|next| next <= end) {
        let brand = &bytes[pos..pos + 4];
        if brand == b"avif" || brand == b"avis" {
            return true;
        }
        pos += 4;
    }
    false
}

fn ico_has_image(bytes: &[u8]) -> bool {
    if bytes.len() < 6 {
        return false;
    }
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    count > 0 && 6usize.saturating_add(count.saturating_mul(16)) <= bytes.len()
}

/// 候选图片后缀 (ASCII 大小写不敏感). 成员名可能用 `/` 或 `\`, 都不做
/// 文件系统 join.
pub(super) fn image_extension(name: &str) -> bool {
    const IMAGE_EXTENSIONS: &[&str] = &[
        "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "bmp", "webp", "avif", "ico", "svg",
    ];
    let segment = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let Some((_, ext)) = segment.rsplit_once('.') else {
        return false;
    };
    IMAGE_EXTENSIONS
        .iter()
        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
}

/// 严格解码内嵌图片 base64: 允许 XML whitespace; 先按有效字符数检查
/// 16 MiB 输出上限, 无 whitespace 时不复制输入; 非 canonical padding /
/// 非法字符失败; 实际输出再复核上限.
pub(super) fn decode_image_base64(text: &str) -> Result<Vec<u8>, Error> {
    const MAX_ENCODED: usize = MAX_INLINE_IMAGE_BYTES.div_ceil(3) * 4;
    let mut effective = 0usize;
    let mut has_whitespace = false;
    for &byte in text.as_bytes() {
        if is_xml_whitespace(byte) {
            has_whitespace = true;
        } else {
            effective += 1;
            if effective > MAX_ENCODED {
                return Err(Error::Limit("max_inline_image_bytes"));
            }
        }
    }
    if effective == 0 {
        return Ok(Vec::new());
    }
    if !effective.is_multiple_of(4) {
        return Err(Error::Invalid("invalid base64 image data".to_string()));
    }
    let decoded = if has_whitespace {
        let mut clean = Vec::with_capacity(effective);
        for &byte in text.as_bytes() {
            if !is_xml_whitespace(byte) {
                clean.push(byte);
            }
        }
        base64::engine::general_purpose::STANDARD.decode(clean)
    } else {
        base64::engine::general_purpose::STANDARD.decode(text.as_bytes())
    };
    let decoded = decoded.map_err(|_| Error::Invalid("invalid base64 image data".to_string()))?;
    if decoded.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(Error::Limit("max_inline_image_bytes"));
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out.extend_from_slice(&[0; 4]); // CRC 不参与签名判定
        out
    }

    #[test]
    fn png_and_apng_are_distinguished_before_first_idat() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"IDAT", &[0; 2]));
        png.extend(chunk(b"IEND", &[]));
        assert_eq!(image_media_type(&png).unwrap(), Some("image/png"));

        let mut apng = b"\x89PNG\r\n\x1a\n".to_vec();
        apng.extend(chunk(b"IHDR", &[0; 13]));
        apng.extend(chunk(b"acTL", &[0; 8]));
        apng.extend(chunk(b"IDAT", &[0; 2]));
        assert_eq!(image_media_type(&apng).unwrap(), Some("image/apng"));
    }

    #[test]
    fn common_binary_signatures_are_recognized() {
        assert_eq!(
            image_media_type(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap(),
            Some("image/jpeg")
        );
        assert_eq!(
            image_media_type(b"GIF89a\x01\x00").unwrap(),
            Some("image/gif")
        );
        assert_eq!(
            image_media_type(b"BM\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00").unwrap(),
            Some("image/bmp")
        );
        assert_eq!(
            image_media_type(b"RIFF\x24\x00\x00\x00WEBPVP8 ").unwrap(),
            Some("image/webp")
        );
        let mut avif = Vec::new();
        avif.extend_from_slice(&24u32.to_be_bytes());
        avif.extend_from_slice(b"ftypavif");
        avif.extend_from_slice(&[0; 12]);
        assert_eq!(image_media_type(&avif).unwrap(), Some("image/avif"));
    }

    #[test]
    fn extension_aliases_are_ascii_case_insensitive() {
        assert!(image_extension("1.BMP"));
        assert!(image_extension("photo.jfif"));
        assert!(image_extension("a/b\\C.APNG"));
        assert!(!image_extension("notes.txt"));
        assert!(!image_extension("noext"));
    }

    #[test]
    fn base64_accepts_xml_whitespace_and_rejects_padding_noise() {
        let decoded = decode_image_base64("iVBO\r\nRw0KGgo=").unwrap();
        assert_eq!(decoded, b"\x89PNG\r\n\x1a\n".to_vec());
        assert!(matches!(
            decode_image_base64("iVBO Rw0KGgo"),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            decode_image_base64("!!!!"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn too_many_angle_brackets_fail_before_dom_allocation() {
        let text = format!("<root>{}</root>", "<".repeat(MAX_XML_NODES as usize));
        assert!(matches!(
            parse_xml(&text),
            Err(Error::Limit("max_xml_nodes"))
        ));
    }

    #[test]
    fn internal_dtd_subset_is_rejected_but_identifier_only_is_not() {
        let subset = r#"<!DOCTYPE book [<!ENTITY a "aaaa">]><book>&a;</book>"#;
        assert!(matches!(parse_xml(subset), Err(Error::Invalid(_))));

        let external = r#"<!DOCTYPE book SYSTEM "book.dtd"><book/>"#;
        assert!(parse_xml(external).is_ok());

        let plain = r#"<?xml version="1.0"?><book><p>x</p></book>"#;
        assert!(parse_xml(plain).is_ok());
    }

    #[test]
    fn declared_windows_1251_is_decoded_strictly() {
        let source = r#"<?xml version="1.0" encoding="windows-1251"?><book>Привет</book>"#;
        let (bytes, _, had_errors) = encoding_rs::WINDOWS_1251.encode(source);
        assert!(!had_errors);
        let text = decode_xml(&bytes).unwrap();
        assert!(text.contains("Привет"));
    }
}
