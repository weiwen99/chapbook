# 未发布

## Windows 图形启动器

- 新增独立单文件 `chapbook-launcher.exe`，本地可双击文件名为 `打开文件夹浏览.exe`。
- 双击弹出系统文件夹选择框，确认后同进程启动 HTTP 服务并打开默认浏览器；取消即退出。
- 自动绑定本机随机空闲端口，无需用户输入路径或端口，多实例互不冲突。
- 控制窗口显示文件夹与地址，提供重开浏览器和停止按钮；关闭窗口也停止服务，退出最多等待 2 秒。
- 启动器无需配套 CLI、源码或资源文件；原有命令行入口与 `cargo run --` 用法保持不变。
- Windows 默认应用接口提取为共用适配器；安全路径显示支持绝对盘符根路径并增加回归测试。

## Windows 原生支持

- 启动令牌改用 `getrandom` 系统安全随机源，修复 Windows 因 `/dev/urandom` 不存在而启动失败。
- 本机打开新增 `ShellExecuteExW` 适配，保持中文、空格与特殊字符路径，不通过 shell 拼接命令。
- Windows 文件请求拒绝盘符、反斜杠、UNC/设备路径与 NTFS ADS；增加对应路由安全回归测试。
- 调整平台相关测试 fixture，README 补充 x64 GNU/MinGW-w64 构建与 `.exe` 启动说明。

上一版本发布说明见 [chapbook v0.4.1](release-notes-v0.4.1.md)。

构建与浏览器行为验收入口见 [README](../README.md#development)。
