# 未发布

## Windows 平台支持

- 启动令牌使用 `getrandom` 系统安全随机源，失败仍拒绝启动；真实应用令牌的格式与授权链有行为回归覆盖。
- 默认应用打开使用 `ShellExecuteExW` 与 UTF-16 路径，不拼接 shell 命令。
- Windows 路径除盘符、反斜杠与 NTFS ADS 外，还拒绝解析后的 UNC/设备前缀，包括正斜杠和 `%2F` 形式。
- 恢复 Unix 星号文件名的 RFC 8187 下载覆盖，Windows 使用可创建的单引号 fixture。
- 增加 Windows GNU/MinGW-w64 CI，执行 Windows 专属路由与可选启动器测试。
- Windows CI 显式安装 MSYS2 GNU `patch`，避免误用 runner 的 Strawberry Perl 版本导致 QuickJS 构建崩溃；构建前记录实际使用的路径与版本，Windows 构建说明同步补齐该依赖。

## Windows 可选图形启动器

- 默认构建只生成 CLI；显式启用 `windows-launcher` 才构建独立的 `chapbook-launcher.exe`。
- 选择文件夹后同进程启动 HTTP 服务，自动分配本机端口并打开浏览器；取消选择不启动服务。
- 浏览器启动在独立工作线程执行，结果异步回传状态窗口；失败以内联提示展示，不阻塞停止操作。
- 状态窗口关闭时在 GUI 线程使用独立于服务 worker 的超时等待；优雅关闭等待上限为 2 秒。
- 控件布局使用具名 `ControlBounds`，状态窗口术语收录到 GLOSSARY。
- 文件夹选择库升级到 rfd 0.17，与直接依赖统一使用 windows-sys 0.61，移除重复的 0.59 绑定。

## 资源回归与文档

- `.gitattributes` 禁止压缩 JavaScript 的 LF/CRLF 检出转换，保留官方 vendored 字节。
- 新增真实浏览器 Mermaid 行为验收并纳入 Linux CI：检查实际懒加载 HTTP/MIME 与有标签、有向边的 SVG。
- 文档改为中性的 Windows 平台与可选启动器说明，不包含 Fork 自指或本机分发命名。

上一版本发布说明见 [chapbook v0.4.3](release-notes-v0.4.3.md)。

构建与浏览器行为验收入口见 [README](../README.md#development)。
