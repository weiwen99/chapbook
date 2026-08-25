# 02: 可信 UI 反嵌入头 + KaTeX 宿主预加载

**What to build:** `.md`/`.org`/Office/代码四类渲染完整页响应升级为可信 UI：与目录/搜索页同出口携带 `frame-ancestors 'none'` + `X-Frame-Options: DENY`。目录/搜索页共享骨架补充 KaTeX 样式表（顺带修复目录页预览面板既有公式无样式缺口）。curl 即可演示。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `.md`/`.org`/Office/代码四类完整页 HTTP 响应实测带两个反嵌入头（目录/搜索页既有行为保持）
- [ ] 目录/搜索页共享骨架与文档页骨架均含 `katex.min.css` link（HTML 断言）
- [ ] fragment/JSON/静态资源响应不带这两个头、不携带 token（既有安全契约不变，测试锁定）
- [ ] Seam A：新增四类页面反嵌入头断言测试；既有全部测试通过；`cargo fmt --check` 与 clippy 零警告
