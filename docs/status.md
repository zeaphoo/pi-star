# pi-star 迁移状态

本表按 pi-web 的 API 组列出当前 Rust 实现程度。`✅` 表示主路径可用且有测试覆盖，`◐` 表示
API/文件层可用但运行时仍有边界，`○` 表示未实现。任务清单与验收标准在
`openspec/changes/port-pi-web-to-rust-backend/`。

| 组 | 状态 | 说明 |
| --- | --- | --- |
| 会话扫描/增量索引 | ✅ | 文件指纹缓存 + 并行解析 + 持久化 `pi-star-session-index.json`；命中后约 20ms |
| 会话读取/树/上下文分页 | ✅ | 活动分支、tail/before、summary 列表、projectRoot/branch |
| 会话写操作 | ✅ | rename、delete、delete-many、fork、stats、search、export、auto-name |
| 侧边栏 UI 状态 | ✅ | 加锁、原子写、revision、损坏备份 |
| 文件 | ✅ | list/read/download/meta/upload/upload-check/allow-link/watch、DOCX 预览 |
| @ 文件索引 | ✅ | git ls-files + 忽略规则回退，模糊匹配 |
| Git / worktree | ✅ | status、diff、list/add/remove |
| 终端 | ✅ | portable-pty + SSE + 进程组清理 |
| Agent 主链路 | ✅ | prompt、SSE、abort、队列、模型/thinking/tools、compact、fork、统计 |
| 会话空闲回收 / 租约 | ✅ | `PI_WEB_IDLE_TIMEOUT_MS`，后台 30s 回收；lease 续租 |
| Subagents | ✅ | 子会话、父会话等待、并发上限、profile 扫描/保存/开关、状态显示 |
| 扩展 UI | ✅ | pending 请求、SSE 重放 id、response/input、阻塞超时；无 Node 扩展进程时不会被触发 |
| Transcript notices | ✅ | cache miss / cache warm / summary cost / thinking dropped，项目设置覆盖全局 |
| Provider | ✅ | OpenAI-compatible / Anthropic Messages；模型缓存、provider usage |
| OAuth / device-code | ✅ | Anthropic 授权码 + PKCE；GitHub Copilot device-code；API Key 保存/删除/登出 |
| 模型与 models.json | ✅ | 目录、默认/启用、CRUD、discover/test、5s 进程内缓存 |
| MCP | ✅ | 文件读写、状态/遮罩、Test（stdio JSON-RPC/HTTP）、OAuth discovery/注册/回调、mcp-auth.json |
| Skills | ✅ | 列表/内容/开关、skills.sh 搜索、GitHub/本地包安装与更新、lock 文件 |
| Plugins | ✅ | 列表、本地/Git/npm 安装、更新、启用/禁用/移除、npm 更新检查 |
| 项目信任 | ✅ | 继承、决策、项目 MCP 清单 |
| 静态服务/SPA | ✅ | 缓存头、SPA fallback、API JSON 404 |
| 认证/安全 | ✅ | Host/Origin、body limit、cookie/Basic Auth、失败指数退避 |
| Web Push | ✅ | VAPID 自动生成、订阅持久化、aes128gcm 加密发送、404/410 清理 |
| Debug / app-update / web-auth | ✅ | 只读快照与基础状态接口 |
| 测试 | ✅ | 13 个 core 单测 + 15 个 HTTP/集成测试 + `npm run typecheck/build/smoke` |

## 运行时边界

以下不是 API 缺口，而是 Rust Agent 运行时当前的能力边界：

- **Node 扩展/插件执行**：插件包可以安装、更新和列入设置，但 Rust Agent 不会执行 pi 的
  TypeScript 扩展。`extension_ui_request` 的传输、响应和超时已实现，等待未来的 Rust
  扩展宿主接入。
- **MCP 会话接入**：Settings 的 MCP Test/OAuth/配置已实现；Agent 回合中自动连接
  `mcp.json` 服务器并注册工具仍待接入。
- **Skills 注入**：Skills 可安装、列出、编辑和搜索；把已启用 skills 的内容注入系统提示
  仍使用基础提示词，尚未完整移植 pi 的 resource loader。
- **Provider 覆盖**：OAuth 目前覆盖 Anthropic 和 GitHub Copilot；其他 provider 仍以
  API Key 为主。

## 端口

pi-star 默认监听 `127.0.0.1:30151`（Vite 开发服务器 30152），不与 pi-web 的 30141 冲突。
