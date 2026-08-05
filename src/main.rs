//! chapbook — a simple static file server.

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

    let listener = match tokio::net::TcpListener::bind((opts.host.as_str(), opts.port)).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(error = %e, host = %opts.host, port = opts.port, "failed to bind");
            std::process::exit(1);
        }
    };

    let app = routes::app(opts.root);
    if let Err(e) = axum::serve(listener, app)
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
