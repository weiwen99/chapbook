//! HTTP 路由.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::ExtensionRejection;
use axum::extract::{ConnectInfo, Form, FromRequest, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use maud::html;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::meta::{
    content_disposition, display_path_markup, display_path_text, encode_relative_path,
    escape_os_name,
};
use crate::{assets, highlight, listing, markdown, office, org, render, sort::SortBy};

#[derive(Clone)]
struct AppState {
    /// 提供服务的文件系统根目录 (启动时已 canonicalize)
    root: PathBuf,
    /// 递归搜索并发门: 容量固定 1, 非空查询同一时刻只允许一个阻塞扫描.
    search_gate: Arc<Semaphore>,
    /// 本机打开 (native-open) 随机令牌: 启动时从 /dev/urandom 读取 16 字节的 hex.
    /// 绝不写入日志; 仅嵌入 loopback 可信页面, 且仅 POST 校验链通过时使用.
    token: String,
    /// 本机打开执行器 (生产 PlatformOpener; 测试注入 RecordingOpener/FailingOpener).
    opener: Arc<dyn OpenWith>,
}

/// 元信息 API / 搜索 / native-open / 静态资源与文件服务路由.
pub fn app(root: PathBuf) -> io::Result<Router> {
    let token = generate_token()?;
    let opener: Arc<dyn OpenWith> = Arc::new(PlatformOpener);
    Ok(app_with(root, token, opener))
}

/// 私有状态装配: 唯一以 AppState 构造 Router 的入口; 同时创建搜索并发门
/// (Semaphore(1)). 私有 — 不对外暴露 gate/opener/token 等测试适配能力.
fn app_with(root: PathBuf, token: String, opener: Arc<dyn OpenWith>) -> Router {
    let state = AppState {
        root,
        search_gate: Arc::new(Semaphore::new(1)),
        token,
        opener,
    };
    Router::new()
        .route("/__/status", get(status))
        .route("/__/search", get(serve_search))
        .route("/__/native-open", post(native_open))
        .route("/__/static/{*path}", get(static_asset))
        .route("/", get(serve_path))
        .route("/{*path}", get(serve_path))
        .with_state(state)
}

/// 从任意 `Read` 精确读取 16 字节并格式化为 32 位小写 hex (token reader seam).
/// 短读/读错误原样返回 io::Error — 不 panic, 不生成弱 token.
fn read_token(mut reader: impl io::Read) -> io::Result<String> {
    let mut bytes = [0u8; 16];
    reader.read_exact(&mut bytes)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(hex)
}

/// 启动 token: 打开 /dev/urandom 并精确读取 16 字节. open/read 错误原样返回
/// (启动失败并记录清晰错误, 绝不降级为弱 token / panic).
fn generate_token() -> io::Result<String> {
    read_token(std::fs::File::open("/dev/urandom")?)
}

/// 本机打开执行器 seam (私有): 同步打开路径, spawn 失败返回 io::Error.
trait OpenWith: Send + Sync {
    fn open(&self, path: &Path) -> io::Result<()>;
}

/// 生产 adapter: Linux `xdg-open` / macOS `open`; 其余平台返回 Unsupported.
/// tokio::process::Command: stdin/stdout/stderr 全 null, kill_on_drop(true),
/// 恰好一个 path argv, 无 shell. spawn 成功后 child 移入 reaper, 立即返回.
struct PlatformOpener;

impl OpenWith for PlatformOpener {
    fn open(&self, path: &Path) -> io::Result<()> {
        let program = if cfg!(target_os = "linux") {
            "xdg-open"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "native-open is not supported on this platform",
            ));
        };
        let child = tokio::process::Command::new(program)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let _handle = spawn_reaper(child);
        Ok(())
    }
}

/// 私有 reaper: 普通 tokio::spawn 异步 wait 子进程; wait 失败仅 warn.
/// runtime shutdown 时 wait 任务随 runtime 丢弃, child drop 触发 kill-on-drop,
/// 保证 Ctrl-C 不被挂起的 opener 阻塞.
fn spawn_reaper(mut child: tokio::process::Child) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = child.wait().await {
            tracing::warn!(error = %e, "native-open: child wait failed");
        }
    })
}

/// 字面 loopback Host 判定: `localhost` (大小写不敏感) / `127.0.0.1` (exact) /
/// `[::1]` (bracketed), 各可带任意端口后缀; 其余一律 false (fail closed).
fn host_is_loopback(host: &str) -> bool {
    let bytes = host.as_bytes();
    let host_part = if let Some(rest) = bytes.strip_prefix(b"[") {
        // bracketed IPv6: 必须 `[::1]` 或 `[::1]:<任意端口>`
        let Some(end) = rest.iter().position(|&b| b == b']') else {
            return false;
        };
        let tail = &rest[end + 1..];
        if !(tail.is_empty() || (tail.starts_with(b":") && tail.len() > 1)) {
            return false;
        }
        &rest[..end]
    } else {
        // 非 bracketed: 可选 `:<任意端口>` 后缀; host 部分含冒号 (未加括号的
        // IPv6 或多余冒号) 一律拒绝 (fail closed)
        match bytes.iter().rposition(|&b| b == b':') {
            Some(idx) => {
                if idx == 0 || idx + 1 == bytes.len() {
                    return false;
                }
                let host_part = &bytes[..idx];
                if host_part.contains(&b':') {
                    return false;
                }
                host_part
            }
            None => bytes,
        }
    };
    host_part.eq_ignore_ascii_case(b"localhost")
        || host_part.eq_ignore_ascii_case(b"127.0.0.1")
        || host_part.eq_ignore_ascii_case(b"::1")
}

/// token 嵌入条件 (目录页/搜索页): ConnectInfo 提取成功 + peer loopback +
/// Host 字面 loopback; 任一不满足即 false (fail closed, 无 ConnectInfo 亦不嵌入).
fn native_controls_allowed(
    connect: &Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    host: Option<&HeaderValue>,
) -> bool {
    let Ok(ConnectInfo(peer)) = connect else {
        return false;
    };
    if !peer.ip().is_loopback() {
        return false;
    }
    host.and_then(|h| h.to_str().ok())
        .is_some_and(host_is_loopback)
}

