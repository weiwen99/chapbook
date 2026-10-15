#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod gui;

#[cfg(windows)]
use std::{ffi::OsStr, io, net::SocketAddr, sync::mpsc, time::Duration};

#[cfg(windows)]
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(windows)]
fn main() {
    if let Err(message) = run() {
        gui::show_error(&message);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The chapbook GUI launcher is Windows-only. Use the chapbook CLI on this platform.");
    std::process::exit(1);
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    let Some(selected) = gui::pick_folder() else {
        return Ok(());
    };
    let root = selected.canonicalize().map_err(|error| {
        format!(
            "无法访问所选文件夹：\n{}\n\n{error}\n\n请确认文件夹仍存在且具有读取权限，然后重新打开程序。",
            chapbook::meta::display_path_text(dunce::simplified(&selected))
        )
    })?;
    if !root.is_dir() {
        return Err(format!(
            "所选位置已不是文件夹：\n{}\n\n请重新打开程序并选择现有文件夹。",
            chapbook::meta::display_path_text(dunce::simplified(&root))
        ));
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("无法启动服务运行环境：{error}\n\n请关闭部分应用后重试。"))?;

    let setup = (|| {
        let app = chapbook::routes::app(root.clone()).map_err(|error| {
            format!("无法初始化服务：{error}\n\n尚未启动服务器，请重新打开程序。")
        })?;
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)))
            .map_err(|error| {
                format!("无法创建本机监听端口：{error}\n\n请检查安全软件是否阻止本程序，然后重试。程序不占用固定端口。")
            })?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("无法获取本机服务地址：{error}\n\n请重新打开程序。"))?;
        let url = format!("http://{address}/");
        let window = gui::StatusWindow::new(&root, &url).map_err(|error| {
            format!("无法创建服务控制窗口：{error}\n\n尚未启动服务器，请重新打开程序。")
        })?;
        Ok::<_, String>((app, listener, url, window))
    })();
    let (app, listener, url, window) = match setup {
        Ok(setup) => setup,
        Err(error) => {
            runtime.shutdown_timeout(Duration::ZERO);
            return Err(error);
        }
    };

    let notifier = window.notifier();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (result_tx, result_rx) = mpsc::channel::<io::Result<()>>();
    let server = runtime.spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
    });
    // Supervise the serving task as well as its I/O result, so a task panic
    // cannot leave a control window advertising a server that has disappeared.
    let completion = runtime.spawn(async move {
        let result = match server.await {
            Ok(result) => result,
            Err(error) => Err(io::Error::other(format!("服务任务异常退出：{error}"))),
        };
        let _ = result_tx.send(result);
        notifier.notify();
    });

    open_browser(&url);
    let result = (|| {
        loop {
            match window.next_event().map_err(|error| {
                format!("服务控制窗口出现错误：{error}\n\n服务将停止，请重新打开程序。")
            })? {
                gui::Event::OpenBrowser => open_browser(&url),
                gui::Event::Stop => return Ok(()),
                gui::Event::ServerStopped => {
                    let detail = match result_rx.try_recv() {
                        Ok(Ok(())) => "服务意外停止。".to_owned(),
                        Ok(Err(error)) => format!("服务运行失败：{error}"),
                        Err(error) => format!("服务已停止，但未收到退出结果：{error}"),
                    };
                    return Err(format!(
                        "{detail}\n\n此地址已停止提供文件夹浏览：\n{url}\n\n请重新打开程序。"
                    ));
                }
            }
        }
    })();

    let _ = shutdown_tx.send(());
    runtime.block_on(async {
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, completion).await;
    });
    // Cancel remaining async work without waiting indefinitely for blocking
    // filesystem/conversion work. Keep the HWND alive through runtime shutdown.
    runtime.shutdown_timeout(Duration::ZERO);
    result
}

#[cfg(windows)]
fn open_browser(url: &str) {
    if let Err(error) = chapbook::desktop::open(OsStr::new(url)) {
        gui::show_error(&format!(
            "无法打开默认浏览器：{error}\n\n服务仍在运行，访问地址：\n{url}\n\n请把该地址复制到浏览器，或在 Windows 设置中配置默认浏览器后，点击控制窗口的“重新打开浏览器”。"
        ));
    }
}
