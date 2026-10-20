# chapbook v0.5.0 — Windows 原生支持

Windows 现在是一等平台：命令行程序无需 WSL 或 MSVC 即可原生运行，另提供默认关闭的图形启动器。
Linux/macOS 行为不变；v0.4.3 的未知扩展名纯文本预览、HTML 预览/源码视图、安全与原始出口契约全部保留。

## Windows 原生支持

- **原生 CLI** — 使用 GNU/MinGW-w64 工具链构建；默认应用打开走 `ShellExecuteExW`，路径以 UTF-16
  传递，不经过 `cmd /c start` 或 PowerShell 命令拼接。
- **安全随机令牌** — 启动令牌改用 `getrandom` 系统安全随机源，失败仍拒绝启动，不降级为弱令牌。
- **路径策略** — 拒绝盘符、反斜杠、UNC/设备前缀（包括正斜杠与 `%2F` 编码形式）及 NTFS ADS；
  解析后的 `Component::Prefix(_)` 一律 fail closed。新增 Windows 专属路由回归。
- **发布资产** — `chapbook-x86_64-pc-windows-gnu.zip`，内含 `chapbook.exe`、
  `chapbook-launcher.exe`、LICENSE、README.md 与 sha256 校验和。
- **运行要求** — Windows 10+ x64；exe 只导入系统 DLL（KERNEL32、msvcrt、ole32、shell32、
  ws2_32 等），MinGW 运行时已静态链接，不需要 MinGW/MSYS2/GCC 或 VC++ 运行库。
  GNU 工具链与 `patch` 只是构建期依赖。

## Windows 可选图形启动器

- 默认构建只生成 CLI，不引入 `rfd`；显式启用 `windows-launcher` 才构建独立的
  `chapbook-launcher.exe`。
- 选择文件夹后启动本机服务并打开默认浏览器；状态窗口展示访问地址，失败内联提示，不阻塞停止。
- 浏览器启动在独立工作线程执行；关闭时在 GUI 线程使用独立于服务 worker 的超时等待。
  优雅关闭等待上限为 2 秒，不代表整个进程的严格实时退出保证。
- 启动器与 CLI 各自独立，可单独复制运行，无需配套另一个二进制。

## CI

- Linux、macOS、Windows 三个平台 job 并行执行；macOS 新增 PR/push CI。
- 三个平台均执行构建、测试、`cargo fmt --check` 与 `cargo clippy --all-targets -D warnings`；
  Mermaid 真实浏览器场景在 Linux 执行，避免重复。
- Windows GNU job 安装 MSYS2 GNU `patch` 并校验实际路径，避免误用 runner 自带的 Strawberry Perl
  版本；`actions/setup-node` 升至 v5。

## 兼容与升级

- **Linux/macOS** — 纯文本预览、`.env` 映射、HTML 预览/源码、Range、PDF/媒体例外、路径安全与
  native-open 语义均不变。
- **Windows 范围** — 仅支持 GNU/MinGW-w64，不支持 MSVC；图形启动器是可选特性。
- **编码边界** — 非 UTF-8 纯文本仍不自动预览（v0.4.3 的已知边界）。
- **升级** — 替换二进制并重启原有进程；无需数据迁移。
- **发布平台** — Linux x86_64（musl 静态）、macOS Apple Silicon、Windows x86_64（GNU）。

## 发布前验证

| 验证项 | 结果 |
| --- | --- |
| `cargo test`（macOS） | 228 通过 |
| `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`（macOS） | 通过 |
| `cargo build --release --locked`（macOS） | 通过 |
| 三平台 CI（Linux / macOS / Windows） | 全部通过 |
| Windows zip 导入表 | 仅系统 DLL；无 `libgcc_s`/`libwinpthread`/`libstdc++` |
| Tag 工作流 | 构建 Linux/macOS/Windows 资产并生成 sha256 |

二进制 `--version` 应返回 `chapbook 0.5.0`。
