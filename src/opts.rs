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
    /// Bind address (default: loopback; 本机安全场景, 局域网分享用 -h 显式覆盖)
    #[arg(short = 'h', long, default_value = "127.0.0.1")]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn root_arg() -> String {
        std::env::temp_dir()
            .to_str()
            .expect("temp dir is utf-8")
            .to_string()
    }

    /// Task 6: 默认 bind 地址必须是字面 127.0.0.1 (本机安全场景).
    #[test]
    fn default_host_is_exactly_loopback() {
        let opts = Opts::try_parse_from(["chapbook", &root_arg()]).expect("parse opts");
        assert_eq!(opts.host, "127.0.0.1");
    }

    /// Task 6: `-h` / `--host` 覆盖能力必须保留 (历史约定: -h 是 host 而非 help).
    #[test]
    fn short_and_long_host_flags_still_override_default() {
        for (flag, host) in [("-h", "0.0.0.0"), ("--host", "::1")] {
            let opts =
                Opts::try_parse_from(["chapbook", flag, host, &root_arg()]).expect("parse opts");
            assert_eq!(opts.host, host, "flag {flag}");
        }
    }
}
