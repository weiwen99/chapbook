# chapbook v0.7.0 — JSON 格式化与可折叠预览

JSON 文件新增格式化阅读视图，可折叠对象与数组，并随时切回未经格式化的高亮原文。所有操作只改变展示，不修改磁盘文件。

## JSON 阅读体验

- 默认显示格式化视图，区分字段名、字符串、数字、布尔值等，亮暗主题跟随系统。
- 对象与数组可逐项折叠／展开，也可使用“全部展开／全部折叠”；支持键盘 Enter／Space 操作。空对象、空数组和标量直接显示。
- “格式化／原文”就地切换，保留当前折叠状态，不重新请求文件、不改变 URL 或浏览历史。
- 原文保留空白、CRLF、制表符与末尾换行，不附加行号；普通代码文件仍保留既有行号。
- 格式化保持字段顺序、重复键、大整数、指数及字符串转义的原始写法，不通过重新序列化改写内容。
- 独立阅读页、列表／网格预览面板和树内容区共用实现，窄屏下工具栏自动换行。

## 支持边界与兼容

- `.json` 扩展名大小写不敏感。沿用代码预览的 1 MiB／严格 UTF-8 限制：文件超限或非 UTF-8 时，完整页回退原文件，fragment 显示安全占位。
- 格式化最多 64 层、20,000 个值，根值也计入层数与数量。非法 JSON 或结构超限时提示并显示高亮原文，不展示不完整的结构树。
- 无 JavaScript 时仍有服务端高亮原文，不显示不可用的交互控件。
- CLI、`?raw=1`、`?download=1` 和 Range 保持既有语义；原文件字节不变。
- 无新增依赖，继续保持单二进制分发、零运行时外部工具依赖。其它格式预览不变。

完整行为说明见 [README JSON 预览](https://github.com/weiwen99/chapbook/blob/v0.7.0/README.md#json-预览)。

## 发布资产与升级

| 平台 | 安装包 |
| --- | --- |
| Linux x86_64（musl 静态链接） | `chapbook-x86_64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `chapbook-aarch64-apple-darwin.tar.gz` |
| Windows x86_64（GNU） | `chapbook-x86_64-pc-windows-gnu.zip` |

每个平台提供 SHA256 校验文件。Windows zip 同时包含 `chapbook.exe` 与 `chapbook-launcher.exe`。

升级只需替换二进制并重启原有进程，无配置或数据迁移。运行 `chapbook --version`，应返回 `chapbook 0.7.0`。