/// POST /__/native-open: form { path, token }, 校验链顺序固定, 任一失败即拒:
/// 1. peer 存在且 loopback (否则 403); 2. Host 字面 loopback (否则 403);
/// 3. Origin 存在且 lowercase 等于 "http://" + lowercase Host (否则 403);
/// 4. token 精确匹配 (否则 403); 5. path 经 resolve_within_root (越界 403);
/// 6. 目标存在 (否则 404); 7. opener 打开绝对路径 (成功 204, spawn 失败 500 + warn).
///
/// 绝不 percent-decode; token 与错误 token 均不得写入日志.
///
/// 安全修复 (复审 finding): handler 以原始 `Request<Body>` 作为最终 extractor —
/// Form body 的消费/解析严格推迟到校验链 1–3 全部通过之后, 链前失败 (无 peer /
/// 非 loopback / 坏 Host / 坏 Origin) 的请求 body 绝不被触碰; 链通过后
/// `Form::from_request` 拒绝 (缺失/错误 Content-Type 或畸形 body) 一律 fail
/// closed 403, 不进入 token/path 步骤.
async fn native_open(
    State(state): State<AppState>,
    connect: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Response {
    // 1. peer 存在且为 loopback (提取失败按非 loopback 处理, fail closed)
    let Ok(ConnectInfo(peer)) = connect else {
        tracing::warn!("native-open rejected: no peer address");
        return StatusCode::FORBIDDEN.into_response();
    };
    if !peer.ip().is_loopback() {
        tracing::warn!(peer = %peer, "native-open rejected: peer is not loopback");
        return StatusCode::FORBIDDEN.into_response();
    }
    // 2. Host 为字面 loopback
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        tracing::warn!("native-open rejected: missing Host");
        return StatusCode::FORBIDDEN.into_response();
    };
    if !host_is_loopback(host) {
        tracing::warn!(host, "native-open rejected: Host is not literal loopback");
        return StatusCode::FORBIDDEN.into_response();
    }
    // 3. Origin 存在且 (lowercase) 等于 "http://" + (lowercase) Host; null 即不匹配
    let origin_matches = headers
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .map(|origin| {
            origin.to_ascii_lowercase() == format!("http://{}", host.to_ascii_lowercase())
        })
        .unwrap_or(false);
    if !origin_matches {
        tracing::warn!("native-open rejected: Origin missing or mismatch");
        return StatusCode::FORBIDDEN.into_response();
    }
    // 3.5 (复审修复): 校验链 1–3 通过后才消费/解析 body. 缺失或错误
    //    Content-Type / 畸形 body -> Form::from_request 拒绝, fail closed 403,
    //    opener 绝不调用; 绝不在链前做任何 body 工作.
    let Ok(form) = Form::<HashMap<String, String>>::from_request(request, &()).await else {
        tracing::warn!("native-open rejected: malformed or missing form body");
        return StatusCode::FORBIDDEN.into_response();
    };
    // 4. token 精确匹配 (日志绝不包含 token 与请求提供的错误 token)
    if form.get("token").map(String::as_str) != Some(state.token.as_str()) {
        tracing::warn!("native-open rejected: token mismatch");
        return StatusCode::FORBIDDEN.into_response();
    }
    // 5. form path 直接交给 resolve_within_root (Form 已解码一次; handler 不再
    //    percent-decode, 字面 %2e%2e 保持 Normal 分量)
    let Some(requested) = form.get("path") else {
        tracing::warn!("native-open rejected: missing path field");
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(fs_path) = resolve_within_root(&state.root, requested) else {
        tracing::warn!(path = %requested, "native-open rejected: path escapes root");
        return StatusCode::FORBIDDEN.into_response();
    };
    // 6. 目标存在 (文件或目录; metadata 跟随符号链接, 悬空链接 -> 404)
    if std::fs::metadata(&fs_path).is_err() {
        tracing::warn!(path = %fs_path.display(), "native-open rejected: target does not exist");
        return StatusCode::NOT_FOUND.into_response();
    }
    // 7. opener 打开 resolve_within_root 后的绝对精确路径
    match state.opener.open(&fs_path) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, path = %fs_path.display(), "native-open: spawn failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// 递归搜索页: `GET /__/search?q=...` (可信 UI, anti-framing 头).
/// 空/纯空白查询直接返回提示页, 不遍历; 非空查询经进程级并发门
/// (容量 1): 忙 -> 429 + Retry-After: 1, 不排队第二次扫描.
async fn serve_search(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    connect: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    headers: HeaderMap,
) -> Response {
    // 仅 loopback peer + 字面 loopback Host 才嵌入 native token; 其余 fail closed
    let native_token = native_controls_allowed(&connect, headers.get(header::HOST))
        .then_some(state.token.as_str());
    let q = query.get("q").cloned().unwrap_or_default();
    let trimmed = q.trim().to_string();
    if trimmed.is_empty() {
        // 空/纯空白查询: 不遍历, 直接返回提示页
        let empty = listing::SearchResult {
            entries: Vec::new(),
            truncated: false,
        };
        return trusted_ui_response(
            render::search_page(&state.root, &trimmed, &empty, native_token).into_string(),
        );
    }
    // 归一化一次; listing::search 不再做逐候选归一化
    let normalized = trimmed.to_lowercase();
    let root = state.root.clone();
    match run_blocking_search(state.search_gate.clone(), move || {
        listing::search(&root, &normalized)
    })
    .await
    {
        Ok(result) => trusted_ui_response(
            render::search_page(&state.root, &trimmed, &result, native_token).into_string(),
        ),
        Err(BlockingSearchError::Busy) => {
            let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            response
        }
        Err(BlockingSearchError::Task(e)) => {
            tracing::warn!(error = %e, "search task panicked or was cancelled");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// 搜索并发门错误: Busy (未获取 permit, job 未被调用) / Task (阻塞任务失败).
#[derive(Debug)]
enum BlockingSearchError {
    Busy,
    Task(tokio::task::JoinError),
}

/// 私有并发/生命周期 seam: 容量 1 的信号量门 + spawn_blocking.
///
/// - `try_acquire_owned`: 失败即 Busy, **不调用 job**;
/// - permit 与 job 一起移入 `spawn_blocking` closure: 即使等待中的 handler
///   被取消 (客户端断开), 已启动的阻塞任务仍持有 permit 直到真实 job 结束;
/// - JoinError 显式映射为 Task.
async fn run_blocking_search<T, F>(
    gate: Arc<tokio::sync::Semaphore>,
    job: F,
) -> Result<T, BlockingSearchError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let Ok(permit) = gate.try_acquire_owned() else {
        return Err(BlockingSearchError::Busy);
    };
    let handle = tokio::task::spawn_blocking(move || {
        // permit 由 blocking task 持有到 job 结束 (panic 时随 unwind 释放)
        let _permit = permit;
        job()
    });
    handle.await.map_err(BlockingSearchError::Task)
}

async fn status() -> &'static str {
    "simple static server is running.\n"
}

async fn static_asset(AxumPath(path): AxumPath<String>) -> Response {
    match path.as_str() {
        "css/materialize.min.css" => asset_response(assets::MATERIALIZE_CSS, "text/css"),
        "css/chapbook-theme.css" => asset_response(assets::THEME_CSS, "text/css"),
        "css/chapbook-doc.css" => asset_response(render::DOC_STYLE, "text/css"),
        "js/materialize.min.js" => asset_response(assets::MATERIALIZE_JS, "application/javascript"),
        "js/chapbook-browser.js" => asset_response(assets::BROWSER_JS, "application/javascript"),
        "katex/katex.min.css" => asset_response(assets::KATEX_CSS, "text/css"),
        path if path.starts_with("katex/fonts/") => {
            let name = &path["katex/fonts/".len()..];
            match assets::KATEX_FONTS.iter().find(|(font, _)| *font == name) {
                Some((_, bytes)) => asset_bytes_response(bytes, "font/woff2"),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn asset_response(content: &'static str, content_type: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], content).into_response()
}

fn asset_bytes_response(content: &'static [u8], content_type: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], content).into_response()
}

/// 列出目录内容或者返回文件.
///
/// 响应模式集中解析 (download > raw > fragment > view, 见 [`parse_response_mode`]):
/// - Download/Raw: 全部经 [`serve_file`] 单一 ServeFile 出口 (attachment / 原文);
/// - Fragment: 全部经 [`serve_fragment`] 严格接口 (生成 HTML, 绝不裸内容);
/// - Full/默认: 目录 -> md -> org -> Office -> 代码 -> ServeFile 顺序分派,
///   Office/代码在默认模式下按 Accept 协商 (含 text/html -> 渲染页).
async fn serve_path(
    State(state): State<AppState>,
    path: Option<AxumPath<String>>,
    Query(query): Query<HashMap<String, String>>,
    connect: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    headers: HeaderMap,
) -> Response {
    // 仅 loopback peer + 字面 loopback Host 才嵌入 native token; 其余 fail closed
    let native_token = native_controls_allowed(&connect, headers.get(header::HOST))
        .then_some(state.token.as_str());
    // sort 参数非法时返回 404 (而非 400), 保持既有行为
    let sort_by = match query.get("sort") {
        Some(s) => match s.parse::<SortBy>() {
            Ok(sort_by) => sort_by,
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        },
        None => SortBy::default(),
    };

    let decoded = path.map(|AxumPath(p)| p).unwrap_or_default();
    // 防路径穿越: 逐分量解析, 拒绝溢出根目录 (返回 403).
    // 注意: 词法的 startsWith(root) 前缀比较不会消除 `..` 分量,
    // `GET /%2e%2e/%2e%2e/etc/passwd` 可以绕过检查读到根目录外的文件.
    // `..` 分量弹到根目录时直接拒绝. 符号链接保持跟随语义.
    let Some(fs_path) = resolve_within_root(&state.root, &decoded) else {
        return StatusCode::FORBIDDEN.into_response();
    };

    // 如果文件不存在，返回 404 Not Found (metadata 跟随符号链接, 悬空链接 -> 404)
    let Ok(metadata) = std::fs::metadata(&fs_path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // 响应模式: 类型分派前集中解析一次 (优先级 download > raw > fragment > view)
    let mode = parse_response_mode(&query);

    if metadata.is_dir() {
        // 目录忽略 download/raw/view (对目录无意义), 恒返回可信完整目录页;
        // 唯一例外: fragment 模式返回迷你列表片段 (面板下钻)
        if mode == Some(ResponseMode::Fragment) {
            return serve_fragment(&state, &fs_path, &metadata, FragmentKind::Dir).await;
        }
        let files = listing::list_dir(&fs_path, &state.root, sort_by);
        trusted_ui_response(
            render::dir_page(&state.root, &fs_path, &files, sort_by, native_token).into_string(),
        )
    } else {
        match mode {
            Some(ResponseMode::Download) => serve_file(&fs_path, headers, true).await,
            Some(ResponseMode::Raw) => serve_file(&fs_path, headers, false).await,
            Some(ResponseMode::Fragment) => {
                serve_fragment(&state, &fs_path, &metadata, classify_fragment(&fs_path)).await
            }
            // Full (?view=1 强制渲染) 与默认 (无模式键): 类型分派;
            // 仅 Office/代码在默认且 Accept 不含 text/html 时按 Raw 返回原文
            Some(ResponseMode::Full) | None => {
                if fs_path.to_string_lossy().ends_with(".md") {
                    // 如果是 .md 文件，使用 comrak 渲染 (headers 透传供非 UTF-8 回退)
                    serve_markdown(&fs_path, headers).await
                } else if fs_path.to_string_lossy().ends_with(".org") {
                    // 如果是 .org 文件，使用 orgize 渲染 (headers 透传供非 UTF-8 回退)
                    serve_org(&fs_path, headers).await
                } else if let Some(format) = office::format_for_path(&fs_path) {
                    // Office 文档 / CSV: 浏览器 -> anydoc 转 markdown 渲染, 脚本 -> 原文
                    if mode == Some(ResponseMode::Full) || accepts_html(&headers) {
                        serve_office(&fs_path, format, &metadata, headers).await
                    } else {
                        serve_file(&fs_path, headers, false).await
                    }
                } else if let Some(lang) = highlight::language_for_path(&fs_path) {
                    // 源代码文件: 浏览器 -> 高亮 HTML, 脚本 -> 原文
                    if mode == Some(ResponseMode::Full) || accepts_html(&headers) {
                        serve_code(&fs_path, lang, &metadata, headers).await
                    } else {
                        serve_file(&fs_path, headers, false).await
                    }
                } else {
                    // 其他文件: ServeFile (支持 Range 请求, 否则播放音视频无法任意快进)
                    serve_file(&fs_path, headers, false).await
                }
            }
        }
    }
}

/// 响应模式: 在 serve_path 内集中解析一次, 先于类型分派.
/// 优先级 (查询键存在性): download > raw > fragment > view;
/// 无任何键时返回 None, 由类型默认裁定 (md/org Full; Office/代码按 Accept; 其余 Raw).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResponseMode {
    Download,
    Raw,
    Fragment,
    Full,
}

fn parse_response_mode(query: &HashMap<String, String>) -> Option<ResponseMode> {
    if query.contains_key("download") {
        Some(ResponseMode::Download)
    } else if query.contains_key("raw") {
        Some(ResponseMode::Raw)
    } else if query.contains_key("fragment") {
        Some(ResponseMode::Fragment)
    } else if query.contains_key("view") {
        Some(ResponseMode::Full)
    } else {
        None
    }
}

/// fragment 类型分派: 与 serve_path 的类型判定顺序一致
/// (目录 -> md -> org -> Office -> 代码 -> 媒体 -> 不支持).
enum FragmentKind {
    Dir,
    Markdown,
    Org,
    Office(office::Format),
    Code(&'static str),
    Media(MediaKind),
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MediaKind {
    Image,
    Video,
    Audio,
}

fn classify_fragment(path: &Path) -> FragmentKind {
    if path.to_string_lossy().ends_with(".md") {
        FragmentKind::Markdown
    } else if path.to_string_lossy().ends_with(".org") {
        FragmentKind::Org
    } else if let Some(format) = office::format_for_path(path) {
        FragmentKind::Office(format)
    } else if let Some(lang) = highlight::language_for_path(path) {
        FragmentKind::Code(lang)
    } else if let Some(kind) = media_kind_for_path(path) {
        FragmentKind::Media(kind)
    } else {
        FragmentKind::Unsupported
    }
}

/// 媒体扩展名: 图片 / 视频 / 音频 (大小写不敏感).
fn media_kind_for_path(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?;
    let kind = match ext.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "ico" => MediaKind::Image,
        "mp4" | "webm" | "mov" | "m4v" => MediaKind::Video,
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" => MediaKind::Audio,
        _ => return None,
    };
    Some(kind)
}

/// fragment 严格接口: 唯一的 Fragment 出口. 任何路径都不返回裸内容 —
/// 成功返回生成的安全 HTML fragment, 失败返回占位 fragment (200),
/// 绝不回退 ServeFile / text/plain.
///
/// 成功 fragment 的外层 wrapper 携带当前路径的 encoded identity
/// (data-native-path-encoded, ASCII-only), 供 JS 安装面板 action;
/// 路径不可编码时省略该属性 (action 保持禁用).
async fn serve_fragment(
    state: &AppState,
    path: &Path,
    metadata: &std::fs::Metadata,
    kind: FragmentKind,
) -> Response {
    let rel = path.strip_prefix(&state.root).unwrap_or(Path::new(""));
    let encoded = encode_relative_path(rel);
    let title = display_path_text(rel);

    match kind {
        FragmentKind::Dir => {
            // 目录 fragment 自带 encoded wrapper (render::dir_fragment), 不重复包装
            let files = listing::list_dir(path, &state.root, SortBy::default());
            fragment_response(render::dir_fragment(&state.root, path, &files))
        }
        FragmentKind::Markdown => {
            let inner = match read_utf8(path).await {
                Ok(source) => {
                    let file_name = file_name_text(path);
                    let (_, body) = markdown::render(&source, &file_name);
                    render::doc_fragment(&body)
                }
                Err(reason) => no_preview(&reason),
            };
            fragment_response(fragment_wrapper(encoded.as_deref(), rel, inner))
        }
        FragmentKind::Org => {
            let inner = match read_utf8(path).await {
                Ok(source) => {
                    let file_name = file_name_text(path);
                    let (_, body) = org::render(&source, &file_name);
                    render::doc_fragment(&body)
                }
                Err(reason) => no_preview(&reason),
            };
            fragment_response(fragment_wrapper(encoded.as_deref(), rel, inner))
        }
        FragmentKind::Office(format) => {
            let inner = if metadata.len() > office::MAX_RENDER_BYTES {
                no_preview("文件过大（超过 32 MiB 限制）")
            } else {
                match tokio::fs::read(path).await {
                    Ok(bytes) => match anydoc::to_markdown_bytes(&bytes, format) {
                        Ok(markdown) => {
                            let file_name = file_name_text(path);
                            let (_, body) = markdown::render(&markdown, &file_name);
                            render::doc_fragment(&body)
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                path = %path.display(),
                                "fragment: failed to convert office document"
                            );
                            no_preview("转换失败")
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            path = %path.display(),
                            "fragment: failed to read office document"
                        );
                        no_preview("无法读取文件")
                    }
                }
            };
            fragment_response(fragment_wrapper(encoded.as_deref(), rel, inner))
        }
        FragmentKind::Code(lang) => {
            let inner = if metadata.len() > highlight::MAX_RENDER_BYTES {
                no_preview("文件过大（超过 1 MiB 限制）")
            } else {
                match read_utf8(path).await {
                    Ok(source) => render::doc_fragment(&highlight::code_block(&source, Some(lang))),
                    Err(reason) => no_preview(&reason),
                }
            };
            fragment_response(fragment_wrapper(encoded.as_deref(), rel, inner))
        }
        FragmentKind::Media(kind) => {
            let inner = media_fragment(encoded.as_deref(), &title, kind);
            fragment_response(fragment_wrapper(encoded.as_deref(), rel, inner))
        }
        FragmentKind::Unsupported => {
            let inner = no_preview("不支持预览此文件类型");
            fragment_response(fragment_wrapper(encoded.as_deref(), rel, inner))
        }
    }
}

/// fragment 响应的统一头: 200, text/html; charset=utf-8, nosniff.
fn fragment_response(html: String) -> Response {
    let mut response = Html(html).into_response();
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// 外层 fragment wrapper: 携带当前路径的 encoded identity, 显示 title (文本 attr)
/// 与逐段 bdi 隔离的标题 markup (供 JS 安装到预览标题);
/// encoded 为 None (路径不可编码) 时省略 data 属性, JS action 保持禁用.
fn fragment_wrapper(encoded: Option<&str>, rel: &Path, inner: String) -> String {
    let inner = maud::PreEscaped(inner);
    let title = display_path_text(rel);
    let title_markup = display_path_markup(rel);
    let wrapper = match encoded {
        Some(enc) => html! {
            div class="cb-fragment" data-native-path-encoded=(enc) title=(title) {
                span.cb-frag-title hidden { (title_markup) }
                (inner)
            }
        },
        None => html! {
            div class="cb-fragment" title=(title) {
                span.cb-frag-title hidden { (title_markup) }
                (inner)
            }
        },
    };
    wrapper.into_string()
}

/// 占位 fragment: 200, maud 转义文案, 不含任何原始文件字节.
fn no_preview(reason: &str) -> String {
    html! {
        div class="cb-doc" {
            p class="cb-no-preview" { "无法预览：" (reason) }
        }
    }
    .into_string()
}

/// 媒体 fragment 内容: src 恒为 Full/raw 的 exact URL (无 fragment query),
/// 由当前 encoded path 构造; 路径不可编码时无有效 src, 返回占位.
fn media_fragment(encoded: Option<&str>, title: &str, kind: MediaKind) -> String {
    let Some(encoded) = encoded else {
        return no_preview("路径无法编码");
    };
    let src = format!("/{encoded}");
    match kind {
        MediaKind::Image => html! {
            img src=(src) alt=(title) loading="lazy";
        }
        .into_string(),
        MediaKind::Video => html! {
            video src=(src) controls preload="metadata";
        }
        .into_string(),
        MediaKind::Audio => html! {
            audio src=(src) controls preload="metadata";
        }
        .into_string(),
    }
}

/// 读取文件并严格 UTF-8 解码; 失败返回占位理由文案 (绝不 lossy 透传).
async fn read_utf8(path: &Path) -> Result<String, String> {
    match tokio::fs::read(path).await {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| "文件不是有效的 UTF-8 文本".to_string()),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "fragment: failed to read file");
            Err("无法读取文件".to_string())
        }
    }
}

/// 可见 fallback 标题 (md/org/office/code 全文页与 fragment 文档标题共用):
/// 经 `meta::escape_os_name` 安全显示 codec, 绝不 lossy 透传 basename.
fn file_name_text(path: &Path) -> String {
    path.file_name().map(escape_os_name).unwrap_or_default()
}

/// 可信 UI (目录页/搜索页) 统一出口: 添加 anti-framing 头.
/// 绝不添加 sandbox (可信 UI 需要同源能力).
fn trusted_ui_response(markup: String) -> Response {
    let mut response = Html(markup).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

/// 将 URL 路径解析为 root 内的文件系统路径. 溢出 root 时返回 None.
fn resolve_within_root(root: &Path, decoded: &str) -> Option<PathBuf> {
    let mut buf = root.to_path_buf();
    for comp in Path::new(decoded).components() {
        match comp {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::Normal(segment) => buf.push(segment),
            Component::ParentDir => {
                if buf == *root {
                    return None;
                }
                buf.pop();
            }
        }
    }
    Some(buf)
}

/// 使用 comrak 将 Markdown 渲染为 HTML (纯 Rust, 无子进程, 不会失败).
/// 自建 TOC + 标题锚点 (与 org 同一 slug 函数); 代码块 syntect 高亮.
/// YAML front matter 的 title 进入 `<title>` 与 title-block-header.
/// 数学公式按原文显示; raw HTML 转义显示 (安全). 非 UTF-8 文件回退
/// ServeFile 裸字节 (单一 raw 出口, 带 sandbox CSP), 原请求头
/// (Range/条件请求) 原样透传.
async fn serve_markdown(path: &Path, headers: HeaderMap) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => render_markdown_response(&source, path).await,
            Err(_) => serve_file(path, headers, false).await,
        },
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to read markdown file");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// 共享: 把 markdown 源渲染为完整文档页 (.md 与 anydoc 转换结果共用).
/// comrak 纯 Rust 渲染 (表格/任务列表/footnotes 等 GFM 扩展), 不会失败.
async fn render_markdown_response(source: &str, path: &Path) -> Response {
    let file_name = file_name_text(path);
    let (title, body) = markdown::render(source, &file_name);
    let page = render::doc_page(&title, &body);
    Html(render::inject_doc_style(&page)).into_response()
}

/// Full 模式的源代码渲染 (syntect, 纯 Rust; 提案 docs/2026-08-05-proposal-syntax-highlight-code-files.org, 方案 A).
///
/// 响应协商与 ?raw=1/?view=1 覆盖已由 serve_path 的 ResponseMode 统一裁定:
/// 到达此处的请求必然要 HTML 页面.
/// 降级: 非 UTF-8 / 超大文件 -> ServeFile 裸字节透传; 未知语言 -> 纯文本转义显示.
async fn serve_code(
    path: &Path,
    lang: &str,
    metadata: &std::fs::Metadata,
    headers: HeaderMap,
) -> Response {
    // 大文件保护: 超过阈值不做渲染
    if metadata.len() > highlight::MAX_RENDER_BYTES {
        return serve_file(path, headers, false).await;
    }
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => {
                let title = file_name_text(path);
                let body = highlight::code_block(&source, Some(lang));
                let page = render::doc_page(&title, &body);
                Html(render::inject_doc_style(&page)).into_response()
            }
            // 非 UTF-8: 不做编码猜测, 裸字节透传
            Err(_) => serve_file(path, headers, false).await,
        },
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to read code file");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"))
}

/// Full 模式的 Office 文档 / CSV 渲染 (anydoc 纯 Rust; 提案
/// docs/2026-08-12-proposal-render-office-documents.org, 方案 A).
///
/// 响应协商与 ?raw=1/?view=1 覆盖已由 serve_path 的 ResponseMode 统一裁定:
/// 到达此处的请求必然要 HTML 页面.
/// 降级: 转换失败 (加密/损坏/超限, anydoc ConvertError) 或超大文件
/// (> office::MAX_RENDER_BYTES) -> ServeFile 裸字节透传. PDF 不在此路径
/// (浏览器原生打开, 见 office 模块).
async fn serve_office(
    path: &Path,
    format: office::Format,
    metadata: &std::fs::Metadata,
    headers: HeaderMap,
) -> Response {
    // 大文件保护: 转换是 CPU 活, 超过阈值不做渲染
    if metadata.len() > office::MAX_RENDER_BYTES {
        return serve_file(path, headers, false).await;
    }
    match tokio::fs::read(path).await {
        Ok(bytes) => match anydoc::to_markdown_bytes(&bytes, format) {
            Ok(markdown) => render_markdown_response(&markdown, path).await,
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "failed to convert office document, serving raw");
                serve_file(path, headers, false).await
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to read office document");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// 渲染失败时的回退: 以纯文本返回原文.
///
/// 管线: `Org::parse` (纯内存解析) → 预扫描生成 TOC/元数据 → 自定义 HtmlHandler
/// 输出 body fragment (标题带锚点) → `render::doc_page` 自组完整页面 → 注入 DOC_STYLE.
///
/// 与旧 pandoc/emacs 管线的差异 (Phase 1, 见提案文档): 代码块无语法高亮 token
/// (内容完整); 4 级标题完整保留 (pandoc/emacs 会结构性丢失); 无子进程/超时兜底.
/// 非 UTF-8 文件回退 ServeFile 裸字节 (orgize 需要 UTF-8 输入; 单一 raw 出口),
/// 原请求头 (Range/条件请求) 原样透传.
async fn serve_org(path: &Path, headers: HeaderMap) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => {
                let file_name = file_name_text(path);
                let (title, body) = org::render(&source, &file_name);
                let page = render::doc_page(&title, &body);
                Html(render::inject_doc_style(&page)).into_response()
            }
            Err(_) => serve_file(path, headers, false).await,
        },
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to read org file");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// 唯一的 raw-content 出口: 全部走 ServeFile (Range/条件请求能力保留),
/// text/* charset 修补。主动内容与 download 附加 `sandbox allow-scripts` CSP
/// (绝不 allow-same-origin); audio/video 省略 document sandbox 以兼容 Chromium，
/// 并强制 nosniff，防止伪装成媒体的主动内容被重新解释。
/// download 时附加 `Content-Disposition: attachment` (RFC 8187, 仅 basename).
async fn serve_file(path: &Path, headers: HeaderMap, download: bool) -> Response {
    // ServeFile 依据请求头处理 Range / If-Modified-Since 等条件请求
    let mut request = Request::new(Body::empty());
    *request.headers_mut() = headers;
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => {
            let mut response = response.map(Body::new);
            let playable_status = matches!(
                response.status(),
                StatusCode::OK | StatusCode::PARTIAL_CONTENT
            );
            let headers = response.headers_mut();
            ensure_text_charset(headers);
            if download || !playable_status || !is_playable_media(headers) {
                headers.insert(
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static("sandbox allow-scripts"),
                );
            } else {
                headers.insert(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                );
            }
            if download {
                let disposition = content_disposition(
                    path.file_name().unwrap_or_else(|| std::ffi::OsStr::new("")),
                );
                if let Ok(value) = disposition.parse() {
                    headers.insert(header::CONTENT_DISPOSITION, value);
                }
            }
            response
        }
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to serve file");
            let mut response = StatusCode::INTERNAL_SERVER_ERROR.into_response();
            response.headers_mut().insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("sandbox allow-scripts"),
            );
            response
        }
    }
}

