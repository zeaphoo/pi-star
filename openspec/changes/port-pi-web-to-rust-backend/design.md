# Design

## Context

pi-web 是 Next.js App Router 应用：页面本身几乎都是客户端组件，但会话、Agent、文件、Git、模型、
MCP 等逻辑分布在 68 个 Route Handler 和 450 个 `lib/` 模块中，并大量调用 pi 的 TypeScript SDK。
pi-star 要在保留 UI 与数据格式的前提下，把这些服务端行为迁移到 Rust。

约束：

- 会话文件、settings、models、MCP、skills、subagents 的磁盘格式必须与 pi 兼容。
- 浏览器端不能感知后端语言变化；API 路径、JSON 字段、SSE 事件名保持 pi-web 的约定。
- 默认只监听回环地址；服务端拥有执行命令的能力，安全边界必须显式。
- 运行时不要求 Node.js；UI 构建仍可使用 Node 工具链。

## Goals / Non-Goals

**Goals:**

- 复制 pi-web 的全部用户可见 UI，并保持交互、样式、i18n、PWA 行为一致。
- 用 Rust + Axum 实现 pi-web 的全部 API 面，先保证会话浏览、文件/Git/终端和 Agent 主链路，
  再补齐 MCP、skills、plugins、subagents、OAuth、Web Push 等设置面。
- 与 pi 共用 `~/.pi/agent` 和项目 `.pi/` 数据，可交替使用 pi CLI 与 pi-star。
- 单个 Rust 二进制可服务 UI、API、SSE 和 PTY。

**Non-Goals:**

- 不追求与 pi TypeScript SDK 的内部模块一一对应；只保证外部行为与数据格式。
- 不在本变更中实现桌面壳、安装器、签名和自动更新分发。
- 不重写 pi-web 的 UI 组件，不改变其视觉设计。
- 不引入数据库；会话和设置继续以文件为唯一事实来源。

## Decisions

### 1. UI 采用 Vite + React，而不是继续使用 Next.js

从 pi-web 复制 UI 时，只保留可从 `AppShell` 和登录页到达的客户端模块，形成 `ui/src/`。
用 Vite 构建静态资源，Axum 同源提供。替换点只有：

- `next/navigation` → 基于 `history`/`popstate` 的轻量路由 shim；
- `next/image` → 普通 `<img>` 包装；
- `app/layout.tsx` → `index.html` + 字体/CSS 入口；
- `app/api/**` 与仅服务端使用的 `lib/**` 不进入前端，由 Rust 实现。

备选方案：

- **Next.js 静态导出**：改动更小，但仍保留 Next 构建/SSR 约定、`next/font` 网络依赖和
  Route Handler 心智，且产物仍依赖 Node 工具链；不符合“后端完全 Rust”的目标。
- **重写 UI**：工作量最大且会偏离 pi-web，被否决。

复制时保留 MIT 许可与来源说明；前端只为类型导入保留最小的 pi 类型声明，运行时不引入 pi SDK。

### 2. Rust workspace 按“核心域 / HTTP 服务”拆分

```text
crates/
  pi-star-core/    会话存储、配置、模型目录、provider、Agent 运行时、工具、MCP
  pi-star/         Axum 路由、SSE、静态资源、认证、安全中间件、CLI
ui/                pi-web UI 的 Vite 适配版
```

`pi-star-core` 不依赖 Axum，便于单测和未来嵌入 Tauri。`pi-star` 只做 HTTP 映射和进程编排。
若后续 provider 数量增长，再拆 `pi-star-providers`，避免一开始过度分层。

### 3. 文件即数据库，内存只做索引与运行时状态

- 会话：读取 `~/.pi/agent/sessions/<encoded-cwd>/*.jsonl`，每行一个 JSON 条目；写入追加并 fsync，
  重命名写 `session_info`，删除按父/子关系递归处理。
- 侧边栏状态：`pi-web-session-state.json`，进程内串行队列 + 文件锁 + 临时文件 rename。
- settings/models/MCP：加锁读-改-写，保留未知字段；损坏文件先备份再重建。
- 会话列表：基于 `(size, mtime, inode)` 的增量索引，进程重启后可重建。

备选方案：SQLite 索引更快，但会把“文件是事实来源”变成双写一致性问题，且 pi CLI 不会更新数据库；
本变更不采用。

### 4. Agent Runtime 使用“每会话任务 + 广播事件”

每个运行中的 session 由 `SessionRuntime` 持有：

- `SessionHandle`：session id、cwd、模型、thinking level、工具集合、状态；
- 一个 tokio 任务执行“模型 → 工具 → 模型”循环；
- 一个 `broadcast::Sender<AgentEvent>` 供任意数量 SSE 订阅者消费；
- 一个命令队列（prompt/steer/followUp/abort/compact/...），命令通过 `mpsc` 送入任务；
- 租约计数与最后活动时间，空闲超时后取消任务、落盘并移出注册表。

