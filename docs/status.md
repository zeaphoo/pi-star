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
| Agent 主链路 | ✅ | 后端按会话选择并持久化为 `pi-star:agent-backend` custom entry：默认 `rpc`（`pi --mode rpc`，标准 PI），可选 `quick`（进程内 `AgentHarness`/QuickPI，pi v3 存储，工具经 AllowedRoots 沙箱，含 MCP 与 subagent）；`/api/agent/backends` 与聊天输入区的 Agent 下拉共用同一份状态，运行中禁止切换。QuickPI 的系统提示按 pi 的 sections（tools/rules/docs/context/skills/cwd）渲染，工具 loadout 读取 settings `defaultTools` 与 `pi-web:tool-selection`，`set_tools` 同时持久化选择 |
| PiRpcBackend | ✅ | prompt/steer/follow-up/abort/queue、模型/thinking、压缩/重试、统计、fork/clone、session rekey、单写者路由、子代理 session 锁、扩展 UI、事件流；`pi-star-agent-extension` 补偿 get/set tools、tree、branch、reload 和基础 subagent 工具；真实 pi + 假模型端到端测试覆盖工具、abort、扩展 UI、fork/rekey。作为默认 `rpc` 后端，仅拥有被选中该后端的会话 |
| 会话空闲回收 / 租约 | ✅ | `PI_WEB_IDLE_TIMEOUT_MS`，后台 30s 回收；lease 续租 |
| Subagents | ✅ | 内置 `general-purpose`/`explore`/`plan`、global/workspace/project 三种文件 scope、pi-web 完整 profile 字段（display_name、load_skills/extensions、codemode、mcp、max_turns、prompt_mode、color、isolation、persist_session、扩展工具选择等）、未知 frontmatter 保留、builtin 开关与 `agents/settings.json`、子会话/等待/并发/状态显示 |
| 扩展 UI | ✅ | pending 请求、SSE 重放 id、response/input、阻塞超时；无 Node 扩展进程时不会被触发 |
| Transcript notices | ✅ | cache miss / cache warm / summary cost / thinking dropped，项目设置覆盖全局 |
| Provider | ✅ | OpenAI-compatible / Anthropic Messages；模型缓存、provider usage |
| OAuth / device-code | ◐ | Anthropic 授权码 + PKCE、GitHub Copilot device-code、API Key 保存/删除/登出；其他 provider 的 OAuth 尚未内置 |
| 模型与 models.json | ✅ | 目录、默认/启用、CRUD、discover/test、图片大小上限校验、models.dev 自动补全、models-store 远程刷新 |
| MCP | ✅ | 文件读写、状态/遮罩、Test、OAuth、mcp-auth.json；Agent 回合自动连接 stdio/HTTP 服务器并注册 `mcp__*` 工具 |
| Skills | ✅ | 与 pi 的 resource loader 对齐：settings `skills` 路径、包内 `skills/`、`~/.pi/agent/skills`、`~/.agents/skills`、项目 `.pi/skills` 与祖先 `.agents/skills`；返回 `sourceInfo`/diagnostics/`projectResourcesLoaded`；列表/内容/开关、skills.sh 搜索、安装更新、lock 文件、已启用 skill 注入系统提示 |
| Plugins | ✅ | 与 pi 包管理器路径一致（`~/.pi/agent/npm|git`、项目 `.pi/npm|git`）；发现包内 extensions/skills/prompts/themes、`pi` manifest、包过滤/禁用、standalone extensions、counts/totals/diagnostics、本地/Git/npm 安装更新、单包与批量启停、更新检查 |
| 项目信任 | ✅ | 继承、决策、项目 MCP 清单 |
| 静态服务/SPA | ✅ | 缓存头、SPA fallback、API JSON 404 |
| 认证/安全 | ✅ | Host/Origin、body limit、cookie/Basic Auth、失败指数退避 |
| Web Push | ✅ | VAPID 自动生成、订阅持久化、aes128gcm 加密发送、404/410 清理 |
| Debug / web-auth | ✅ | 只读快照；连接页面表按 agent/terminal/file-watch 统计 SSE、地址与浏览器标签 |
| app-update | ✅ | 可配置发布源（`PI_STAR_UPDATE_URL`），12h 缓存，支持 `PI_STAR_SKIP_VERSION_CHECK=1` |
| 内嵌前端资源 | ✅ | release 用 `rust-embed` 编入 `ui/dist`；debug 读磁盘；`--ui-dir` 显式覆盖；健康响应暴露 `assets` |
| 桌面运行契约 | ✅ | stdout `assets`/`listening`/`bind-failed` 事件、stderr 日志、bind 退出码 2、`--exit-on-stdin-eof` |
| Tauri 桌面壳 | ✅ | 单实例、定位/启动/健康确认、退避重启、splash/错误页、托盘、自启动、关窗到托盘、进程树清理 |
| 桌面 WebView | ✅ | 原生装饰、稳定端口、主窗口零 IPC、只读 `__PI_STAR__` 标记、桌面模式跳过 PWA/SW |
| 桌面打包 | ✅ | `scripts/package-desktop.sh` 注入 `bundle.resources`；Linux deb 已验证携带 `PiStarApp` + `pi-star`；Windows/macOS 配置就绪 |
| 测试 | ✅ | 全部 Rust workspace 单测/集成测试（含按会话后端切换、CLI 事件/stdin/端口）+ 1652 个客户端单测 + 完整浏览器 e2e（含 PTY 终端）+ `app/` 单测 |

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

## 桌面形态

`app/` 是独立的 Tauri 2 workspace：启动并监督随包分发的 `pi-star`，默认使用固定端口
`127.0.0.1:30151`。固定端口用于保持 webview 的 origin 稳定（localStorage、cookie、缓存），
端口冲突时失败并提示，不静默换端口。

- 主窗口使用原生装饰，没有任何 Tauri IPC 权限；仅内置 splash 可调用 `desktop_action`。
- release `pi-star` 内嵌 UI，安装包不再需要 `ui/dist`；debug 仍读磁盘。
- 桌面偏好：`~/.local/share/com.pistar.desktop/desktop.toml`；日志：`pi-star.log`、`desktop.log`。
- frameless 标题栏/`windowControl`、原生通知、自动更新、签名与公证仍是后续边界。

## 端口

pi-star 默认监听 `127.0.0.1:30151`（Vite 开发服务器 30152），不与 pi-web 的 30141 冲突。
桌面壳默认也使用 30151，可通过 `desktop.toml` 的 `port` 覆盖。
