# Proposal

## Why

pi-star 需要成为一个可独立开源的 pi Web 工作台：UI 与 pi-web 完全一致，但服务端不依赖
Node.js / TypeScript。当前 pi-web 的全部能力都绑定在 Next.js 路由与 pi 的 TypeScript SDK 上，
既无法嵌入 Rust 桌面壳，也无法复用 Rust 生态的部署、打包与安全模型。

本变更把 pi-web 的前端原样搬到 pi-star，并用 Rust + Axum 重新实现其全部后端行为，
数据格式（`~/.pi/agent` 下的会话 JSONL、settings、models、MCP、skills 等）保持与 pi 兼容。

## What Changes

- 新增 `ui/`：从 pi-web 复制全部用户可见 UI（App Shell、会话、聊天、文件、Git、终端、设置面板、
  PWA、i18n、主题、快捷键等），只做 Next.js → Vite/React 的宿主适配，不改交互与视觉。
- 新增 Rust workspace 与 Axum 服务：同源提供静态 UI、REST API、SSE 事件流、文件监听和 PTY。
- 重新实现 pi-web 的 68 个 API 路由，覆盖会话、Agent、模型、认证、文件、Git、worktree、
  终端、MCP、skills、plugins、subagents、项目信任、Web Push、应用更新与 Web 登录。
- 新增 Rust Agent Runtime：会话生命周期、prompt 队列、工具循环、流式事件、压缩、分支、
  子代理、扩展 UI 请求，并输出与 pi-web 客户端兼容的 SSE 事件。
- 保持 pi 数据目录与文件格式兼容；pi-star 与 pi CLI / pi-web 可以读取同一批会话。
- 删除对 `next`、pi TypeScript SDK 运行时、`node-pty`、`web-push` 等服务端 JS 依赖的依赖。
- **BREAKING**：服务端启动方式由 `pi-web` CLI 改为 Rust 二进制 `pi-star`；API 路径与响应
  形状保持兼容，不再提供 Next.js 运行时。

## Capabilities

### New Capabilities

- `web-ui`: 从 pi-web 复制的完整浏览器 UI、宿主适配、PWA 与同源 API 约定。
- `session-workspace`: 会话扫描、读取、树/分支、上下文分页、搜索、统计、导出、重命名、删除、fork、
  运行时会话列表与侧边栏 UI 状态。
- `agent-runtime`: Agent 会话注册表、prompt/命令协议、工具循环、SSE 事件、压缩、子代理、
  扩展 UI 与空闲/关闭生命周期。
- `workspace-tools`: 项目文件浏览、上传、预览、监听、@ 文件索引、Git status/diff/worktree、PTY 终端。
- `model-providers`: 模型目录、默认模型、enabledModels、models.json、provider 凭据/OAuth、
  模型测试/发现、provider usage。
- `resource-settings`: MCP、skills、plugins、subagent 设置、工具设置、项目信任、全局 settings 读写。
- `server-operations`: 同源静态服务、监听地址与 Host/Origin 校验、密码登录、请求体上限、Web Push、
  应用更新、优雅关闭与进程生命周期。

### Modified Capabilities

<!-- 无：项目当前没有已归档的 capability。 -->

## Impact

- 新增 `ui/`（React + Vite）与 `crates/`（Rust workspace）。
- 新增 `pi-star` 二进制；默认监听 `127.0.0.1:30151`，可用 `--port` / `--hostname` 覆盖。
- 复用 `~/.pi/agent` 与项目 `.pi/` 下的数据；写入操作需保持原子写、锁与权限位。
- 前端不再调用 Next.js Route Handler；所有 API 改为 Axum 路由，路径保持 `/api/...`。
- 移除 Node.js 服务端依赖；构建 UI 仍可使用 Node 工具链，但运行时不要求 Node。
