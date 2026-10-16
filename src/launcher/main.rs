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
            format!("无法创建状态窗口：{error}\n\n尚未启动服务器，请重新打开程序。")
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
    // cannot leave a status window advertising a server that has disappeared.
    runtime.spawn(async move {
        let result = match server.await {
            Ok(result) => result,
            Err(error) => Err(io::Error::other(format!("服务任务异常退出：{error}"))),
        };
        let _ = result_tx.send(result);
        notifier.server_stopped();
    });

    let mut browser = BrowserActivation::new();
    browser.start(&url, &window);
    let result = (|| {
        loop {
            match window.next_event().map_err(|error| {
                format!("状态窗口出现错误：{error}\n\n服务将停止，请重新打开程序。")
            })? {
                gui::Event::OpenBrowser => browser.start(&url, &window),
                gui::Event::BrowserFinished => browser.finish(&window),
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

    stop_service(runtime, shutdown_tx, &result_rx, SHUTDOWN_TIMEOUT);
    result
}

#[cfg(windows)]
struct BrowserActivation {
    result_tx: mpsc::Sender<io::Result<()>>,
    result_rx: mpsc::Receiver<io::Result<()>>,
    busy: bool,
}

#[cfg(windows)]
impl BrowserActivation {
    fn new() -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            result_tx,
            result_rx,
            busy: false,
        }
    }

    fn start(&mut self, url: &str, window: &gui::StatusWindow) {
        self.start_with(url, window, |url| chapbook::desktop::open(OsStr::new(url)));
    }

    fn start_with(
        &mut self,
        url: &str,
        window: &gui::StatusWindow,
        activate: impl FnOnce(&str) -> io::Result<()> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        window.browser_pending();
        let url = url.to_owned();
        let result_tx = self.result_tx.clone();
        let notifier = window.notifier();
        // OS activation may hang indefinitely. This detached OS thread neither
        // occupies a Tokio worker nor needs joining when the status window exits.
        if let Err(error) = std::thread::Builder::new()
            .name("chapbook-browser".to_owned())
            .spawn(move || {
                let result = activate(&url);
                let _ = result_tx.send(result);
                notifier.browser_finished();
            })
        {
            self.busy = false;
            window.browser_finished(Err(error));
        }
    }

    fn finish(&mut self, window: &gui::StatusWindow) {
        if let Ok(result) = self.result_rx.try_recv() {
            self.busy = false;
            window.browser_finished(result);
        }
    }
}

#[cfg(windows)]
fn stop_service(
    runtime: tokio::runtime::Runtime,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    result_rx: &mpsc::Receiver<io::Result<()>>,
    timeout: Duration,
) {
    let _ = shutdown_tx.send(());
    // This deadline belongs to the GUI's OS thread, not to the runtime whose
    // workers may all be blocked. Keep the status window alive until it expires.
    let _ = result_rx.recv_timeout(timeout);
    runtime.shutdown_timeout(Duration::ZERO);
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn shutdown_deadline_survives_a_blocked_runtime_worker() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        runtime.spawn(async move {
            entered_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let (result_tx, result_rx) = mpsc::channel::<io::Result<()>>();
        runtime.spawn(async move {
            let _ = shutdown_rx.await;
            let _ = result_tx.send(Ok(()));
        });

        let (stopped_tx, stopped_rx) = mpsc::channel();
        let consumer = std::thread::spawn(move || {
            stop_service(runtime, shutdown_tx, &result_rx, Duration::from_millis(30));
            let _ = stopped_tx.send(());
        });
        let stopped = stopped_rx.recv_timeout(Duration::from_secs(1));
        // Release the deliberately blocked worker even if the assertion fails.
        let _ = release_tx.send(());
        stopped.expect("shutdown must not depend on Tokio worker progress");
        consumer.join().unwrap();
    }
}
