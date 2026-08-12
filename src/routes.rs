//! HTTP 路由.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{assets, highlight, listing, markdown, office, org, render, sort::SortBy};

#[derive(Clone)]
pub struct AppState {
    /// 提供服务的文件系统根目录 (启动时已 canonicalize)
    pub root: PathBuf,
}

/// 元信息 API 与静态资源前缀: `/__/status`, `/__/static/...`.
pub fn app(root: PathBuf) -> Router {
    let state = AppState { root };
    Router::new()
        .route("/__/status", get(status))
        .route("/__/static/{*path}", get(static_asset))
        .route("/", get(serve_path))
        .route("/{*path}", get(serve_path))
        .with_state(state)
}

async fn status() -> &'static str {
    "simple static server is running.\n"
}

async fn static_asset(AxumPath(path): AxumPath<String>) -> Response {
    match path.as_str() {
        "css/materialize.min.css" => asset_response(assets::MATERIALIZE_CSS, "text/css"),
        "css/chapbook-theme.css" => asset_response(assets::THEME_CSS, "text/css"),
        "js/materialize.min.js" => asset_response(assets::MATERIALIZE_JS, "application/javascript"),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn asset_response(content: &'static str, content_type: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], content).into_response()
}

/// 列出目录内容或者返回文件
async fn serve_path(
    State(state): State<AppState>,
    path: Option<AxumPath<String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
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

    if metadata.is_dir() {
        // 如果是目录，列出目录内容
        let files = listing::list_dir(&fs_path, &state.root, sort_by);
        Html(render::dir_page(&state.root, &fs_path, &files, sort_by).into_string()).into_response()
    } else if fs_path.to_string_lossy().ends_with(".md") {
        // 如果是 .md 文件，使用 comrak 渲染
        serve_markdown(&fs_path).await
    } else if fs_path.to_string_lossy().ends_with(".org") {
        // 如果是 .org 文件，使用 orgize 渲染
        serve_org(&fs_path).await
    } else if let Some(format) = office::format_for_path(&fs_path) {
        // Office 文档 / CSV: anydoc 转 markdown 后走 comrak 渲染 (浏览器) 或原文 (脚本)
        serve_office(&fs_path, format, &metadata, &query, headers).await
    } else if let Some(lang) = highlight::language_for_path(&fs_path) {
        // 如果是源代码文件，按 Accept 协商: 浏览器 -> 高亮 HTML, 脚本 -> 原文
        serve_code(&fs_path, lang, &metadata, &query, headers).await
    } else {
        // 如果是文件，返回文件内容 (支持 Range 请求, 否则播放音视频无法任意快进)
        serve_file(&fs_path, headers).await
    }
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
/// 数学公式按原文显示; raw HTML 转义显示 (安全). 非 UTF-8 文件回退 text/plain 原文.
async fn serve_markdown(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => render_markdown_response(&source, path).await,
            Err(_) => serve_raw_text(path).await,
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
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (title, body) = markdown::render(source, &file_name);
    let page = render::doc_page(&title, &body);
    Html(render::inject_doc_style(&page)).into_response()
}

/// 语法高亮渲染源代码文件 (syntect, 纯 Rust; 提案 docs/2026-08-05-proposal-syntax-highlight-code-files.org, 方案 A).
///
/// 响应协商: 代码文件有浏览器与脚本两类消费者, 不能一律改返回 HTML:
/// - 默认按 Accept 头: 含 text/html (浏览器) -> 高亮 HTML; 否则 (curl 的 *​/*) -> 原文
/// - 显式覆盖: ?raw=1 强制原文, ?view=1 强制渲染 (用于分享链接)
///
/// 降级: 非 UTF-8 / 超大文件 -> ServeFile 裸字节透传; 未知语言 -> 纯文本转义显示.
async fn serve_code(
    path: &Path,
    lang: &str,
    metadata: &std::fs::Metadata,
    query: &HashMap<String, String>,
    headers: HeaderMap,
) -> Response {
    let wants_html =
        query.contains_key("view") || (!query.contains_key("raw") && accepts_html(&headers));
    // 大文件保护: 超过阈值不做渲染
    if !wants_html || metadata.len() > highlight::MAX_RENDER_BYTES {
        return serve_file(path, headers).await;
    }
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => {
                let title = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let body = highlight::code_block(&source, Some(lang));
                let page = render::doc_page(&title, &body);
                Html(render::inject_doc_style(&page)).into_response()
            }
            // 非 UTF-8: 不做编码猜测, 裸字节透传
            Err(_) => serve_file(path, headers).await,
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

/// Office 文档 / CSV 渲染 (anydoc 纯 Rust; 提案 docs/2026-08-12-proposal-render-office-documents.org, 方案 A).
///
/// 响应协商与代码文件一致 (浏览器与脚本两类消费者):
/// - 默认按 Accept 头: 含 text/html (浏览器) -> anydoc 转 GFM markdown 后经
///   comrak 渲染文档页 (TOC/锚点/高亮与 .md 一致); 否则 (curl 的 */*) -> 原文
/// - 显式覆盖: ?raw=1 强制原文, ?view=1 强制渲染 (用于分享链接)
///
/// 降级: 转换失败 (加密/损坏/超限, anydoc ConvertError) -> ServeFile 裸字节
/// 透传, 浏览器下载后由本地 Office 打开; 超大文件 (> office::MAX_RENDER_BYTES)
/// 同样透传. PDF 不在此路径 (浏览器原生打开, 见 office 模块).
async fn serve_office(
    path: &Path,
    format: office::Format,
    metadata: &std::fs::Metadata,
    query: &HashMap<String, String>,
    headers: HeaderMap,
) -> Response {
    let wants_html =
        query.contains_key("view") || (!query.contains_key("raw") && accepts_html(&headers));
    // 大文件保护: 转换是 CPU 活, 超过阈值不做渲染
    if !wants_html || metadata.len() > office::MAX_RENDER_BYTES {
        return serve_file(path, headers).await;
    }
    match tokio::fs::read(path).await {
        Ok(bytes) => match anydoc::to_markdown_bytes(&bytes, format) {
            Ok(markdown) => render_markdown_response(&markdown, path).await,
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "failed to convert office document, serving raw");
                serve_file(path, headers).await
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
/// 非 UTF-8 文件回退 text/plain 原文 (orgize 需要 UTF-8 输入).
async fn serve_org(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(source) => {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (title, body) = org::render(&source, &file_name);
                let page = render::doc_page(&title, &body);
                Html(render::inject_doc_style(&page)).into_response()
            }
            Err(_) => serve_raw_text(path).await,
        },
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to read org file");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// 渲染失败时的回退: 以纯文本返回原文.
async fn serve_raw_text(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            String::from_utf8_lossy(&bytes).into_owned(),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read org-mode file: {e}\n"),
        )
            .into_response(),
    }
}

async fn serve_file(path: &Path, headers: HeaderMap) -> Response {
    // ServeFile 依据请求头处理 Range / If-Modified-Since 等条件请求
    let mut request = Request::new(Body::empty());
    *request.headers_mut() = headers;
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => {
            let mut response = response.map(Body::new);
            ensure_text_charset(response.headers_mut());
            response
        }
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to serve file");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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
