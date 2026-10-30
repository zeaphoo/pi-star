# pi-star 迁移状态

本表按 pi-web 的 API 组列出当前 Rust 实现程度。`✅` 表示主路径可用且有测试覆盖，`◐` 表示
API/文件层可用但运行时仍有边界，`○` 表示未实现。任务清单与验收标准在
`openspec/changes/port-pi-web-to-rust-backend/`。

| 组 | 状态 | 说明 |
| --- | --- | --- |
| 会话扫描/增量索引 | ✅ | 文件指纹缓存 + 并行解析 + 持久化 `pi-star-session-index.json`；命中后约 20ms |
| 会话读取/树/上下文分页 | ✅ | 活动分支、tail/before、summary 列表、projectRoot/branch |
| 会话写操作 | ✅ | rename、delete、delete-many、fork、stats、search、export |
| 会话自动命名 | ✅ | 调用当前/默认模型生成标题；无可用模型时回退到本地截取 |
| 侧边栏 UI 状态 | ✅ | 加锁、原子写、revision、损坏备份 |
| 文件 | ✅ | list/read/download/meta/upload/upload-check/allow-link/watch、DOCX 预览 |
| @ 文件索引 | ✅ | git ls-files + 忽略规则回退，模糊匹配 |
| Git / worktree | ✅ | status、diff、list/add/remove |
| 终端 | ✅ | portable-pty + SSE + 进程组清理 |
| Agent 主链路 | ✅ | 默认 Rust Agent；`PI_STAR_AGENT_BACKEND=pi-rpc` 时由 `pi --mode rpc` 子进程执行，JSONL 协议桥接 prompt/事件/扩展 UI |
| PiRpcBackend | ✅ | prompt/steer/follow-up/abort/queue、模型/thinking、压缩/重试、统计、fork/clone、session rekey、扩展 UI、事件流；`pi-star-agent-extension` 补偿 get/set tools、tree、branch、reload 和基础 subagent 工具 |
| 会话空闲回收 / 租约 | ✅ | `PI_WEB_IDLE_TIMEOUT_MS`，后台 30s 回收；lease 续租 |
| Subagents | ✅ | 子会话、父会话等待、并发上限、profile 扫描/保存/开关、状态显示 |
| 扩展 UI | ✅ | pending 请求、SSE 重放 id、response/input、阻塞超时；无 Node 扩展进程时不会被触发 |
| Transcript notices | ✅ | cache miss / cache warm / summary cost / thinking dropped，项目设置覆盖全局 |
| Provider | ✅ | OpenAI-compatible / Anthropic Messages；模型缓存、provider usage |
| OAuth / device-code | ◐ | Anthropic 授权码 + PKCE、GitHub Copilot device-code、API Key 保存/删除/登出；其他 provider 的 OAuth 尚未内置 |
| 模型与 models.json | ✅ | 目录、默认/启用、CRUD、discover/test、图片大小上限校验、models.dev 自动补全、models-store 远程刷新 |
| MCP | ✅ | 文件读写、状态/遮罩、Test、OAuth、mcp-auth.json；Agent 回合自动连接 stdio/HTTP 服务器并注册 `mcp__*` 工具 |
| Skills | ✅ | 列表/内容/开关、skills.sh 搜索、GitHub/本地包安装与更新、更新检查、lock 文件、已启用 skill 注入系统提示 |
| Plugins | ✅ | 列表、本地/Git/npm 安装、更新、启用/禁用/移除、npm 更新检查 |
| 项目信任 | ✅ | 继承、决策、项目 MCP 清单 |
| 静态服务/SPA | ✅ | 缓存头、SPA fallback、API JSON 404 |
| 认证/安全 | ✅ | Host/Origin、body limit、cookie/Basic Auth、失败指数退避 |
| Web Push | ✅ | VAPID 自动生成、订阅持久化、aes128gcm 加密发送、404/410 清理 |
| Debug / web-auth | ✅ | 只读快照；连接页面表按 agent/terminal/file-watch 统计 SSE、地址与浏览器标签 |
| app-update | ✅ | 可配置发布源（`PI_STAR_UPDATE_URL`），12h 缓存，支持 `PI_STAR_SKIP_VERSION_CHECK=1` |
| 测试 | ✅ | 23 个 core 单测 + 33 个服务端单测/集成测试 + 1648 个客户端单测 + 完整浏览器 e2e（含 PTY 终端） |

## 运行时边界

以下不是 API 缺口，而是 Rust Agent 运行时当前的能力边界：

- **Node/TypeScript 扩展执行**：扩展包可以安装和管理，`extension_ui_request` 的传输、
  响应和超时也已实现；但 Rust Agent 不执行 pi 的 TypeScript 扩展，扩展注册工具、命令、
  widgets 和扩展 UI 的宿主仍未接入。
- **Provider OAuth 覆盖**：OAuth 仍只内置 Anthropic 和 GitHub Copilot；OpenAI
  Codex/ChatGPT、OpenRouter、Kimi、xAI、Radius 等 provider 需要各自的 OAuth 流程，
  目前仍以 API Key 或 pi 写入的凭据为主。
- **会话导出**：优先调用已安装的 `pi --export` 生成与 pi 相同的 HTML；`pi` 不可用时
  回退到 Rust 生成的简化 HTML。

## 端口

pi-star 默认监听 `127.0.0.1:30151`（Vite 开发服务器 30152），不与 pi-web 的 30141 冲突。
