# pi-star

pi-star 是 [pi coding agent](https://github.com/earendil-works/pi) 的本地 Web 工作台：
UI 直接来自 [pi-web](https://github.com/agegr/pi-web)（MIT），服务端用 **Rust + Axum** 重新实现，
运行时不依赖 Node.js。

目标很直接：保留 pi-web 的全部界面与交互，同时把会话、Agent、文件、Git、终端、模型和设置能力
放到一个可独立发布的 Rust 二进制里，并继续使用 `~/.pi/agent` 下的 pi 数据。

## 当前状态

已经跑通的主链路：

- 从 pi-web 复制 231 个客户端模块，Vite 构建后由 Axum 同源提供；页面、会话侧边栏、
  文件树、聊天、模型选择、设置面板和终端 UI 均可加载。
- 扫描并读取 pi 的 JSONL 会话，支持会话列表、活动分支上下文、向上分页、树/分支、
  重命名、删除、fork、搜索、统计、导出和侧边栏 UI 状态（pin/archive/项目顺序/项目名）。
- 文件列表/读取/下载/元数据/上传/上传检查/目录链接确认、@ 文件索引、文件 watch SSE。
- Git status/diff、worktree 列表/创建/删除、PTY 终端（输入/resize/SSE 输出）。
- Agent 运行时：新建会话、prompt、SSE 事件、abort/队列、set_model、set_thinking_level、
  set_tools、compact、navigate_tree、fork、统计、最近回复、空闲回收；OpenAI-compatible
  与 Anthropic Messages 流式 provider；内置 read/write/edit/bash/find/grep/ls 工具。
- Subagent：子会话创建、父会话等待、并发上限、profile 扫描/保存/开关和状态显示。
- 模型目录、models.json、默认/启用模型、模型缓存、provider usage、API Key 保存/删除、
  Anthropic OAuth、GitHub Copilot device-code、tools/cache-miss 设置、项目信任、
  MCP 文件读写/Test/OAuth、skills 列表/内容/开关/搜索/安装/更新、plugins 安装/更新/启停、
  DOCX 预览、transcript notices、debug 快照、密码登录、Web Push（VAPID + aes128gcm）和应用更新接口。

OpenSpec 任务已全部完成（61/61）。运行时边界（Node 扩展执行、Agent 回合中的 MCP 工具注册、
完整 skill resource loader、更多 provider 的 OAuth）记录在 [`docs/status.md`](docs/status.md)。

## 架构

```text
ui/                     pi-web UI 的 Vite 适配版（React 19 + Tailwind 4）
  src/app|components|hooks|lib
  src/shims/            next/navigation、next/image、parseStreamingJson 的本地替身
crates/pi-star-ai/      模型、provider、流式协议与 usage（源码融合自 rpi）
crates/pi-star-agent/   Agent loop、事件、队列、工具调用（源码融合自 rpi）
crates/pi-star-tools/   内置工具与 ExecutionEnv（源码融合自 rpi）
crates/pi-star-harness/ 会话状态机、压缩、system prompt、skills、harness loop
crates/pi-star-core/    会话(v3)、配置、模型、文件/Git、终端、UiState、Agent 适配层
crates/pi-star/         Axum 路由、SSE、静态资源、认证与安全中间件、CLI
openspec/               本次迁移的 proposal/spec/design/tasks
scripts/                从只读 pi-web checkout 重新复制 UI 的脚本
```

### Agent 后端

默认使用融合后的 `AgentHarness` 内核（`crates/pi-star-agent/tools/harness/ai`），
会话仍写 pi 的 v3 JSONL。旧版 Rust Agent 保留一个版本的 A/B 窗口：

```bash
PI_STAR_AGENT_BACKEND=legacy ./target/release/pi-star --port 30151
```

也可以让会话交给 `pi --mode rpc` 子进程执行（TS 扩展兼容路径）：

```bash
PI_STAR_AGENT_BACKEND=pi-rpc ./target/release/pi-star --port 30151
```

可用 `PI_STAR_PI_CLI` 指定 pi 可执行文件，`PI_STAR_AGENT_EXTENSION` 指定
`agent-extension/index.ts`，`PI_STAR_PI_RPC_MAX_SESSIONS` 限制同时存活的 pi RPC 会话数
（默认 8，超出时先回收空闲会话）。两种后端共享同一批会话文件和配置，但一个会话同一时间只能由一个
后端写入。PiRpcBackend 覆盖 prompt、steer、follow-up、abort、队列、模型/thinking、压缩、重试、
会话统计、fork/clone、命令、扩展 UI 和事件流；`get_tools`、`set_tools`、`navigate_tree`、
`fork_branch`、`reload` 由 `agent-extension/` 通过 pi 扩展 API 补偿。该扩展还注册基础的
`Agent`、`get_subagent_result`、`steer_subagent` 工具，子代理是独立的 pi RPC 会话。

数据仍以文件为唯一事实来源：

- 会话：`~/.pi/agent/sessions/<encoded-cwd>/*.jsonl`
- 设置：`~/.pi/agent/settings.json`、`models.json`、`auth.json`
- 侧边栏状态：`~/.pi/agent/pi-web-session-state.json`
- 项目资源：`<cwd>/.pi/mcp.json`、`.pi/skills`，以及 `~/.pi/agent/skills`

pi-star 与 pi CLI / pi-web 可以读取同一批会话；停止 pi-star 后可以直接用它们继续。

## 构建与运行

要求：Rust stable（1.85+）和 Node.js 22+（仅构建 UI 需要）。

```bash
# 1. 构建 UI
cd ui
npm install
npm run build
cd ..

# 2. 构建并运行服务
cargo build --release
./target/release/pi-star --port 30151
```

打开 <http://127.0.0.1:30151>。

常用参数：

```bash
pi-star --port 8080 --hostname 0.0.0.0
pi-star --no-open
pi-star --ui-dir ui/dist --agent-dir ~/.pi/agent
```

对应环境变量：`PORT`、`PI_WEB_HOSTNAME`、`PI_WEB_NO_OPEN`、`PI_WEB_PASSWORD`、
`PI_WEB_ALLOWED_HOSTS`、`PI_CODING_AGENT_DIR`。

### 开发

```bash
# 终端 1：Rust 服务（30151）
cargo run -p pi-star -- --no-open

# 终端 2：Vite（30152，/api 代理到 30151）
cd ui && npm run dev
```

验证：

```bash
cargo test --offline -p pi-star-core
cargo test --offline -p pi-star
cd ui && npm run typecheck && npm run build && npm test
# 需要已运行的 pi-star（默认 http://127.0.0.1:30151）
npm run smoke
```

`npm test` 运行从 pi-web 迁移的 1648 个客户端单元测试（Node test runner + jiti）。
浏览器回归测试会自己启动 `target/{debug,release}/pi-star`，先构建 UI 和二进制：

```bash
cd ui && npm run build
cd .. && cargo build
cd ui && npm run test:e2e        # 会话、分页、侧边栏、设置、模型、文件、主题等
npm run test:terminal            # 真实 PTY 终端、重连、重启、workspace 隔离
```

Playwright 需要 Chromium。浏览器不在默认位置时设置 `PI_STAR_CHROMIUM`，例如
`PI_STAR_CHROMIUM=/usr/bin/chromium-browser`；`PI_STAR_BIN` 可指定二进制路径。
pi-web 的扩展对话框 e2e 需要执行 Node/TypeScript 扩展，默认跳过；设置
`PI_STAR_EXTENSIONS=1` 可在未来 Rust 扩展宿主接入后重新启用。`npm run smoke` 的
`PI_STAR_URL` 可覆盖已运行服务地址。

从 pi-web 迁移的测试位于 `ui/src/**/*.test.mjs` 与 `ui/e2e/`。仅存在于 pi-web 服务端
（Next API Route、Agent RPC、MCP host 等）的测试由 Rust 单元/集成测试覆盖：`cargo test
-p pi-star-core` 与 `cargo test -p pi-star`。

重新从 pi-web 复制 UI：

```bash
PI_WEB_DIR=/path/to/pi-web node scripts/sync-ui-from-pi-web.mjs
```

脚本只复制从 `app/page.tsx`、`app/login/page.tsx`、`AppShell` 和 `PwaRegistration` 可达的
客户端模块，不会把 pi-web 的 API Route 或 Node-only 模块带进来。

## 安全

- 默认只监听 `127.0.0.1`。绑定到局域网时请设置长随机 `PI_WEB_PASSWORD`。
- 文件/Git/终端 API 只允许会话 cwd、projectRoot、本进程显式允许的根，以及用户确认过的
  目录链接目标；路径会解析符号链接后再次校验。
- 带 `Origin` 的请求会校验同源；`Host` 只接受回环、私有网段或 `PI_WEB_ALLOWED_HOSTS`。
- 密码通过 HttpOnly、SameSite=Strict cookie 保存；API 客户端可用 Basic Auth（用户名 `pi`）。
- 明文 HTTP 不提供加密；远程访问请使用 HTTPS 反向代理或可信 VPN。
- MCP/stdio、终端和工具执行都会启动子进程，只应在可信项目中使用。

## 许可

pi-star 自身以 MIT 发布。`ui/` 来自 pi-web（MIT），来源与复制清单见
[`ui/UPSTREAM.md`](ui/UPSTREAM.md)；第三方依赖的许可随各自包分发。
