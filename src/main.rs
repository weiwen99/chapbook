//! chapbook — a simple static file server.

use std::net::SocketAddr;

use chapbook::opts::Opts;
use chapbook::routes;
use clap::Parser;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let opts = Opts::parse();
    tracing::info!(
        root = %opts.root.display(),
        host = %opts.host,
        port = opts.port,
        "server parameters"
    );

    // 初始化 app (token 生成) 先于 bind: 初始化失败记录清晰错误并退出,
    // 绝不降级为弱 token 运行.
    let app = match routes::app(opts.root) {
        Ok(app) => app,
        Err(e) => {
            tracing::error!(error = %e, "failed to initialize server (token generation)");
            std::process::exit(1);
        }
    };

    let listener = match tokio::net::TcpListener::bind((opts.host.as_str(), opts.port)).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(error = %e, host = %opts.host, port = opts.port, "failed to bind");
            std::process::exit(1);
        }
    };

    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    {
        tracing::error!(error = %e, "server error");
        std::process::exit(1);
    }
    tracing::info!("server stopped.");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