SSE 序列化直接产生 pi-web `agent-event-wire.ts` 约定的精简事件；`turn_*`、`entry_appended`
和 system 消息在服务端过滤。心跳为每 30 秒一条 `data: {"type":"heartbeat"}`。

备选方案：每个 SSE 连接各跑一个循环会重复执行 Agent；共享 broadcast 更接近 pi-web 的
`session.onEvent()` 模型。

### 5. Provider 适配器与模型目录

`Provider` trait 暴露 `stream_chat(request) -> Stream<Item = ProviderDelta>`，首批实现：

- OpenAI Chat Completions / Responses 兼容接口；
- Anthropic Messages API；
- 通过 `models.json` 注册的通用 OpenAI-compatible provider。

模型目录：优先读取 `~/.pi/agent/models.json` 与缓存；可联网时从 models.dev/pi.dev 刷新，
离线时使用内置目录。凭据存放在 pi 兼容的凭据文件，权限 0600，日志中统一脱敏。

OAuth 使用 provider 的授权码/设备码流程，服务端只暴露 flow id 和状态，令牌永不进入浏览器。

### 6. 工具执行与文件安全

工具层与文件 API 共用 `Workspace`：

- 允许根 = 会话 cwd、projectRoot、显式允许根、`allow-link` 确认目标；
- 所有路径先 `canonicalize`（不存在时解析最近存在的祖先），再做前缀检查；
- `read/write/edit` 限制大小，`bash` 使用项目环境、进程组和超时；
- 终端使用 `portable-pty`，退出时杀整个进程组。

工具输出以 `toolResult` 消息追加，事件中只发送 UI 需要的字段，避免把大体积 base64 重复传输。

### 7. HTTP 契约保持 pi-web 形状

- 成功响应维持现有 JSON 字段；错误维持 `{ error: string }`，需要时附加 `code`、`reason`、
  `accepted`、`missingCwd`。
- SSE 使用 `Content-Type: text/event-stream`、`Cache-Control: no-cache, no-transform`、
  `X-Accel-Buffering: no`，每行 `data: <json>`。
- 上传使用 multipart，默认限制 100 MB；其他 JSON 请求使用较小的 body limit。
- 认证：`PI_WEB_PASSWORD` 存在时启用 cookie 会话；API 客户端可用 Basic Auth（用户名 `pi`）。
  同时校验 Host/Origin，防止 DNS rebinding 与 CSRF。

### 8. 启动与生命周期

CLI 使用 `clap` 解析 pi-web 兼容参数，`tracing` 输出日志。启动顺序：读取配置 → 初始化
agent 目录 → 构建路由 → 绑定端口 → 可选打开浏览器。SIGINT/SIGTERM 触发
`CancellationToken`，依次停止接受连接、取消 Agent、关闭 PTY/监听、等待扩展关闭期限。

桌面壳通过 `window.__PI_STAR__` 注入桥；Web 端只在检测到桥时渲染窗口控制，不影响浏览器模式。

## Risks / Trade-offs

- **[Agent 行为与 pi SDK 不完全一致]** → 先以事件协议和会话格式为兼容边界；用 pi-web 的测试
  夹具和真实会话文件做回归，逐项补齐 provider/工具语义。
- **[扩展生态依赖 Node 运行时]** → 首版明确只支持内置能力与 MCP/skills 等声明式资源；
  Node 扩展的兼容层单独立项，不在本变更中假装支持。
- **[文件监听在 Linux 上有 inotify 上限]** → 每个 cwd 合并 watcher，超出时降级为轮询并记录诊断。
- **[PTY 与子进程孤儿化]** → Unix 使用 `setsid` + 进程组 kill，Windows 使用 Job Object；
  服务端退出和会话关闭都走同一清理路径。
- **[OAuth/MCP 密钥泄露]** → 统一 secret 类型、日志脱敏、子进程环境清洗、测试覆盖。
- **[前端复制后与上游漂移]** → 在 `ui/UPSTREAM.md` 记录来源 commit，定期用脚本比较客户端模块清单。
- **[大仓库文件索引开销]** → 使用 `ignore` crate 并行遍历、结果缓存和数量上限，避免阻塞请求。

## Migration Plan

1. 在 `ui/` 建立可运行的 Vite 壳，复制客户端模块并跑通空 API 的页面渲染。
2. 在 `crates/` 建立 Axum 服务与会话/文件只读 API，替换前端数据源。
3. 实现 Agent 运行时、provider 与工具，跑通“新建会话 → 流式回复 → 工具调用 → 落盘”。
4. 补齐 Git/worktree/终端、设置/资源、MCP/OAuth、Push 与安全中间件。
5. 保留 pi-web 作为只读参考；pi-star 发布前用真实 `~/.pi/agent` 数据做兼容性验收。
6. 回滚策略：数据格式不变，停止 pi-star 后可直接用 pi CLI / pi-web 打开同一批会话；
   UI 构建产物可回退到上一个 pi-star 版本。

## Open Questions

- Node 扩展的兼容范围是否需要在首个公开版本内解决；当前按“声明式资源 + MCP”先行。
