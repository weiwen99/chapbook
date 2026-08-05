# Third-Party Notices

本文件列出 chapbook 分发物（二进制 / 源码）中包含或静态链接的第三方组件及其许可证。
chapbook 本身采用 Apache License 2.0（见 [LICENSE](./LICENSE)）。

## 内嵌前端资源（随二进制分发，必须保留 MIT 版权声明）

### Materialize v2.3.3（社区维护分支）

`assets/materialize.min.css` 与 `assets/materialize.min.js` 来自
[materializecss/materialize](https://github.com/materializecss/materialize)（v2.3.3，MIT）。

> The MIT License (MIT)
>
> Copyright (c) 2014-2026 Materialize
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## 内嵌语法定义（随二进制分发）

`assets/syntaxes/` 下的 14 个 TextMate 语法文件用于补充 syntect 默认集缺失的常见语言
（TypeScript、TOML、Kotlin、Swift、Dockerfile、GraphQL、Dart、Elixir、CMake、Protobuf、
Zig、INI、Nix、Svelte）：

- `TOML.sublime-syntax` 来自 [sublimehq/Packages](https://github.com/sublimehq/Packages)（MIT）。
- 其余 13 个来自 [sharkdp/bat](https://github.com/sharkdp/bat) 的 `assets/syntaxes/02_Extra/`
  （bat 项目为 MIT；各语法文件源自其各自上游仓库，许可证以各上游为准，多为 MIT 或
  BSD-3-Clause —— 主要上游包括 Microsoft/TypeScript-Sublime-Plugin（TypeScript，MIT）、
  dart-lang（Dart）等）。

## 静态链接的渲染相关依赖

以下 crate 以静态链接方式编译进二进制（许可证以 crates.io 发布元数据为准）：

| crate | 许可证 | 用途 |
|---|---|---|
| [orgize](https://github.com/PoiScript/orgize) | MIT | .org 文档渲染 |
| [comrak](https://github.com/kivikakk/comrak) | BSD-2-Clause | .md 文档渲染 |
| [syntect](https://github.com/trishume/syntect) | MIT | 代码语法高亮（内嵌语法集） |
| [maud](https://github.com/lambda-fairy/maud) | MIT OR Apache-2.0 | HTML 模板 |
| [axum](https://github.com/tokio-rs/axum) / [tokio](https://github.com/tokio-rs/tokio) | MIT | HTTP 服务运行时 |

其余传递依赖（clap、chrono、tower-http、percent-encoding 等）的完整清单与许可证见
[crates.io](https://crates.io) 各 crate 页面；`Cargo.lock` 记录了精确版本。

## 说明

- `assets/chapbook-theme.css` 与 `assets/chapbook-doc.css` 为 chapbook 自有作品（Apache-2.0）；
  其中的颜色取值参照 GitHub 主题（颜色本身不受版权保护）。
- 若以源码形式再分发，请保留本文件与 [LICENSE](./LICENSE)。
