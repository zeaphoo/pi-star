# Proposal

## Why

pi-star 的浏览器端同时持有四类长连接：Agent 事件、终端输出、文件 watch、登录流程。浏览器的
HTTP/1.1 连接池对每个源只有 6 条连接（同一浏览器的所有标签页共享），而连接数是
`流种类 × 组件实例 × 标签页`，会随功能增加持续增长。

当前已按 #1283 逐个打补丁：隐藏页 30 秒后释放 Agent 流、文件 watch 跟随页面可见性、终端
`pagehide` 关闭、Debug 面板画 6 槽连接表。这些措施只能缓解已知的三处，并且把"记得释放"变成
每个新功能的义务：终端流在标签页隐藏时仍占着连接，主视图切走后终端工作区仍然挂载；单页
4 分屏终端加 2 个 dock 终端就能开出 12 条流。池一满，`fetch` 在浏览器里排队永不返回，界面
表现为"完全没响应"，而普通请求（`ui/src/lib/agent-client.ts` 的 `sendAgentCommand`）没有超时，
连错误都看不到。

本变更把长连接数从 `O(组件 × 标签页)` 降为 `O(1)`：所有流式数据走一条 WebSocket 多路复用
总线，由 SharedWorker 在标签页之间共享，并让"在别处新开一条流"在架构与 CI 上都不可能发生。

## What Changes

- 新增服务端 `Hub` 与唯一入口 `GET /api/stream`（WebSocket）：频道注册、订阅引用计数、
  每频道环形缓冲与序号、背压与丢弃策略。缓冲按写入水位动态伸缩（终端 1 MiB 起步、单频道
  上限 20 MiB），并受全 Hub 内存预算约束，大流量输出不会无界增长。
- 桌面端与浏览器端走同一条 localhost WebSocket：pi-star 以独立进程运行，Tauri 壳只是启动器，
  本变更不引入 IPC 传输，也不预留 transport 抽象层。
- 新增客户端 `ui/src/lib/stream-bus.ts`：唯一的连接出口，`subscribe(channel, cursor, handler)`
  与 `unsubscribe` 按频道引用计数，组件不再直接创建 `EventSource`。
- 新增 `ui/src/lib/stream-worker.ts`（SharedWorker）：跨标签页共享那一条连接；
  `SharedWorker` 不可用时用 Web Locks 选 leader 标签页 + BroadcastChannel 转发，
  再退化为每页一条。
- 迁移四类流到频道：`agent:<sid>`、`terminal:<id>`、`file:<path>`、`auth:<provider>`；
  删除现有的逐功能释放逻辑（#1283 的隐藏页释放、lease 探活、`EventSource` 重连与看门狗）。
- 终端输入、resize 与 Agent 命令改为走同一连接的 `cmd` 帧，消除"每次按键一个 POST"；
  HTTP POST 路由保留为兼容路径。
- 新增频道级游标与幂等恢复：终端按字节 `offset`、文件按 `(size, mtime)` 版本、Agent 按
  `seq` 加会话快照重读。修复 `?after=` 参数在服务端被忽略、断线重连会重放整段 backlog 的问题。
- 新增 `ui/src/lib/api-fetch.ts`：统一超时、优先级与并发上限；池饥饿时快速失败并给出可读错误，
  不再无限转圈。
- 新增护栏：ESLint 禁止 `stream-bus.ts` / `stream-worker.ts` 之外出现 `new EventSource`、
  `new WebSocket`；服务端每 holder 连接数与每连接频道数限额；CI 与 e2e 断言长连接恒为 1。
- 旧 SSE 端点只通过响应头、日志与 `CHANGELOG.md` 说明弃用，不单独提供迁移文档。
- **BREAKING（对外 API）**：旧的 `*/events` SSE 端点标记为 deprecated，保留一个发布周期供
  外部脚本迁移，之后删除。

## Capabilities

### New Capabilities

- `stream-transport`: 浏览器与 pi-star 服务端之间的长连接传输——单连接多路复用、频道订阅、
  游标恢复、背压、跨标签页共享、连接预算、请求超时策略与诊断。

### Modified Capabilities

<!-- 无：主 specs 目录为空，`web-ui` / `agent-runtime` / `workspace-tools` 仍属于
     `port-pi-web-to-rust-backend` 变更；本变更归档时再以 REMOVED/MODIFIED 增量替换其中的
     SSE 相关要求。 -->

## Impact

- 服务端：新增 `crates/pi-star/src/stream/`（Hub、频道、协议帧、连接任务）；`app.rs` 增加
  `/api/stream` 路由；`debug_streams.rs` 升级为按频道/连接统计；`agent.rs`、`terminal.rs`、
  `files.rs`、`auth.rs` 的 SSE 处理器改为频道源（旧端点保留一版）。
- 客户端：新增 `ui/src/lib/stream-bus.ts`、`stream-worker.ts`、`api-fetch.ts`；改造
  `useAgentSession.ts`、`TerminalPanel.tsx`、`TerminalPanel` 输入链路、`FileViewer.tsx`、
  `ModelsConfig.tsx`；删除 `usePageHidden.ts` 的隐藏释放特例、`agent-event-connection.ts` 的
  重连/看门狗逻辑与 `page-debug` 的逐源跟踪。
- 构建：Vite 开发代理需要 `ws: true`；SharedWorker 以 module worker 方式打包。
- 测试：新增 Hub 与总线单测、频道恢复与背压单测、跨标签页 e2e；现有
  `hidden-page-stream.test.mjs` 与 `agent-event-connection.test.mjs` 由总线语义测试替换。
- 兼容性：数据格式、会话与终端语义不变；PTY、Agent 运行时与文件监听继续复用现有实现。
- 回滚：旧 SSE 路由与 `stream-bus` 开关各保留一个发布周期，出问题时前端可切回逐功能流。
