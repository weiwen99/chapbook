//! 书籍预览的公共 HTTP 行为: 内容、资产、锚点、响应模式与不可信输入边界。

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chapbook::routes;
use tower::ServiceExt;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

// 有效的 8×8 图片, 浏览器 smoke 使用同样的像素内容而非仅有 magic 的假图片。
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAI0lEQVR4AdzKMQ0AAAwCwQYjlVjhFQEOYOeT3w57Tzcm1AEEAAD//5d4Ta8AAAAGSURBVAMAFx0T0f9pl7sAAAAASUVORK5CYII=";
const JPEG: &str = concat!(
    "/9j/4AAQSkZJRgABAQAAAQABAAD/4gHYSUNDX1BST0ZJTEUAAQEAAAHIAAAAAAQwAABtbnRyUkdCIFhZWiAH4AABAAEAAAAAAABh",
    "Y3NwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAA9tYAAQAAAADTLQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAAAAAAAAAAAAAAAAAAAlkZXNjAAAA8AAAACRyWFlaAAABFAAAABRnWFlaAAABKAAAABRiWFlaAAABPAAAABR3dHB0AAAB",
    "UAAAABRyVFJDAAABZAAAAChnVFJDAAABZAAAAChiVFJDAAABZAAAAChjcHJ0AAABjAAAADxtbHVjAAAAAAAAAAEAAAAMZW5VUwAA",
    "AAgAAAAcAHMAUgBHAEJYWVogAAAAAAAAb6IAADj1AAADkFhZWiAAAAAAAABimQAAt4UAABjaWFlaIAAAAAAAACSgAAAPhAAAts9Y",
    "WVogAAAAAAAA9tYAAQAAAADTLXBhcmEAAAAAAAQAAAACZmYAAPKnAAANWQAAE9AAAApbAAAAAAAAAABtbHVjAAAAAAAAAAEAAAAM",
    "ZW5VUwAAACAAAAAcAEcAbwBvAGcAbABlACAASQBuAGMALgAgADIAMAAxADb/2wBDAAYEBQYFBAYGBQYHBwYIChAKCgkJChQODwwQ",
    "FxQYGBcUFhYaHSUfGhsjHBYWICwgIyYnKSopGR8tMC0oMCUoKSj/2wBDAQcHBwoIChMKChMoGhYaKCgoKCgoKCgoKCgoKCgoKCgo",
    "KCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCj/wAARCAAIAAgDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAb/",
    "xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAABQb/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIR",
    "AxEAPwCTACo5/9k=",
);

fn app(root: &Path) -> Router {
    routes::app(root.to_path_buf()).unwrap()
}

async fn request(app: &Router, uri: &str, accept: &str) -> Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::ACCEPT, accept)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn bytes(response: Response<Body>) -> Vec<u8> {
    to_bytes(response.into_body(), 128 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec()
}

async fn text(response: Response<Body>) -> String {
    String::from_utf8(bytes(response).await).unwrap()
}

fn strict_fragment(response: &Response<Body>) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert!(!response.headers().contains_key(header::X_FRAME_OPTIONS));
}

fn isolated_image(response: &Response<Body>) {
    assert_eq!(
        response.headers()[header::CONTENT_SECURITY_POLICY],
        "sandbox"
    );
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(!response.headers().contains_key(header::X_FRAME_OPTIONS));
}

fn attrs<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!("{name}=\"");
    html.split(&needle)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

fn linked_target<'a>(html: &'a str, label: &str) -> &'a str {
    html.split("<a ")
        .filter_map(|rest| rest.split_once("</a>"))
        .filter(|(anchor, _)| anchor.contains(label))
        .find_map(|(anchor, _)| attrs(anchor, "href").first().copied())
        .and_then(|href| href.strip_prefix('#'))
        .unwrap_or_else(|| panic!("missing internal link for {label}"))
}

