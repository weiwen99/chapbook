//! CLI 参数 (clap derive).

use std::path::PathBuf;

use clap::{ArgAction, Parser};

#[derive(Parser, Debug)]
#[command(
    name = "chapbook",
    version,
    about = "serve a directory as a readable little book",
    // -h 表示 --host (历史约定), 帮助只走 --help
    disable_help_flag = true
)]
pub struct Opts {
    /// Bind address
    #[arg(short = 'h', long, default_value = "0.0.0.0")]
    pub host: String,

    /// Listen port
    #[arg(short = 'p', long, default_value_t = 8888)]
    pub port: u16,

    /// Directory to serve (must exist)
    #[arg(value_name = "root-directory", value_parser = parse_root)]
    pub root: PathBuf,

    /// Print help
    #[arg(long = "help", action = ArgAction::Help)]
    pub help: Option<bool>,
}

/// 转化为 realpath 以检验路径是否存在.
fn parse_root(s: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(s).map_err(|e| format!("Invalid path: {s}, exception: {e}"))
}
