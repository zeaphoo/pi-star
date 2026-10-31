# pi-star

pi-star 是 [pi coding agent](https://github.com/earendil-works/pi) 的本地 Web 工作台：
UI 直接来自 [pi-web](https://github.com/agegr/pi-web)（MIT），服务端用 **Rust + Axum** 重新实现，
前端资源与服务端不需要 Node.js；桌面安装包自带 Node 和 pi，单独运行 `pi-star` 时默认 `rpc`
后端仍可使用系统安装的 pi（或用 QuickPI）。

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
crates/pi-star/         Axum 路由、SSE、静态/内嵌资源、认证与安全中间件、CLI
app/                    Tauri 2 桌面壳（独立 Cargo workspace）：启动并监督 pi-star
openspec/               迁移与桌面化的 proposal/spec/design/tasks
scripts/                UI 复制、桌面打包与版本一致性脚本
```

release 构建会把 `ui/dist` 用 `rust-embed` 编进 `pi-star` 二进制；debug 构建仍读磁盘，
方便前端迭代。桌面壳不包含业务代码，只负责启动、健康确认、监督、托盘和窗口。

### Agent 后端

后端按会话选择，默认是标准 PI（Node.js），由 `pi --mode rpc` 子进程执行：

| id | 名称 | 实现 |
| --- | --- | --- |
| `rpc` | 标准 PI (Node.js) | `PiRpcBackend`，`pi --mode rpc`，兼容 TypeScript 扩展 |
| `quick` | 极速 PI (QuickPI) | 进程内 `AgentHarness`（`crates/pi-star-agent/tools/harness/ai`） |

选择写在会话文件的 `pi-star:agent-backend` custom entry 中，随会话文件走，pi 会忽略它；
没有该 entry 时按标准 PI 处理。两种后端共享同一批会话文件和配置，但一个会话同一时间只能由一个
后端写入。

- `GET /api/agent/backends` 返回可选项。
- `POST /api/agent/new` 可用 `backend: "rpc" | "quick"` 指定新会话的后端。
- 会话命令 `{"type":"set_backend","backend":"rpc|quick"}` 在会话空闲时切换，运行中返回 409。
- 聊天输入区的 Agent 下拉与 `/api/agent/:id` 的 `backend` 字段使用同一份状态。

`PI_STAR_PI_CLI` 指定 pi 可执行文件，`PI_STAR_AGENT_EXTENSION` 指定
`agent-extension/index.ts`，`PI_STAR_PI_RPC_MAX_SESSIONS` 限制同时存活的 pi RPC 会话数
（默认 8，超出时先回收空闲会话）。PiRpcBackend 覆盖 prompt、steer、follow-up、abort、队列、
模型/thinking、压缩、重试、会话统计、fork/clone、命令、扩展 UI 和事件流；`get_tools`、
`set_tools`、`navigate_tree`、`fork_branch`、`reload` 由 `agent-extension/` 通过 pi 扩展 API
补偿。该扩展还注册基础的 `Agent`、`get_subagent_result`、`steer_subagent` 工具，子代理是
独立的 pi RPC 会话。

QuickPI 的系统提示按 pi 的 sections（preamble/tools/rules/docs/addendum/project_context/skills/cwd）渲染，
并读取 `SYSTEM.md`、`APPEND_SYSTEM.md`、`AGENTS.md`/`CLAUDE.md` 与 `settings.json` 的 `defaultTools`；
`pi-web:tool-selection` custom entry 会在重开会话时恢复工具选择。Settings › Skills、Sub-agents、
Plugins 使用与 pi 相同的资源发现规则：settings 路径、包内资源、`~/.agents`、项目 `.pi` 与
`npm/git` 安装目录。

数据仍以文件为唯一事实来源：

- 会话：`~/.pi/agent/sessions/<encoded-cwd>/*.jsonl`
- 设置：`~/.pi/agent/settings.json`、`models.json`、`auth.json`
- 侧边栏状态：`~/.pi/agent/pi-web-session-state.json`
- 项目资源：`<cwd>/.pi/mcp.json`、`.pi/skills`，以及 `~/.pi/agent/skills`

pi-star 与 pi CLI / pi-web 可以读取同一批会话；停止 pi-star 后可以直接用它们继续。

## 构建与运行

要求：Rust stable（1.85+）和 Node.js 22+（构建 UI、桌面打包和准备内置 pi 运行时需要）。

```bash
# 1. 构建 UI
cd ui
npm install
npm run build
cd ..

# 2. 构建并运行服务（release 会内嵌上一步的 ui/dist）
cargo build --release
./target/release/pi-star --port 30151
```

打开 <http://127.0.0.1:30151>。release 二进制可以单独复制到没有 `ui/dist` 的目录运行；
debug 构建默认从仓库 `ui/dist` 读资源，改完前端只需重新 `npm run build`，不必重编 Rust。

启动后不会自动打开浏览器；需要时用 `--open` 显式打开。标准输出只包含以 `pi-star: ` 开头的
生命周期事件（`assets`、`listening`、`bind-failed`），日志写标准错误；端口被占用时进程以
退出码 2 退出。

常用参数：

```bash
pi-star --port 8080 --hostname 0.0.0.0
pi-star --open
pi-star --ui-dir ui/dist --agent-dir ~/.pi/agent
pi-star --port 0 --exit-on-stdin-eof   # 临时端口 + 父进程消失即退出
```

`--ui-dir`（环境变量 `PI_STAR_UI_DIR`）是显式覆盖：给出时始终从磁盘读 UI，并在启动时校验
`index.html`；不给出时 release 用内置资源、debug 用 `ui/dist`。

对应环境变量：`PORT`、`PI_WEB_HOSTNAME`、`PI_STAR_OPEN_BROWSER`、`PI_STAR_UI_DIR`、
`PI_WEB_PASSWORD`、`PI_WEB_ALLOWED_HOSTS`、`PI_CODING_AGENT_DIR`。桌面安装包还会设置
`PI_STAR_PI_RUNTIME`（内置 Node + pi）、`PI_STAR_AGENT_EXTENSION` 和 `PI_PACKAGE_DIR`；
`PI_STAR_PI_CLI` 可显式改用用户自己的 pi，Windows 的 bash 路径可用 `PI_STAR_BASH` 覆盖。

### 开发

```bash
# 终端 1：Rust 服务（30151）
cargo run -p pi-star

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

`npm test` 运行从 pi-web 迁移的客户端单元测试（Node test runner + jiti）。
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

## 桌面应用

`app/` 是独立的 Tauri 2 workspace（根 workspace 已 `exclude = ["app"]`），负责：

- 单实例启动，定位并启动随包分发的 `pi-star`；
- 通过 stdout 事件 + `/api/health` 确认进程归属，监督崩溃并按 1/2/4/8/16 秒退避重启；
- splash/错误页、原生装饰主窗口、托盘、开机自启动、关窗到托盘；
- 退出时清理 pi-star 及其 Node/PTY 子进程树。

安装包同时内置 Node、pi CLI（`@earendil-works/pi-coding-agent`）和 `agent-extension/`，
安装后不需要单独安装 Node 或 pi。版本固定在 `packaging/pi-runtime.json`。

主窗口加载 `http://127.0.0.1:<port>/`，端口默认 `30151` 并持久化在桌面偏好中。端口保持稳定是
因为 webview 的 localStorage、cookie 和缓存都以 origin 为键；冲突时桌面壳会显示错误，不会静默
换端口。主窗口使用系统原生装饰，且没有 Tauri IPC 权限；只有内置 splash 可以调用重试/退出/打开
日志。

开发：

```bash
# 终端 1：构建一次 debug pi-star
cargo build -p pi-star
# 终端 2：构建 UI（debug 壳从磁盘读）
cd ui && npm run build && cd ..
# 终端 3：运行桌面壳（会定位仓库 target/debug/pi-star）
cd app && cargo tauri dev
```

UI 热更新：先 `cd ui && npm run dev`（30152），再在 debug 构建下设置
`PI_STAR_DESKTOP_URL=http://127.0.0.1:30152/` 启动桌面壳；该模式跳过 spawn 与监督。

打包（当前平台）：

```bash
./scripts/check-desktop-version.sh
./scripts/package-desktop.sh          # 当前平台
./scripts/package-linux-app.sh        # deb + AppImage
./scripts/package-windows.sh          # NSIS + MSI + portable zip（Git Bash）
./scripts/package-mac.sh              # .app + dmg
```

脚本顺序为：准备 `pi-runtime`（Node + pi + agent-extension）→ 构建 UI → 构建内嵌 UI 的
release `pi-star` → Tauri 安装包。`pi-star` 与 `pi-runtime/` 通过临时 `bundle.resources`
注入，Linux 落在 `/usr/lib/PiStar/`，macOS 落在 `Contents/Resources/`，Windows 与
`PiStarApp.exe` 同目录。产物在 `dist/<os>/`。Linux **只发布桌面应用**，不再有单独的服务包。

Windows 的 `bash` 工具需要 Git for Windows（不会回退 cmd.exe，也不用 WSL 的
`System32\bash.exe`）；探测顺序和 `PI_STAR_BASH` 覆盖见 [`packaging/README.md`](packaging/README.md)。
安装包默认未签名；macOS 配置签名 secrets 后会自动签名/公证。CI 与 Release 配置见
`.github/workflows/`。

桌面壳数据目录（Linux `~/.local/share/com.pistar.desktop/`）：

- `desktop.toml`：`autostart`、`close_to_tray`、`port`；
- `pi-star.log`：子进程 stderr 与事件日志（启动时超过 5 MB 轮转）；
- `desktop.log`：壳自身的偏好/自启动错误。

pi-star 的会话与设置仍在 `~/.pi/agent`，与浏览器形态、pi CLI 共用。

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