fn unique_resolved_fragments(html: &str) {
    let ids = attrs(html, "id");
    let unique: HashSet<_> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "duplicate target ID");
    for href in attrs(html, "href") {
        if let Some(target) = href.strip_prefix('#').filter(|target| !target.is_empty()) {
            assert!(unique.contains(target), "unresolved fragment {href}");
        }
    }
}

fn bmp() -> Vec<u8> {
    // 2×2, 24-bit BGR, 每行补齐到 4 字节; 红色像素。
    let mut bytes = b"BM".to_vec();
    bytes.extend(70_u32.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(54_u32.to_le_bytes());
    bytes.extend(40_u32.to_le_bytes());
    bytes.extend(2_i32.to_le_bytes());
    bytes.extend(2_i32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(24_u16.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend([0; 16]);
    bytes.extend([0, 0, 255, 0, 0, 255, 0, 0].repeat(2));
    bytes
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn epub(picture: &[u8], picture_name: &str) -> Vec<u8> {
    let package = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="book-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Preview book</dc:title><dc:identifier id="book-id">book-test</dc:identifier><dc:language>en</dc:language></metadata><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="picture" href="images/{picture_name}" media-type="image/png"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#
    );
    let one = format!(
        r##"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title><script src="/attack.js">window.bookAttack=1</script></head><body><h1 id="chapter-one">Repeated</h1><p><a href="two.xhtml#destination">jump-chapter</a> <a href="two.xhtml">jump-start</a></p><p><img src="images/{picture_name}" alt="Embedded picture"/></p><p><a href="#a:b">jump-colon</a> <a href="#a-b">jump-dash</a></p><p id="a:b">ColonTarget</p><p id="a-b">DashTarget</p><p><a href="JaVa&#x09;ScRiPt:alert(1)">unsafe-EPUB</a></p><ol start="3"><li>Third</li><li>Fourth<ul><li>Child</li></ul></li></ol><table><tr><th>Name</th><th>Value</th></tr><tr><td rowspan="2">Merged</td><td>One</td></tr><tr><td>Two</td></tr></table><pre><code>&lt;script&gt;not code&lt;/script&gt;</code></pre></body></html>"##
    );
    let picture_path = format!("OPS/images/{picture_name}");
    zip_bytes(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("OPS/package.opf", package.as_bytes()),
        ("OPS/one.xhtml", one.as_bytes()),
        ("OPS/two.xhtml", br##"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Repeated</h1><p id="destination">Chapter landing</p><p><a href="one.xhtml#chapter-one">back-to-first</a></p></body></html>"##),
        (&picture_path, picture),
    ])
}

fn fb2(encoding: &str) -> String {
    let picture = STANDARD.encode(bmp());
    format!(
        r##"<?xml version="1.0" encoding="{encoding}"?>
<!DOCTYPE FictionBook SYSTEM "https://example.invalid/FictionBook.dtd">
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info><book-title>Привет</book-title><coverpage><image l:href="#cover"/></coverpage><annotation><p>Book annotation</p></annotation></title-info></description>
<body><title><p>Main body</p></title><section id="chapter"><title><p>Section</p></title><p id="from-note">Before <strong>bold</strong> between <emphasis>italic</emphasis> after <a type="note" l:href="#n1">[1]</a>.</p><p><image l:href="#cover"/><image l:href="#missing" alt="Missing illustration"/></p><p><a l:href="java&#x09;script:alert(1)">unsafe-FB2</a></p><poem><stanza><v>Verse one</v><v>Verse two</v></stanza></poem><table><tr><th>Key</th><th>Value</th></tr><tr><td>Alpha</td><td>Beta</td></tr></table></section></body>
<body name="notes"><title><p>Notes</p></title><section id="n1"><title><p>1</p></title><p>Footnote detail <a l:href="#from-note">return-note</a></p></section></body>
<binary id="cover" content-type="image/bmp">{picture}</binary>
</FictionBook>"##
    )
}

fn mixed_comic() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in [
        ("nested/10.png", STANDARD.decode(PNG).unwrap()),
        ("readme.txt", b"not a page".to_vec()),
        ("__MACOSX/copy.png", STANDARD.decode(PNG).unwrap()),
        (".hidden/page.png", STANDARD.decode(PNG).unwrap()),
        ("nested/2.JPG", STANDARD.decode(JPEG).unwrap()),
        ("nested/1.BMP", bmp()),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.add_directory("nested/folder.png/", options).unwrap();
    zip.add_symlink("nested/3.png", "10.png", options).unwrap();
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
async fn epub_preserves_embedded_assets_and_all_internal_link_targets() {
    let dir = tempfile::tempdir().unwrap();
    let picture = STANDARD.decode(PNG).unwrap();
    std::fs::write(dir.path().join("book.EPUB"), epub(&picture, "pixel.png")).unwrap();
    let app = app(dir.path());
    for uri in ["/book.EPUB", "/book.EPUB?fragment=1"] {
        let response = request(&app, uri, "text/html").await;
        if uri.contains("fragment") {
            strict_fragment(&response);
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
        }
        let html = text(response).await;
        unique_resolved_fragments(&html);
        for label in ["jump-chapter", "jump-start", "back-to-first"] {
            assert!(attrs(&html, "id").contains(&linked_target(&html, label)));
        }
        let colon = linked_target(&html, "jump-colon");
        let dash = linked_target(&html, "jump-dash");
        assert_ne!(colon, dash);
        let colon_anchor = html.find(&format!("id=\"{colon}\"")).unwrap();
        let dash_anchor = html.find(&format!("id=\"{dash}\"")).unwrap();
        assert!(colon_anchor < html.find("ColonTarget").unwrap());
        assert!(html.find("ColonTarget").unwrap() < dash_anchor);
        assert!(dash_anchor < html.find("DashTarget").unwrap());
        let data = attrs(&html, "src")
            .into_iter()
            .find(|src| src.starts_with("data:image/png;base64,"))
            .unwrap();
        assert_eq!(
            STANDARD.decode(data.split_once(',').unwrap().1).unwrap(),
            picture
        );
        assert!(html.contains("Third") && html.contains("Child"));
        assert!(attrs(&html, "start").contains(&"3"));
        assert!(attrs(&html, "rowspan").contains(&"2"));
        assert_eq!(html.matches("Merged").count(), 1);
        assert!(html.contains("not code"));
        assert!(!html.contains("<script>not code"));
        assert!(!attrs(&html, "src").contains(&"/attack.js"));
        assert!(
            !attrs(&html, "href")
                .iter()
                .any(|href| href.to_ascii_lowercase().contains("script:"))
        );
        assert!(!html.contains("data-native-token"));
    }
}

#[tokio::test]
async fn epub_and_fb2_share_browser_readable_bmp_assets() {
    let dir = tempfile::tempdir().unwrap();
    let picture = bmp();
    std::fs::write(dir.path().join("book.epub"), epub(&picture, "pixel.bmp")).unwrap();
    std::fs::write(dir.path().join("book.fb2"), fb2("UTF-8")).unwrap();
    let app = app(dir.path());
    for uri in ["/book.epub?fragment=1", "/book.fb2?fragment=1"] {
        let response = request(&app, uri, "*/*").await;
        strict_fragment(&response);
        let html = text(response).await;
        let embedded = attrs(&html, "src")
            .into_iter()
            .filter(|src| src.starts_with("data:image/bmp;base64,"))
            .collect::<Vec<_>>();
        assert!(!embedded.is_empty(), "BMP must remain a readable image");
        for image in embedded {
            assert_eq!(
                STANDARD.decode(image.split_once(',').unwrap().1).unwrap(),
                picture
            );
        }
    }
}

#[tokio::test]
async fn fb2_decodes_supported_encodings_and_retains_mixed_text_and_notes() {
    let dir = tempfile::tempdir().unwrap();
    let utf8 = fb2("UTF-8").into_bytes();
    let legacy = fb2("windows-1251");
    let (legacy, _, errors) = encoding_rs::WINDOWS_1251.encode(&legacy);
    assert!(!errors);
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend(fb2("UTF-16").encode_utf16().flat_map(u16::to_le_bytes));
    for (name, data) in [
        ("utf8.fb2", utf8),
        ("legacy.FB2", legacy.into_owned()),
        ("utf16.fb2", utf16),
    ] {
        std::fs::write(dir.path().join(name), data).unwrap();
        let app = app(dir.path());
        for suffix in ["?view=1", "?fragment=1"] {
            let response = request(&app, &format!("/{name}{suffix}"), "*/*").await;
            assert_eq!(response.status(), StatusCode::OK);
            let html = text(response).await;
            for content in [
                "Привет",
                "Book annotation",
                "Before",
                "bold",
                "between",
                "italic",
                "after",
                "Footnote detail",
                "Verse one",
                "Verse two",
                "Alpha",
                "Beta",
                "Missing illustration",
            ] {
                assert!(html.contains(content), "lost FB2 content: {content}");
            }
            unique_resolved_fragments(&html);
            assert_ne!(
                linked_target(&html, "[1]"),
                linked_target(&html, "return-note")
            );
            assert!(html.find("Before").unwrap() < html.find("Footnote detail").unwrap());
            assert!(
                !attrs(&html, "href")
                    .iter()
                    .any(|href| href.to_ascii_lowercase().contains("script:"))
            );
            assert!(!html.contains("<FictionBook"));
        }
    }
}

#[tokio::test]
async fn books_keep_negotiation_download_raw_and_range_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let picture = STANDARD.decode(PNG).unwrap();
    let fixtures = [
        ("book.epub", epub(&picture, "pixel.png")),
        ("book.fb2", fb2("UTF-8").into_bytes()),
        ("book.cbz", mixed_comic()),
    ];
    for (name, original) in fixtures {
        std::fs::write(dir.path().join(name), &original).unwrap();
        let app = app(dir.path());
        for (suffix, accept, download) in [
            ("", "*/*", false),
            ("?raw=1&fragment=1&view=1", "text/html", false),
            ("?download=1&raw=1&fragment=1&view=1", "text/html", true),
        ] {
            let response = request(&app, &format!("/{name}{suffix}"), accept).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CONTENT_SECURITY_POLICY],
                "sandbox allow-scripts"
            );
            assert_eq!(
                response.headers().contains_key(header::CONTENT_DISPOSITION),
                download
            );
            assert_eq!(bytes(response).await, original);
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/{name}?raw=1"))
                    .header(header::RANGE, "bytes=0-7")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(bytes(response).await, original[..8]);
    }
}

#[tokio::test]
async fn cbz_naturally_orders_mixed_images_and_excludes_nonpages() {
    let dir = tempfile::tempdir().unwrap();
    // Windows 文件名禁止 `?`；Unix 仍覆盖它不能误变成资源 URL 查询分隔符。
    let (name, encoded) = if cfg!(windows) {
        ("漫画 #%.CBZ", "%E6%BC%AB%E7%94%BB%20%23%25.CBZ")
    } else {
        ("漫画 ?#%.CBZ", "%E6%BC%AB%E7%94%BB%20%3F%23%25.CBZ")
    };
    std::fs::write(dir.path().join(name), mixed_comic()).unwrap();
    let app = app(dir.path());
    for mode in ["?view=1", "?fragment=1"] {
        let response = request(&app, &format!("/{encoded}{mode}"), "*/*").await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = text(response).await;
        unique_resolved_fragments(&html);
        let images = attrs(&html, "src")
            .into_iter()
            .filter(|src| src.starts_with("/__/book-page/"))
            .collect::<Vec<_>>();
        assert_eq!(
            images,
            [
                format!("/__/book-page/{encoded}?page=5"),
                format!("/__/book-page/{encoded}?page=4"),
                format!("/__/book-page/{encoded}?page=0")
            ]
        );
        for (uri, media_type, original) in [
            (images[0], "image/bmp", bmp()),
            (images[1], "image/jpeg", STANDARD.decode(JPEG).unwrap()),
            (images[2], "image/png", STANDARD.decode(PNG).unwrap()),
        ] {
            let image = request(&app, uri, "*/*").await;
            assert_eq!(image.status(), StatusCode::OK);
            isolated_image(&image);
            assert_eq!(image.headers()[header::CONTENT_TYPE], media_type);
            assert_eq!(bytes(image).await, original);
        }
        assert!(!html.contains("nested/"));
        assert!(!html.contains("__MACOSX"));
        assert!(attrs(&html, "rel").contains(&"next"));
        assert!(attrs(&html, "rel").contains(&"prev"));
    }
    for index in [1, 2, 3, 6, 7, 8] {
        let response = request(
            &app,
            &format!("/__/book-page/{encoded}?page={index}"),
            "*/*",
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        isolated_image(&response);
    }
}

#[tokio::test]
async fn cbz_preserves_apng_and_jpeg_aliases_and_waits_for_concurrent_images() {
    let dir = tempfile::tempdir().unwrap();
    let png = STANDARD.decode(PNG).unwrap();
    let jpeg = STANDARD.decode(JPEG).unwrap();
    std::fs::write(
        dir.path().join("aliases.cbz"),
        zip_bytes(&[
            ("1.apng", &png),
            ("2.jpe", &jpeg),
            ("3.jfif", &jpeg),
            ("4.jpeg", &jpeg),
            ("5.png", &png),
            ("6.jpg", &jpeg),
        ]),
    )
    .unwrap();
    let app = app(dir.path());
    let html = text(request(&app, "/aliases.cbz?view=1", "*/*").await).await;
    let images = attrs(&html, "src")
        .into_iter()
        .filter(|src| src.starts_with("/__/book-page/"))
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 6, "no alias page may be silently omitted");
    let (a, b, c, d, e, f) = tokio::join!(
        request(&app, images[0], "*/*"),
        request(&app, images[1], "*/*"),
        request(&app, images[2], "*/*"),
        request(&app, images[3], "*/*"),
        request(&app, images[4], "*/*"),
        request(&app, images[5], "*/*")
    );
    for (response, original) in [a, b, c, d, e, f]
        .into_iter()
        .zip([&png, &jpeg, &jpeg, &jpeg, &png, &jpeg])
    {
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "normal image concurrency must not fail busy"
        );
        assert_eq!(bytes(response).await, *original);
    }
}

#[tokio::test]
async fn cbz_svg_is_an_isolated_image_and_invalid_image_bytes_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><script>window.bookAttack=1</script><rect width="8" height="8" fill="red"/></svg>"#;
    std::fs::write(
        dir.path().join("vector.cbz"),
        zip_bytes(&[
            ("1.svg", svg),
            (
                "2.png",
                b"<html><script>window.bookAttack=1</script></html>",
            ),
        ]),
    )
    .unwrap();
    let app = app(dir.path());
    let response = request(
        &app,
        "/__/book-page/vector.cbz?page=0&fragment=1&download=1",
        "text/html",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    isolated_image(&response);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(bytes(response).await, svg);
    let response = request(&app, "/__/book-page/vector.cbz?page=1", "*/*").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    isolated_image(&response);
    assert!(!text(response).await.contains("<script>"));
}

#[tokio::test]
async fn cbz_page_rejects_path_traversal_and_invalid_indices() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("book.cbz"), mixed_comic()).unwrap();
    std::fs::write(dir.path().join("not.cbz.txt"), "not a comic").unwrap();
    let app = app(dir.path());
    for (uri, status) in [
        (
            "/__/book-page/%2e%2e/outside.cbz?page=0",
            StatusCode::FORBIDDEN,
        ),
        ("/__/book-page/%FF.cbz?page=0", StatusCode::BAD_REQUEST),
        ("/__/book-page/book.cbz", StatusCode::NOT_FOUND),
        ("/__/book-page/book.cbz?page=-1", StatusCode::NOT_FOUND),
        (
            "/__/book-page/book.cbz?page=999999999999999999999999999",
            StatusCode::NOT_FOUND,
        ),
        ("/__/book-page/book.cbz?page=abc", StatusCode::NOT_FOUND),
        ("/__/book-page/missing.cbz?page=0", StatusCode::NOT_FOUND),
        ("/__/book-page/not.cbz.txt?page=0", StatusCode::NOT_FOUND),
    ] {
        let response = request(&app, uri, "*/*").await;
        assert_eq!(response.status(), status, "{uri}");
        isolated_image(&response);
    }
    #[cfg(windows)]
    for uri in [
        "/__/book-page/C%3A/book.cbz?page=0",
        "/__/book-page/..%5Cbook.cbz?page=0",
    ] {
        assert_eq!(
            request(&app, uri, "*/*").await.status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn malformed_books_fall_back_only_outside_fragment_mode() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["bad.epub", "bad.fb2", "bad.cbz"] {
        let original = b"<script>window.bookAttack=1</script> not a book";
        std::fs::write(dir.path().join(name), original).unwrap();
        let app = app(dir.path());
        let full = request(&app, &format!("/{name}?view=1"), "text/html").await;
        assert_eq!(full.status(), StatusCode::OK);
        assert_eq!(bytes(full).await, original);
        let fragment = request(&app, &format!("/{name}?fragment=1"), "*/*").await;
        strict_fragment(&fragment);
        let html = text(fragment).await;
        assert!(html.contains("data-cb-content-state=\"placeholder\""));
        assert!(!html.contains("window.bookAttack"));
    }
}

#[tokio::test]
async fn fb2_rejects_entities_and_preallocation_amplification_before_rendering() {
    let dir = tempfile::tempdir().unwrap();
    let entity = format!(
        "<!DOCTYPE FictionBook [<!ENTITY large \"{}\">]><FictionBook><body><section><p>{}</p></section></body></FictionBook>",
        "x".repeat(8192),
        "&large;".repeat(600)
    );
    let cdata = format!(
        "<FictionBook><body><section><p><![CDATA[{}]]></p></section></body></FictionBook>",
        "<".repeat(200_001)
    );
    let equals = format!(
        "<FictionBook><body><section><p>{}</p></section></body></FictionBook>",
        "=".repeat(200_001)
    );
    let secret = tempfile::tempdir().unwrap();
    let secret_path = secret.path().join("canary");
    std::fs::write(&secret_path, "external-entity-canary").unwrap();
    let external = format!(
        "<!DOCTYPE FictionBook [<!ENTITY leak SYSTEM \"file://{}\">]><FictionBook><body><section><p>&leak;</p></section></body></FictionBook>",
        secret_path.to_str().unwrap()
    );
    let unknown_encoding = fb2("unknown-encoding");
    for (index, input) in [entity, cdata, equals, external, unknown_encoding]
        .into_iter()
        .enumerate()
    {
        let name = format!("invalid-{index}.fb2");
        std::fs::write(dir.path().join(&name), &input).unwrap();
        let app = app(dir.path());
        let fragment = request(&app, &format!("/{name}?fragment=1"), "*/*").await;
        strict_fragment(&fragment);
        let html = text(fragment).await;
        assert!(
            html.contains("data-cb-content-state=\"placeholder\""),
            "unsafe XML accepted: {index}"
        );
        assert!(!html.contains("external-entity-canary"));
        let full = request(&app, &format!("/{name}?view=1"), "*/*").await;
        assert_eq!(bytes(full).await, input.as_bytes());
    }
}

#[tokio::test]
async fn oversized_book_inputs_keep_original_and_safe_fragment() {
    let dir = tempfile::tempdir().unwrap();
    for (name, length) in [
        ("big.epub", 32_u64 * 1024 * 1024 + 1),
        ("big.fb2", 32 * 1024 * 1024 + 1),
        ("big.cbz", 512 * 1024 * 1024 + 1),
    ] {
        std::fs::File::create(dir.path().join(name))
            .unwrap()
            .set_len(length)
            .unwrap();
        let app = app(dir.path());
        let full = request(&app, &format!("/{name}?view=1"), "*/*").await;
        assert_eq!(full.status(), StatusCode::OK);
        assert_eq!(full.headers()[header::CONTENT_LENGTH], length.to_string());
        assert_eq!(
            full.headers()[header::CONTENT_SECURITY_POLICY],
            "sandbox allow-scripts"
        );
        drop(full);
        let fragment = request(&app, &format!("/{name}?fragment=1"), "*/*").await;
        strict_fragment(&fragment);
        assert!(
            text(fragment)
                .await
                .contains("data-cb-content-state=\"placeholder\"")
        );
    }
}

#[tokio::test]
async fn cbz_rejects_ambiguous_eocd_and_bad_crc() {
    let dir = tempfile::tempdir().unwrap();
    let png = STANDARD.decode(PNG).unwrap();
    let mut ambiguous = zip_bytes(&[("1.png", &png)]);
    let footer = ambiguous.len() - 22;
    ambiguous[footer + 20..footer + 22].copy_from_slice(&23_u16.to_le_bytes());
    let mut hidden_footer = [0_u8; 22];
    hidden_footer[..4].copy_from_slice(b"PK\x05\x06");
    hidden_footer[8..10].copy_from_slice(&4097_u16.to_le_bytes());
    hidden_footer[10..12].copy_from_slice(&4097_u16.to_le_bytes());
    ambiguous.extend(hidden_footer);
    ambiguous.push(b'x'); // zip 的宽松 footer 搜索会接受未到 EOF 的候选。
    let mut corrupt = zip_bytes(&[("1.png", &png)]);
    let data_start = corrupt
        .windows(png.len())
        .position(|bytes| bytes == png)
        .unwrap();
    corrupt[data_start + png.len() - 1] ^= 1;
    for (name, data) in [("ambiguous.cbz", ambiguous), ("crc.cbz", corrupt)] {
        std::fs::write(dir.path().join(name), data).unwrap();
        let response = request(
            &app(dir.path()),
            &format!("/__/book-page/{name}?page=0"),
            "*/*",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        isolated_image(&response);
    }
}

#[tokio::test]
async fn cbz_bounds_actual_decompressed_output_not_only_declared_size() {
    let dir = tempfile::tempdir().unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(1));
    zip.start_file("1.png", options).unwrap();
    let png = STANDARD.decode(PNG).unwrap();
    zip.write_all(&png).unwrap();
    std::io::copy(
        &mut std::io::repeat(0).take(32 * 1024 * 1024 + 1 - png.len() as u64),
        &mut zip,
    )
    .unwrap();
    let mut archive = zip.finish().unwrap().into_inner();
    archive[22..26].copy_from_slice(&1_u32.to_le_bytes());
    let central = archive
        .windows(4)
        .rposition(|window| window == b"PK\x01\x02")
        .unwrap();
    archive[central + 24..central + 28].copy_from_slice(&1_u32.to_le_bytes());
    std::fs::write(dir.path().join("expansion.cbz"), archive).unwrap();
    let response = request(
        &app(dir.path()),
        "/__/book-page/expansion.cbz?page=0",
        "*/*",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    isolated_image(&response);
}

#[tokio::test]
async fn fb2_keeps_multiple_notes_bodies_at_their_source_positions() {
    let dir = tempfile::tempdir().unwrap();
    let source = r##"<FictionBook xmlns:l="http://www.w3.org/1999/xlink">
<body><section><p>Opening<a l:href="#n1">first-note</a><a l:href="#n2">second-note</a></p></section></body>
<body name="notes"><section id="n1"><title><p>1</p></title><p>First note body</p></section></body>
<body><section><p>Middle body</p></section></body>
<body name="notes"><section id="n2"><title><p>2</p></title><p>Second note body</p></section></body>
<body><section><p>Closing body</p></section></body>
</FictionBook>"##;
    std::fs::write(dir.path().join("ordered.fb2"), source).unwrap();
    let html = text(request(&app(dir.path()), "/ordered.fb2?fragment=1", "*/*").await).await;
    unique_resolved_fragments(&html);
    let positions = [
        "Opening",
        "First note body",
        "Middle body",
        "Second note body",
        "Closing body",
    ]
    .map(|text| html.find(text).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_ne!(
        linked_target(&html, "first-note"),
        linked_target(&html, "second-note")
    );
}

#[tokio::test]
async fn cbz_serves_unicode_path_extra_field_images() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let picture = STANDARD.decode(PNG).unwrap();
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "legacy.png",
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(&picture).unwrap();
        let mut archive = zip.finish().unwrap().into_inner();
        let footer = archive.len() - 22;
        let central =
            u32::from_le_bytes(archive[footer + 16..footer + 20].try_into().unwrap()) as usize;
        let directory_size =
            u32::from_le_bytes(archive[footer + 12..footer + 16].try_into().unwrap());
        let old_extra = u16::from_le_bytes(archive[central + 30..central + 32].try_into().unwrap());
        // Info-ZIP Unicode Path: version 1, CRC32("legacy.png"), UTF-8 filename.
        let unicode_name = "漫画.png".as_bytes();
        let mut extra = 0x7075_u16.to_le_bytes().to_vec();
        extra.extend(((5 + unicode_name.len()) as u16).to_le_bytes());
        extra.push(1);
        extra.extend(0x8f5c5b55_u32.to_le_bytes());
        extra.extend(unicode_name);
        let insertion = central + 46 + "legacy.png".len();
        archive.splice(insertion..insertion, extra.iter().copied());
        archive[central + 30..central + 32]
            .copy_from_slice(&(old_extra + extra.len() as u16).to_le_bytes());
        let footer = archive.len() - 22;
        archive[footer + 12..footer + 16]
            .copy_from_slice(&(directory_size + extra.len() as u32).to_le_bytes());
        std::fs::write(dir.path().join("unicode.cbz"), archive).unwrap();

        let html = text(request(&app, "/unicode.cbz?fragment=1", "*/*").await).await;
        let page = attrs(&html, "src")
            .into_iter()
            .find(|src| src.starts_with("/__/book-page/"))
            .unwrap();
        let response = request(&app, page, "*/*").await;
        assert_eq!(response.status(), StatusCode::OK, "{method:?}");
        isolated_image(&response);
        assert_eq!(bytes(response).await, picture);
    }
}

#[tokio::test]
async fn fb2_decodes_fragment_references_once_for_notes_and_images() {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        r##"<FictionBook xmlns:l="http://www.w3.org/1999/xlink">
<body><section id="n1"><p>Decoded note target</p></section>
<section id="n%31"><p>Literal percent target</p></section>
<p><a l:href="#n%31">decoded-note</a><a href="#n%2531">literal-percent-note</a></p>
<image l:href="#cov%65r"/></body>
<binary id="cover" content-type="image/png">{PNG}</binary></FictionBook>"##
    );
    std::fs::write(dir.path().join("encoded.fb2"), source).unwrap();
    let app = app(dir.path());
    for uri in ["/encoded.fb2", "/encoded.fb2?fragment=1"] {
        let html = text(request(&app, uri, "text/html").await).await;
        unique_resolved_fragments(&html);
        assert_ne!(
            linked_target(&html, "decoded-note"),
            linked_target(&html, "literal-percent-note")
        );
        let image = attrs(&html, "src")
            .into_iter()
            .find_map(|src| src.strip_prefix("data:image/png;base64,"))
            .unwrap();
        assert_eq!(
            STANDARD.decode(image).unwrap(),
            STANDARD.decode(PNG).unwrap()
        );
    }
}