fn is_playable_media(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("audio/") || value.starts_with("video/"))
}

/// mime_guess 给出的 text/* Content-Type 不带 charset (如 text/plain, text/x-rust),
/// 浏览器会按 Latin-1 等本地编码猜测, UTF-8 中文直接乱码. 统一补 charset=utf-8.
fn ensure_text_charset(headers: &mut HeaderMap) {
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        let ct = content_type.to_str().unwrap_or_default().to_string();
        if ct.starts_with("text/")
            && !ct.contains("charset")
            && let Ok(value) = format!("{ct}; charset=utf-8").parse()
        {
            headers.insert(header::CONTENT_TYPE, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// native-open 校验链拒绝用例: (peer, host, origin, body, label).
    type ValidationCase<'a> = (
        Option<SocketAddr>,
        &'a str,
        Option<&'a str>,
        String,
        &'a str,
    );

    fn q(keys: &[&str]) -> HashMap<String, String> {
        keys.iter()
            .map(|k| (k.to_string(), "1".to_string()))
            .collect()
    }

    async fn body_to_string(res: Response) -> String {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("read body");
        String::from_utf8(bytes.to_vec()).expect("body is utf-8")
    }

    /// 优先级总序 download > raw > fragment > view; 无键 -> None (类型默认).
    #[test]
    fn response_mode_priority_total_order() {
        assert_eq!(
            parse_response_mode(&q(&["download", "raw", "fragment", "view"])),
            Some(ResponseMode::Download)
        );
        assert_eq!(
            parse_response_mode(&q(&["download", "fragment"])),
            Some(ResponseMode::Download)
        );
        assert_eq!(
            parse_response_mode(&q(&["raw", "fragment", "view"])),
            Some(ResponseMode::Raw)
        );
        assert_eq!(
            parse_response_mode(&q(&["fragment", "view"])),
            Some(ResponseMode::Fragment)
        );
        assert_eq!(parse_response_mode(&q(&["view"])), Some(ResponseMode::Full));
        assert_eq!(parse_response_mode(&q(&[])), None);
        assert_eq!(parse_response_mode(&q(&["sort"])), None);
    }

    /// 键存在性判定: 空值键也算存在 (Query 表单语义).
    #[test]
    fn response_mode_keys_count_by_existence() {
        let mut query = HashMap::new();
        query.insert("download".to_string(), String::new());
        assert_eq!(parse_response_mode(&query), Some(ResponseMode::Download));
    }

    /// 媒体扩展名集合与大小写不敏感.
    #[test]
    fn media_kind_extensions_exact() {
        for ext in ["png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "ico"] {
            assert_eq!(
                media_kind_for_path(Path::new(&format!("a.{ext}"))),
                Some(MediaKind::Image),
                "{ext}"
            );
        }
        for ext in ["mp4", "webm", "mov", "m4v"] {
            assert_eq!(
                media_kind_for_path(Path::new(&format!("a.{ext}"))),
                Some(MediaKind::Video),
                "{ext}"
            );
        }
        for ext in ["mp3", "flac", "ogg", "opus", "wav", "m4a"] {
            assert_eq!(
                media_kind_for_path(Path::new(&format!("a.{ext}"))),
                Some(MediaKind::Audio),
                "{ext}"
            );
        }
        assert_eq!(
            media_kind_for_path(Path::new("a.MP3")),
            Some(MediaKind::Audio),
            "大小写不敏感"
        );
        assert_eq!(media_kind_for_path(Path::new("a.pdf")), None);
        assert_eq!(media_kind_for_path(Path::new("a.mp2")), None);
        assert_eq!(media_kind_for_path(Path::new("noext")), None);
    }

    /// fragment 类型分派顺序: 目录 -> md -> org -> Office -> 代码 -> 媒体 -> 不支持.
    #[test]
    fn classify_fragment_dispatches_all_kinds() {
        assert!(matches!(
            classify_fragment(Path::new("x.md")),
            FragmentKind::Markdown
        ));
        assert!(matches!(
            classify_fragment(Path::new("x.org")),
            FragmentKind::Org
        ));
        assert!(matches!(
            classify_fragment(Path::new("x.csv")),
            FragmentKind::Office(_)
        ));
        assert!(matches!(
            classify_fragment(Path::new("x.rs")),
            FragmentKind::Code("rust")
        ));
        assert!(matches!(
            classify_fragment(Path::new("x.png")),
            FragmentKind::Media(MediaKind::Image)
        ));
        assert!(matches!(
            classify_fragment(Path::new("x.mp4")),
            FragmentKind::Media(MediaKind::Video)
        ));
        assert!(matches!(
            classify_fragment(Path::new("x.xyz123")),
            FragmentKind::Unsupported
        ));
    }

    /// 预持有唯一 permit -> Busy, job 不被调用; 释放后可正常执行.
    #[tokio::test]
    async fn preheld_permit_yields_busy_without_calling_job() {
        let gate = Arc::new(Semaphore::new(1));
        let held = gate.clone().try_acquire_owned().expect("preheld permit");
        let called = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&called);
        let result = run_blocking_search(gate.clone(), move || {
            flag.store(true, Ordering::SeqCst);
            1
        })
        .await;
        assert!(matches!(result, Err(BlockingSearchError::Busy)));
        assert!(!called.load(Ordering::SeqCst), "job must not run on Busy");
        drop(held);
        let result = run_blocking_search(gate, || 2).await;
        assert_eq!(result.expect("after release"), 2);
    }

    /// 并发生命周期 + handler 取消: 第一个 job 启动后, abort 等待中的 async
    /// task (客户端断开), permit 必须仍由 blocking closure 持有 -> 第二次调用
    /// Busy; 释放后不用 sleep, 同步 real closure 结束 (permit 恢复), 第三次成功.
    /// 全用 oneshot channel 同步.
    #[tokio::test]
    async fn running_job_blocks_second_then_release_allows_third() {
        let gate = Arc::new(Semaphore::new(1));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();

        let first = tokio::spawn({
            let gate = Arc::clone(&gate);
            async move {
                run_blocking_search(gate, move || {
                    let _ = started_tx.send(());
                    // 阻塞直到测试释放; permit 由该 blocking closure 持有
                    let _ = release_rx.blocking_recv();
                    "first"
                })
                .await
            }
        });

        // 真实 job 已启动 => permit 已被 blocking task 持有
        started_rx.await.expect("first job must start");

        // 客户端断开: abort 等待中的 handler (spawn_blocking 任务不受影响, 继续运行)
        first.abort();
        let join = first.await.expect_err("aborted handler must not join");
        assert!(join.is_cancelled(), "abort must cancel the awaiting task");

        // 取消后 permit 仍在 blocking closure 中: 第二次调用必须 Busy
        let second = run_blocking_search(gate.clone(), || "second").await;
        assert!(
            matches!(second, Err(BlockingSearchError::Busy)),
            "second must be Busy while first blocking job continues after cancellation"
        );

        // 释放阻塞 job; 不用 sleep: 轮询信号量直到 blocking closure 真实返回
        // (permit 在 closure 结束时释放 — real closure completion)
        let _ = release_tx.send(());
        let recovered = loop {
            match gate.clone().try_acquire_owned() {
                Ok(permit) => break permit,
                Err(_) => tokio::task::yield_now().await,
            }
        };
        drop(recovered);

        let third = run_blocking_search(gate, || "third").await;
        assert_eq!(third.expect("third ok"), "third");
    }

    /// job panic -> Task(JoinError), permit 随 unwind 释放, 之后可恢复.
    #[tokio::test]
    async fn panic_maps_to_task_error_and_permit_recovers() {
        let gate = Arc::new(Semaphore::new(1));
        let result = run_blocking_search(gate.clone(), || -> i32 { panic!("boom") }).await;
        assert!(
            matches!(result, Err(BlockingSearchError::Task(_))),
            "panic must map to Task"
        );
        let again = run_blocking_search(gate, || 7).await;
        assert_eq!(again.expect("permit must recover"), 7);
    }

    /// HTTP 429 映射 (私有装配/私有 gate): 预持有 permit -> Busy -> 429 +
    /// Retry-After: 1; 空白查询绕过并发门; 释放 permit 后恢复 200.
    /// 原先的公开 app_with_state/AppState 集成测试依赖已移除 — 此测试用
    /// 私有 AppState 直接驱动 serve_search.
    #[tokio::test]
    async fn search_handler_busy_maps_to_429_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "1").unwrap();
        let gate = Arc::new(Semaphore::new(1));
        let held = gate.clone().try_acquire_owned().expect("preheld permit");
        let state = AppState {
            root: dir.path().to_path_buf(),
            search_gate: gate.clone(),
            token: "test-token".to_string(),
            opener: Arc::new(RecordingOpener::new()),
        };

        // 忙 -> 429 + Retry-After: 1
        let mut query = HashMap::new();
        query.insert("q".to_string(), "1.txt".to_string());
        let res = serve_search(
            State(state.clone()),
            Query(query),
            connect_info_missing().await,
            HeaderMap::new(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            res.headers()
                .get(header::RETRY_AFTER)
                .expect("Retry-After")
                .to_str()
                .unwrap(),
            "1"
        );

        // 空白查询不经过并发门
        let mut query = HashMap::new();
        query.insert("q".to_string(), "   ".to_string());
        let res = serve_search(
            State(state.clone()),
            Query(query),
            connect_info_missing().await,
            HeaderMap::new(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(body_to_string(res).await.contains("没有找到匹配的文件。"));

        // 释放 permit 后恢复
        drop(held);
        let mut query = HashMap::new();
        query.insert("q".to_string(), "1.txt".to_string());
        let res = serve_search(
            State(state),
            Query(query),
            connect_info_missing().await,
            HeaderMap::new(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            body_to_string(res).await.contains(r#"href="/1.txt""#),
            "recovery search must list the match"
        );
    }

    /// 可见标题 codec (md/org/office/code 全文页与 fragment 文档标题共用
    /// file_name_text): bidi / default-ignorable / 非法 UTF-8 字节 / 多空白
    /// 必须经 escape_os_name 转义, 绝不 lossy 透传原始字符.
    #[test]
    fn file_name_text_uses_safe_display_codec() {
        assert_eq!(
            file_name_text(Path::new("\u{202E}evil.md")),
            "\\u{202E}evil.md"
        );
        assert_eq!(file_name_text(Path::new("a\u{200B}b.md")), "a\\u{200B}b.md");
        assert_eq!(file_name_text(Path::new("a  b.md")), "a\\x20\\x20b.md");
        assert_eq!(file_name_text(Path::new("My File.md")), "My File.md");
        assert_eq!(file_name_text(Path::new("x.md")), "x.md");
        assert_eq!(file_name_text(Path::new("")), "");
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(
                file_name_text(Path::new(OsStr::from_bytes(b"bad\xFF.md"))),
                "bad\\xFF.md"
            );
        }
    }

    /// Markdown 全文页与 Office 转换页共用 render_markdown_response:
    /// 无 front matter 时 <title> 回退文件名, 必须输出转义形式而非原始
    /// bidi/ignorable 字符.
    #[tokio::test]
    async fn markdown_and_office_title_escapes_unsafe_filename() {
        let path = Path::new("\u{202E}evil\u{200B}name.md");
        let res = render_markdown_response("# hello\nbody", path).await;
        let html = body_to_string(res).await;
        assert!(
            html.contains("\\u{202E}evil\\u{200B}name.md"),
            "escaped fallback title missing: {html}"
        );
        assert!(
            !html.contains('\u{202E}'),
            "raw bidi control leaked: {html}"
        );
        assert!(!html.contains('\u{200B}'), "raw ZWSP leaked: {html}");
    }

    /// Org 全文页: 无 #+TITLE 时 <title> 回退文件名, 必须输出转义形式.
    #[tokio::test]
    async fn org_page_title_escapes_unsafe_filename() {
        let dir = tempfile::tempdir().unwrap();
        let name = "\u{202E}evil\u{200B}report.org";
        std::fs::write(dir.path().join(name), "* Head\nbody").unwrap();
        let res = serve_org(&dir.path().join(name), HeaderMap::new()).await;
        let html = body_to_string(res).await;
        assert!(
            html.contains("\\u{202E}evil\\u{200B}report.org"),
            "escaped fallback title missing: {html}"
        );
        assert!(
            !html.contains('\u{202E}'),
            "raw bidi control leaked: {html}"
        );
        assert!(!html.contains('\u{200B}'), "raw ZWSP leaked: {html}");
    }

    /// 代码全文页: <title> 回退文件名必须转义.
    #[tokio::test]
    async fn code_page_title_escapes_unsafe_filename() {
        let dir = tempfile::tempdir().unwrap();
        let name = "\u{202E}evil\u{200B}main.rs";
        std::fs::write(dir.path().join(name), "fn main() {}\n").unwrap();
        let metadata = std::fs::metadata(dir.path().join(name)).unwrap();
        let res = serve_code(&dir.path().join(name), "rust", &metadata, HeaderMap::new()).await;
        let html = body_to_string(res).await;
        assert!(
            html.contains("\\u{202E}evil\\u{200B}main.rs"),
            "escaped fallback title missing: {html}"
        );
        assert!(
            !html.contains('\u{202E}'),
            "raw bidi control leaked: {html}"
        );
        assert!(!html.contains('\u{200B}'), "raw ZWSP leaked: {html}");
    }

    // ---- Task 6: native-open 安全链 / token 生命周期 / opener reaping ----

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn loopback_peer() -> SocketAddr {
        "127.0.0.1:34567".parse().expect("loopback addr")
    }

    fn nonloopback_peer() -> SocketAddr {
        "192.168.1.99:34567".parse().expect("nonloopback addr")
    }

    /// 提取失败的 ConnectInfo (无 extension): 通过真实 extractor 获取 rejection,
    /// 因为 rejection 的内部字段私有, 无法直接构造.
    async fn connect_info_missing() -> Result<ConnectInfo<SocketAddr>, ExtensionRejection> {
        use axum::extract::FromRequestParts;
        let (mut parts, _) = axum::http::Request::new(()).into_parts();
        ConnectInfo::<SocketAddr>::from_request_parts(&mut parts, &()).await
    }

    fn page_request(peer: Option<SocketAddr>, host: &str, uri: &str) -> Request<Body> {
        let mut builder = Request::builder().uri(uri).header(header::HOST, host);
        if let Some(peer) = peer {
            builder = builder.extension(ConnectInfo(peer));
        }
        builder.body(Body::empty()).expect("page request")
    }

    fn native_request(
        body: &str,
        peer: Option<SocketAddr>,
        host: &str,
        origin: Option<&str>,
    ) -> Request<Body> {
        native_request_with_ct(
            Body::from(body.to_string()),
            Some("application/x-www-form-urlencoded"),
            peer,
            host,
            origin,
        )
    }

    /// 同 native_request, 但 Content-Type 可缺失 (None) 或任意给定 —
    /// 用于证明 body 解析严格晚于校验链 1–3 (peer/Host/Origin).
    fn native_request_with_ct(
        body: Body,
        content_type: Option<&str>,
        peer: Option<SocketAddr>,
        host: &str,
        origin: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/__/native-open")
            .header(header::HOST, host);
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if let Some(peer) = peer {
            builder = builder.extension(ConnectInfo(peer));
        }
        builder.body(body).expect("native request")
    }

    /// 模拟浏览器 URLSearchParams 的 application/x-www-form-urlencoded 编码:
    /// alphanumeric + `*` `-` `.` `_` 保留, 空格 -> `+`, 其余 %XX (UTF-8 字节).
    fn form_encode(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for &byte in value.as_bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                    out.push(byte as char)
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    }

    /// Form identity round-trip 矩阵: 中文 / + % # ? \ / 字面 %2e%2e 与 %2F /
    /// 原始 bidi/default-ignorable / CR LF CRLF / 24 个非 U+0020 White_Space /
    /// U+0020 单双内部与前导尾随变体.
    fn identity_matrix_names() -> Vec<String> {
        let mut names: Vec<String> = vec![
            "中文.txt".into(),
            "a+b.txt".into(),
            "a%b.txt".into(),
            "a#b.txt".into(),
            "a?b.txt".into(),
            "a\\b.txt".into(),
            "%2e%2e".into(),
            "%2F".into(),
            "%2F.txt".into(),
            "report%0D.command".into(),
            "\u{202E}evil.md".into(),
            "a\u{200B}b.md".into(),
            "\u{2060}x.txt".into(),
            "x\u{FEFF}.txt".into(),
            "report\u{000D}.command".into(),
            "report\u{000A}.command".into(),
            "report\u{000D}\u{000A}.command".into(),
            "a b.txt".into(),
            "a  b.txt".into(),
            " leading.txt".into(),
            "trailing .txt".into(),
        ];
        let whitespace = "\u{0009}\u{000A}\u{000B}\u{000C}\u{000D}\u{0085}\u{00A0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}";
        assert_eq!(
            whitespace.chars().count(),
            24,
            "必须恰好 24 个非 U+0020 White_Space 字符"
        );
        assert!(
            whitespace.chars().all(|c| c.is_whitespace() && c != ' '),
            "矩阵字符必须是 White_Space 且非 U+0020"
        );
        names.extend(whitespace.chars().map(|c| c.to_string()));
        names
    }

    /// 测试 opener: 记录每次收到的完整 PathBuf (exact identity, 禁止 lossy 比较).
    #[derive(Default)]
    struct RecordingOpener {
        opened: Mutex<Vec<PathBuf>>,
    }

    impl RecordingOpener {
        fn new() -> Self {
            Self::default()
        }
        fn opened(&self) -> Vec<PathBuf> {
            self.opened.lock().expect("opener lock").clone()
        }
    }

    impl OpenWith for RecordingOpener {
        fn open(&self, path: &Path) -> io::Result<()> {
            self.opened
                .lock()
                .expect("opener lock")
                .push(path.to_path_buf());
            Ok(())
        }
    }

    /// 测试 opener: spawn 恒失败 (对应 500 分支).
    struct FailingOpener;

    impl OpenWith for FailingOpener {
        fn open(&self, _path: &Path) -> io::Result<()> {
            Err(io::Error::other("intentional spawn failure"))
        }
    }

    /// token reader: 精确 16 字节 -> 32 位小写 hex.
    #[test]
    fn token_reader_exact_hex() {
        let bytes: Vec<u8> = (0u8..16).collect();
        let token = read_token(bytes.as_slice()).expect("exact 16 bytes");
        assert_eq!(token.len(), 32);
        assert_eq!(token, "000102030405060708090a0b0c0d0e0f");
        assert_eq!(token, token.to_ascii_lowercase(), "必须全小写");
    }

    /// token reader: 短读 (15 / 0 字节) -> io::Error, 不 panic.
    #[test]
    fn token_reader_short_read_is_error() {
        assert!(read_token(&[1u8, 2, 3][..]).is_err());
        assert!(read_token(&[][..]).is_err());
    }

    /// token reader: 读取失败原样返回错误 (failing reader seam).
    #[test]
    fn token_reader_failing_reader_is_error() {
        struct Failing;
        impl io::Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("boom"))
            }
        }
        let err = read_token(Failing).expect_err("failing reader must error");
        assert_eq!(err.kind(), io::ErrorKind::Other);
    }

    /// Host 字面 loopback 判定表: localhost (大小写不敏感) / 127.0.0.1 / [::1],
    /// 各可带任意端口; 其余一律拒绝 (fail closed).
    #[test]
    fn host_is_loopback_table() {
        for host in [
            "localhost",
            "LOCALHOST",
            "LocalHost",
            "localhost:8888",
            "localhost:abc",
            "127.0.0.1",
            "127.0.0.1:8888",
            "[::1]",
            "[::1]:8888",
            "[::1]:https",
        ] {
            assert!(host_is_loopback(host), "host 必须通过: {host:?}");
        }
        for host in [
            "",
            "example.com",
            "example.com:8080",
            "localhost.evil.com",
            "127.0.0.2",
            "127.0.0.1.5",
            "::1",
            "::1:8888",
            "[::2]",
            "[::1]x",
            "[::1]:",
            "localhost:",
            "127.0.0.1:8888:9",
            " localhost",
            "[::1",
        ] {
            assert!(!host_is_loopback(host), "host 必须拒绝: {host:?}");
        }
    }

    /// token 嵌入: 目录页与搜索页仅在 peer loopback + Host 字面 loopback 时
    /// 嵌入 token 与 native 控制; 其余 fail closed (无 meta, 无按钮).
    #[tokio::test]
    async fn token_embedded_only_for_loopback_peer_and_literal_loopback_host() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::new(RecordingOpener::new()),
        );

        // 目录页正向: 各种字面 loopback Host
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "localhost:8888",
            "[::1]:8888",
        ] {
            let res = app
                .clone()
                .oneshot(page_request(Some(loopback_peer()), host, "/"))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "host {host:?}");
            let body = body_to_string(res).await;
            assert!(body.contains("cb-native-open-token"), "host {host:?}");
            assert!(
                body.contains(&format!(r#"content="{TOKEN}""#)),
                "host {host:?}: token meta 缺失"
            );
            assert!(
                body.contains(r#"data-cb-action="native""#),
                "host {host:?}: native 控制缺失"
            );
        }

        // 目录页负向: 无 peer / 非 loopback peer / 非字面 Host
        for (peer, host) in [
            (None, "localhost"),
            (Some(nonloopback_peer()), "localhost"),
            (Some(loopback_peer()), "evil.com"),
            (Some(loopback_peer()), "localhost.evil.com"),
        ] {
            let res = app
                .clone()
                .oneshot(page_request(peer, host, "/"))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "peer {peer:?} host {host:?}");
            let body = body_to_string(res).await;
            assert!(
                !body.contains("cb-native-open-token"),
                "peer {peer:?} host {host:?}: 不得嵌入 token"
            );
            assert!(
                !body.contains(r#"data-cb-action="native""#),
                "peer {peer:?} host {host:?}: 不得渲染 native 控制"
            );
        }

        // 搜索页: 正向 / 负向
        let res = app
            .clone()
            .oneshot(page_request(
                Some(loopback_peer()),
                "localhost",
                "/__/search?q=1.txt",
            ))
            .await
            .unwrap();
        assert!(body_to_string(res).await.contains("cb-native-open-token"));
        let res = app
            .clone()
            .oneshot(page_request(
                Some(loopback_peer()),
                "evil.com",
                "/__/search?q=1.txt",
            ))
            .await
            .unwrap();
        assert!(!body_to_string(res).await.contains("cb-native-open-token"));
    }

    /// native-open 校验链: peer/Host/Origin/token 任一失败 -> 403, opener 绝不调用.
    #[tokio::test]
    async fn native_open_validation_chain_403_without_opener_call() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        let body_ok = format!("path=1.txt&token={TOKEN}");
        let cases: Vec<ValidationCase<'_>> = vec![
            (
                None,
                "localhost",
                Some("http://localhost"),
                body_ok.clone(),
                "无 peer (ConnectInfo 缺失)",
            ),
            (
                Some(nonloopback_peer()),
                "localhost",
                Some("http://localhost"),
                body_ok.clone(),
                "非 loopback peer",
            ),
            (
                Some(loopback_peer()),
                "evil.com",
                Some("http://localhost"),
                body_ok.clone(),
                "Host 非字面 loopback",
            ),
            (
                Some(loopback_peer()),
                "localhost",
                None,
                body_ok.clone(),
                "Origin 缺失",
            ),
            (
                Some(loopback_peer()),
                "localhost",
                Some("http://evil.com"),
                body_ok.clone(),
                "Origin 不匹配",
            ),
            (
                Some(loopback_peer()),
                "localhost",
                Some("null"),
                body_ok.clone(),
                "Origin 为 null",
            ),
            (
                Some(loopback_peer()),
                "localhost",
                Some("http://localhost"),
                "path=1.txt&token=wrong-token".to_string(),
                "token 错误",
            ),
            (
                Some(loopback_peer()),
                "localhost",
                Some("http://localhost"),
                format!("token={TOKEN}"),
                "path 字段缺失",
            ),
        ];
        for (peer, host, origin, body, label) in cases {
            let res = app
                .clone()
                .oneshot(native_request(&body, peer, host, origin))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{label}");
        }
        assert!(
            opener.opened().is_empty(),
            "opener 不得被调用: {:?}",
            opener.opened()
        );
    }

    /// Task-6 复审修复 (security review finding): form body 解析必须严格晚于
    /// 校验链 1–3 (peer / Host / Origin). 缺失 Content-Type 的 body 在链前失败
    /// 的组合 (无 peer / 非 loopback / 坏 Host / 坏 Origin) 一律 403 — 绝不允许
    /// axum extractor 阶段的 415 抢先; opener 绝不调用.
    #[tokio::test]
    async fn native_open_missing_content_type_chain_403_no_opener() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        let body = format!("path=1.txt&token={TOKEN}");
        let cases: Vec<(Option<SocketAddr>, &str, Option<&str>, &str)> = vec![
            (
                None,
                "localhost",
                Some("http://localhost"),
                "无 peer (ConnectInfo 缺失)",
            ),
            (
                Some(nonloopback_peer()),
                "localhost",
                Some("http://localhost"),
                "非 loopback peer",
            ),
            (
                Some(loopback_peer()),
                "evil.com",
                Some("http://localhost"),
                "Host 非字面 loopback",
            ),
            (
                Some(loopback_peer()),
                "localhost",
                Some("http://evil.com"),
                "Origin 不匹配",
            ),
        ];
        for (peer, host, origin, label) in cases {
            let res = app
                .clone()
                .oneshot(native_request_with_ct(
                    Body::from(body.clone()),
                    None,
                    peer,
                    host,
                    origin,
                ))
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "{label}: 缺失 Content-Type 的 body 不得在链前产生非 403 状态"
            );
        }
        assert!(
            opener.opened().is_empty(),
            "opener 不得被调用: {:?}",
            opener.opened()
        );
    }

    /// 同上, 但 Content-Type 存在却错误 (text/plain / application/json):
    /// Form extractor 同样在链前拒绝 -> 415; 修复后必须由链的 403 接管.
    #[tokio::test]
    async fn native_open_wrong_content_type_chain_403_no_opener() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        let body = format!("path=1.txt&token={TOKEN}");
        for content_type in ["text/plain", "application/json"] {
            let cases: Vec<(Option<SocketAddr>, &str, Option<&str>, &str)> = vec![
                (
                    None,
                    "localhost",
                    Some("http://localhost"),
                    "无 peer (ConnectInfo 缺失)",
                ),
                (
                    Some(nonloopback_peer()),
                    "localhost",
                    Some("http://localhost"),
                    "非 loopback peer",
                ),
                (
                    Some(loopback_peer()),
                    "evil.com",
                    Some("http://localhost"),
                    "Host 非字面 loopback",
                ),
                (
                    Some(loopback_peer()),
                    "localhost",
                    Some("http://evil.com"),
                    "Origin 不匹配",
                ),
            ];
            for (peer, host, origin, label) in cases {
                let res = app
                    .clone()
                    .oneshot(native_request_with_ct(
                        Body::from(body.clone()),
                        Some(content_type),
                        peer,
                        host,
                        origin,
                    ))
                    .await
                    .unwrap();
                assert_eq!(
                    res.status(),
                    StatusCode::FORBIDDEN,
                    "Content-Type {content_type:?} {label}: 链前失败必须 403"
                );
            }
        }
        assert!(
            opener.opened().is_empty(),
            "opener 不得被调用: {:?}",
            opener.opened()
        );
    }

    /// 授权边界 (peer/Host/Origin 全部通过) 下, 缺失/错误 Content-Type 与畸形
    /// body (非法 UTF-8 字节) 仍 fail closed (403) 且 opener 绝不调用 —
    /// form 解析/拒绝发生在链 1–3 之后, 任何 body 问题都不得绕过校验链.
    #[tokio::test]
    async fn native_open_authorized_bad_form_fails_closed_403_no_opener() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        let form = format!("path=1.txt&token={TOKEN}");
        let variants: Vec<(Body, Option<&str>, &str)> = vec![
            (Body::from(form.clone()), None, "缺失 Content-Type"),
            (
                Body::from(form.clone()),
                Some("text/plain"),
                "错误 Content-Type",
            ),
            (
                Body::from(vec![0xffu8, 0xfe]),
                Some("application/x-www-form-urlencoded"),
                "畸形 body (非法 UTF-8 字节)",
            ),
        ];
        for (body, content_type, label) in variants {
            let res = app
                .clone()
                .oneshot(native_request_with_ct(
                    body,
                    content_type,
                    Some(loopback_peer()),
                    "localhost",
                    Some("http://localhost"),
                ))
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "{label}: 授权边界畸形 form 必须 fail closed 403"
            );
        }
        assert!(
            opener.opened().is_empty(),
            "opener 不得被调用: {:?}",
            opener.opened()
        );
    }

    /// 越界 403 / 不存在 404 只在完整校验链通过后判定.
    #[tokio::test]
    async fn native_open_traversal_403_and_missing_404_after_chain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        // 链 (peer/Host/Origin/token) 全部通过后: 越界 -> 403
        let body = format!("path={}&token={TOKEN}", form_encode("../../etc/passwd"));
        let res = app
            .clone()
            .oneshot(native_request(
                &body,
                Some(loopback_peer()),
                "localhost",
                Some("http://localhost"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // 链全部通过后: 目标不存在 -> 404
        let body = format!("path=nope.txt&token={TOKEN}");
        let res = app
            .clone()
            .oneshot(native_request(
                &body,
                Some(loopback_peer()),
                "localhost",
                Some("http://localhost"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        assert!(opener.opened().is_empty());
    }

    /// 正向: Host/Origin 大小写不敏感, 204, RecordingOpener 恰好一次收到
    /// resolve_within_root 后的绝对完整 PathBuf.
    #[tokio::test]
    async fn native_open_valid_204_exact_absolute_path_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        let body = format!("path=1.txt&token={TOKEN}");
        let res = app
            .clone()
            .oneshot(native_request(
                &body,
                Some(loopback_peer()),
                "LOCALHOST",
                Some("HTTP://LOCALHOST"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let opened = opener.opened();
        assert_eq!(opened, vec![dir.path().join("1.txt")], "exact 绝对 PathBuf");
    }

    /// opener spawn 失败 -> 500.
    #[tokio::test]
    async fn native_open_spawn_failure_is_500() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1.txt"), "x").unwrap();
        let app = app_with(
            dir.path().to_path_buf(),
            TOKEN.to_string(),
            Arc::new(FailingOpener),
        );
        let body = format!("path=1.txt&token={TOKEN}");
        let res = app
            .clone()
            .oneshot(native_request(
                &body,
                Some(loopback_peer()),
                "localhost",
                Some("http://localhost"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Form identity round-trip: 全部矩阵名 (含 CR/LF/CRLF, 24 个非 U+0020
    /// White_Space, bidi/default-ignorable, 字面 percent) 单次 form decode 后
    /// 精确到达完整 PathBuf; 无显示转义进入 identity; 每个恰被打开一次.
    #[tokio::test]
    async fn native_open_form_identity_round_trip_exact_pathbuf() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let names = identity_matrix_names();
        for name in &names {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let opener = Arc::new(RecordingOpener::new());
        let app = app_with(
            root.clone(),
            TOKEN.to_string(),
            Arc::clone(&opener) as Arc<dyn OpenWith>,
        );

        for name in &names {
            let body = format!("path={}&token={TOKEN}", form_encode(name));
            let res = app
                .clone()
                .oneshot(native_request(
                    &body,
                    Some(loopback_peer()),
                    "localhost",
                    Some("http://localhost"),
                ))
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::NO_CONTENT,
                "name {name:?} wire {:?}",
                form_encode(name)
            );
        }

        let opened = opener.opened();
        assert_eq!(
            opened.len(),
            names.len(),
            "每个矩阵名必须恰好打开一次: {opened:?}"
        );
        for (index, name) in names.iter().enumerate() {
            assert_eq!(
                opened[index],
                root.join(name),
                "exact PathBuf identity (无显示转义)"
            );
        }
    }

    /// 子进程 helper 模式 (当前 test executable, 零外部依赖):
    /// CHAPBOOK_REAPER_MODE=long -> 长时间运行不退出; 其余 -> 立即退出.
    #[test]
    fn reaper_process_helper() {
        if let Some("long") = std::env::var("CHAPBOOK_REAPER_MODE").ok().as_deref() {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
    }

    /// reaper 正常路径: 立即退出的 child 被异步 wait 回收, JoinHandle 快速完成.
    #[tokio::test]
    async fn reaper_immediate_child_completes() {
        let child = tokio::process::Command::new(std::env::current_exe().expect("test exe"))
            .arg("reaper_process_helper")
            .arg("--nocapture")
            .env("CHAPBOOK_REAPER_MODE", "exit")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn helper");
        let start = std::time::Instant::now();
        spawn_reaper(child).await.expect("reaper join");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "reaper 必须在 child 退出后快速完成"
        );
    }

    /// runtime shutdown 不挂起 + kill-on-drop: 长运行 helper 放入独立 runtime,
    /// drop runtime 在固定短超时内返回 (证明不存在 spawn_blocking(wait) 挂起),
    /// 且 child 被 kill-on-drop 终止 (stdout pipe EOF — 通道同步, 无 sleep race).
    #[test]
    fn reaper_runtime_drop_returns_quickly_and_kills_long_child() {
        let mut cmd = tokio::process::Command::new(std::env::current_exe().expect("test exe"));
        cmd.arg("reaper_process_helper")
            .arg("--nocapture")
            .env("CHAPBOOK_REAPER_MODE", "long")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("standalone runtime");
        let mut child = rt
            .block_on(async { cmd.spawn() })
            .expect("spawn long helper");
        let pid = child.id().expect("child pid");
        let mut stdout = child.stdout.take().expect("piped stdout");

        // reaper 在独立 runtime 内运行; 观察线程等待 child stdout EOF (进程死亡)
        let reaper = {
            let _guard = rt.enter();
            spawn_reaper(child)
        };
        drop(reaper);
        let (eof_tx, eof_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            use tokio::io::AsyncReadExt;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("observer runtime");
            rt.block_on(async {
                let mut buf = [0u8; 1];
                loop {
                    match stdout.read(&mut buf).await {
                        Ok(0) | Err(_) => break, // EOF: 子进程已终止
                        Ok(_) => {}
                    }
                }
            });
            let _ = eof_tx.send(());
        });

        let start = std::time::Instant::now();
        drop(rt);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "runtime shutdown 不得在 pending wait 上挂起"
        );
        eof_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("kill-on-drop 必须终止长运行 child (pid {pid})"));
    }
}
