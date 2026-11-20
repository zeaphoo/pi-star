# Design

## Context

浏览器对同一个源（scheme + host + port）只允许 6 条并发 HTTP/1.1 连接，且这个配额由同一浏览器的
所有标签页共享。pi-star 目前把"每个实时数据源"实现成一条独立 SSE：

- `GET /api/agent/{id}/events`（`crates/pi-star/src/routes/agent.rs`）
- `GET /api/terminal/{id}/events`（`crates/pi-star/src/routes/terminal.rs`）
- `GET /api/files?...&kind=watch`（`crates/pi-star/src/routes/files.rs`）
- `GET /api/auth/login/{provider}`（登录流程）

前端对应四类 `new EventSource`。终端输入、resize 走 `POST /api/terminal/{id}`，Agent 命令走
`POST /api/agent/{id}`，这些请求与页面渲染、会话读取共用同一个连接池。

已有缓解（#1283）：隐藏页 30 秒后释放 Agent 流与文件 watch、Agent 流的看门狗与重连、
租约请求 5 秒超时用于识别"池被占满"、Debug 面板的 6 槽指示。它们都是逐功能的补丁：

- 终端流只在 `pagehide` / 离线 / 卸载时关闭，标签页切到后台仍持有连接；
- 主视图切走后终端工作区仍挂载（`display: none`），其分屏继续流式接收；
- 单个页面可合法持有 `1(agent) + 4(主区终端) + 8(dock 终端) + N(文件 watch)` 条连接；
- 普通 `fetch` 没有超时，池满时界面无任何反馈。

服务端与运行时的现状可以复用：Agent 会话、Agent 运行时、`pi_rpc` 会话、`TerminalManager`
都已有广播能力（`broadcast::Sender`），`debug_streams` 已有流注册表。

约束：

- 浏览器模式与 Tauri 桌面版共用同一套 UI 与 API，默认只监听回环地址。
- 认证是 HttpOnly cookie（`auth.rs`），并做 Host/Origin 校验；WebSocket 握手必须沿用。
- 终端输出可达每秒数百 KB，Agent 事件不可丢；两者不能互相阻塞。
- 不引入数据库、不改变会话/终端/文件的数据格式。

## Goals / Non-Goals

**Goals:**

- 每个浏览器 profile 到 pi-star 源的长连接**恒为 1 条**（共享 Worker），任何组件数量、
  分屏数量、标签页数量都不改变这个数字。
- 所有流式能力（Agent、终端、文件 watch、登录）走同一套频道协议，新增功能默认安全。
- 断线、切后台、冻结标签页都变成廉价且可恢复的常态，不再依赖"保持连接不断"。
- 池饥饿不再能让 UI 静默假死：普通请求有超时与明确错误，流永远只占 1 条。
- 把"不许在别处开流"变成 lint/CI 约束，而不是代码评审时的记忆。
- 缓冲按负载伸缩：单频道有硬上限（终端 20 MiB），全 Hub 有内存预算，大流量输出不会无界增长。

**Non-Goals:**

- 不引入桌面端 Tauri IPC 传输：pi-star 以独立进程运行，Tauri 壳只负责启动与展示，桌面版与
  浏览器版走同一条 localhost WebSocket。因此也不预留 transport 抽象层。
- 不引入 HTTP/2 或 TLS：明文 localhost 上浏览器不会协商 h2（h2c 不可用），且证书分发成本高于收益。
- 不重写 Agent 运行时、PTY、文件监听的数据面；只替换它们的传输与订阅方式。
- 不做"多用户/多租户"的连接配额；限额只用于防御 bug 与误用。
- 不改变会话 JSONL、settings、终端进程生命周期语义。

## Decisions

### 1. 单一多路复用 WebSocket，而不是继续用 SSE

一个 `GET /api/stream` 升级为 WebSocket，承载全部频道。选择 WS 而不是"多路复用 SSE"的原因：

- WS 在 101 之后脱离 HTTP 连接池，**不占那 6 个槽**；SSE 一条占一个。
- 双工：终端输入、resize、Agent 命令可以走同一连接，消除"每次按键一个 POST"的竞争。
- 浏览器对 WS 的上限是每主机约 255 条（Chromium 系）、全局 200 条（Firefox 默认），
  与"只需 1 条"相差一个数量级以上。服务端仍设每 holder 限额（见决策 9）。

代价：没有 SSE 的自动重连与 `Last-Event-ID`，需要自己实现心跳、退避重连与游标恢复。
现有代码里已经有三套重连逻辑要收敛，实际是净减少。

备选方案：

- **SSE 总线（单端点 + 频道帧）**：改动更小，但输入/命令仍走 POST，且每标签页仍占 1 条连接，
  需要把跨标签页共享也一起做才等价；双工收益丢失。作为迁移的第一步并不划算。
- **HTTP/2**：需要 TLS 与本地证书管理；只是把上限从 6 变大，不满足"O(1)"的不变式。

### 2. 频道模型与帧协议

所有数据被建模为**频道**，频道名即订阅键：

| 频道 | 源 | 恢复游标 |
| --- | --- | --- |
| `agent:<sid>` | Agent 运行时 / `pi_rpc` 会话的广播 | `seq`，超出窗口时重读会话快照 |
| `terminal:<id>` | `TerminalManager` 的广播 + backlog | 字节 `offset` |
| `file:<absPath>` | 文件版本轮询（合并后） | `(size, mtime)` 版本 |
| `auth:<provider>` | 登录流程事件 | 从头重放（流程短） |

帧为 JSON 文本，首字段 `op`。客户端 → 服务端：

```json
{"op":"hello","protocol":1,"page":"<pageId>"}
{"op":"sub","id":7,"channel":"terminal:abc","cursor":12345}
{"op":"unsub","id":7}
{"op":"cmd","channel":"terminal:abc","payload":{"type":"input","data":"ls\r"}}
{"op":"ack","channel":"terminal:abc","seq":9123,"bytes":4096}
```

服务端 → 客户端：

```json
{"op":"ready","protocol":1,"limits":{"channels":64,"frameBytes":1048576}}
{"op":"sub_ok","id":7,"channel":"terminal:abc","seq":9123}
{"op":"ev","channel":"terminal:abc","seq":9124,"data":{"type":"output","offset":32768,"data":"..."}}
{"op":"gap","channel":"terminal:abc","from":9000,"to":9123}
{"op":"sub_err","id":7,"code":"unknown_channel","message":"..."}
{"op":"cmd_result","channel":"agent:sid","id":12,"status":200,"body":{...}}
{"op":"pong"}
```

`id` 是客户端自增的订阅句柄，`seq` 是频道内单调序号（由 Hub 分配，与业务 offset 分开）。
同一频道被多个订阅者共享时，Hub 只向上游注册一次。

### 3. 服务端 Hub：频道注册表 + 每频道环形缓冲

```text
Hub
 ├─ channels: HashMap<ChannelKey, Arc<Channel>>
 └─ connections: HashMap<ConnId, ConnHandle>
Channel
 ├─ key / kind
 ├─ seq: AtomicU64
 ├─ ring: Mutex<VecDeque<(Seq, Arc<[u8]>)>>   // 有界，按字节与条数双上限
 ├─ subs: AtomicUsize                        // 订阅引用计数
 └─ upstream: Option<UpstreamHandle>          // 无人订阅时释放
ConnHandle
 ├─ outbound: mpsc::Sender<Frame>             // 有界
 ├─ subs: HashSet<ChannelKey>
 └─ page: Option<String>                      // ?holder= 的替代，用于诊断
```

- 订阅：先查频道，不存在则按 kind 创建并接上上游（Agent 广播、终端广播、watcher、登录流程）。
- 上游事件写入 ring 并广播给该频道的所有订阅连接；`seq += 1`。
- 退订/连接断开：引用计数减一；归零时释放上游（终端频道不再监听、watcher 关闭、
  登录流程取消）。
- `terminal:<id>` 的 ring 直接由 `TerminalManager` 的 backlog 语义驱动：上游事件带绝对
  `offset`，Hub 用它切片，天然支持 `cursor=offset` 续传。
- Agent 频道复用现有事件过滤（`filter_event`）与 `connected` 首帧，保持前端事件形状不变。

**缓冲容量按负载伸缩**（环境变量 `PI_STAR_STREAM_RING_*` 可覆盖）：

| 频道 | 初始容量 | 单频道上限 | 说明 |
| --- | --- | --- | --- |
| `terminal:<id>` | 1 MiB | 20 MiB | 唯一可能持续产生大流量的频道（大文件输出、`yes`、构建日志） |
| `agent:<sid>` | 512 KiB | 8 MiB | 含流式文本与工具输出；超窗由会话快照补偿 |
| `file:<path>` | 64 KiB | 1 MiB | 只存版本事件 |
| `auth:<provider>` | 64 KiB | 256 KiB | 短流程，从不淘汰 |

- 扩容：写入达到当前容量 80% 时按 2 倍扩容（至少能放下下一帧），到单频道上限为止；
  扩容计入全局预算。
- 收缩：连续 60 秒使用率低于 25% 时减半，回落到初始容量；只保留 `cursor` 之后仍可能被
  请求的部分。
- 全局预算：默认 256 MiB（`PI_STAR_STREAM_RING_BUDGET_BYTES`）。预算不足时先降到半容量，
  仍超出则按 `terminal` → `file` → `agent` 的顺序清空可重建的历史（pinned 快照保留；前两者可
  `reset` 重建，Agent 由会话快照补偿），`auth` 永不淘汰，并记录 `evicted` 诊断。
- 每帧以 `Arc<[u8]>` 共享，缓冲内存按频道计，不随订阅者数量增长。
- Debug 面板展示每频道的容量、使用量、水位与淘汰次数。

### 4. 恢复语义：断线是常态，不是异常

| 频道 | 重连后 |
| --- | --- |
| `terminal:<id>` | 客户端带最后消费的 `offset` 订阅；Hub 从 ring 中切片回放，缺口超过 ring 时下发 `reset` 并把 ring 全量作为新基线（客户端清屏后写入） |
| `file:<path>` | 客户端带最后 `(size, mtime)`；不同则立即读一次文件，相同则只等 `change` |
| `agent:<sid>` | 客户端带最后 `seq`；窗口内增量回放，窗口外由客户端重读会话快照（现有 `loadSession`）后以快照为准 |
| `auth:<provider>` | 从头重放；流程本身幂等 |

频道**新建时忽略客户端游标**，直接以快照作为基线（没有历史可重放；客户端携带的游标属于
上一次挂载）。游标只用于恢复一个仍然存活的频道——例如断线重连或另一个标签页仍在订阅。

Agent 事件**不允许**因恢复而丢失语义：窗口外的增量一律以快照重读补偿，UI 已有的
`applyLoadedAgentState` 是唯一权威。终端允许"丢弃中间字节 + reset 基线"，因为终端只关心
最终屏幕状态与最近回滚。

### 5. 背压、丢弃与队头阻塞

一条连接承载所有频道，必须有明确的优先级与有界队列：

- 每连接 `outbound` 为有界 `mpsc`（默认 1 MiB / 512 帧）。它与频道 ring 是两笔预算：前者是
  正在发送、后者是可回放历史；两者都计入诊断。
- 频道优先级：`agent` > `auth` > `file` > `terminal`。
- 队列满时：
  - `terminal` / `file`：合并与丢弃最旧帧，并向该订阅下发 `gap`（终端随后可用 `reset`
    基线）；不阻塞其它频道。
  - `agent`：**不丢弃**。若队列持续满，关闭该连接（`code=slow_consumer`），客户端按退避
    重连并从 `seq`/快照恢复；这样宁可断线也不产生静默的语义缺口。
- 客户端保留 `ack` 操作位（当前只作诊断，帧由服务端有界队列兜底）；浏览器冻结或落后来不及
  消费时，Hub 主动丢中间帧并下发 `gap`，而不是无限堆内存。
- 写入任务按 8–16 ms 合并小帧，减少帧头与调度开销（终端高频输出场景）。

### 6. 跨标签页共享：SharedWorker 是主路径

```text
tab A ──MessagePort──┐
tab B ──MessagePort──┼── SharedWorker ── 1 条 WebSocket ── /api/stream
tab C ──MessagePort──┘   （引用计数 + 最新快照缓存）
```

- Worker 持有唯一 socket，维护跨标签页的频道引用计数；tab 关闭时端口断开，自动退订它持有的
  频道。
- 新 tab 订阅时，Worker 用已有频道的 ring 尾部直接喂快照，不必让服务端重建上游。
- Worker 内统一做心跳、退避重连、游标恢复；页面侧 `stream-bus.ts` 只负责订阅与分发。
- 降级链：
  1. `SharedWorker` 可用（Chromium/WebView2、Safari 16.4+）→ 1 条/浏览器。
  2. 不可用（旧 WKWebView 等）→ `navigator.locks.request("pi-star-stream-bus")` 选 leader
     标签页持有 socket，其余标签页通过 `BroadcastChannel` 收发；leader 关闭后锁释放，
     另一个标签页接管并重连。
  3. 再不可用 → 每页 1 条（仍远小于逐功能流的数量；隐藏页退订自己的频道以降低负担）。
- 页面隐藏/冻结不再需要"释放连接"：标签页只退订自己的频道（终端、文件），Agent 频道保持；
  连接本身由 Worker 按"是否还有标签页"决定，不再由可见性决定。

### 7. 客户端唯一出口与连接预算

- `ui/src/lib/stream-bus.ts` 是唯一允许创建长连接的模块；`stream-worker.ts` 内部创建 socket。
- 通道对象只暴露 `subscribe(channel, cursor, handler) => unsubscribe`；引用计数保证同一频道
  在页面内只占一个订阅。
- 页面级预算：主连接 1 条 + 可选 1 条"应急连接"（仅当 Worker 报告 socket 不可用时，
  由页面兜底），永不出现第 3 条。长连接不再从 6 池里取，普通 API 因此总有 ≥5 条可用。
- 页面可见性只影响订阅：隐藏超过 30 秒的标签页退订 `terminal` / `file` 频道（保留 `agent`），
  回到前台时按游标重新订阅。PTY 与文件监听在服务端保持，重连无损。

### 8. 普通请求策略：超时、优先级、可读失败

- 新增 `ui/src/lib/api-fetch.ts`：默认 30 秒超时（可覆盖），合并 `AbortSignal`，按
  `interactive` / `background` 两类调度：后台类并发上限 2，交互类不受后台任务阻塞。
- 超时与网络失败统一抛 `ApiError`，带 `code`（如 `timeout`、`offline`、`pool_starved`）与
  可读消息；UI 显示重试而不是无限加载。
- 池饥饿检测：超时且浏览器报告连接数已满时给出"连接被实时流占满"的明确提示（正常情况下
  不再触发，因为流只有 1 条）。
- 迁移范围：`agent-client.ts`、`terminal-client.ts`、`file`/`session`/`models` 等 fetch 调用点
  统一走 `apiFetch`；保留现有 `SESSION_LEASE_TIMEOUT_MS` 语义直到租约逻辑随 Agent 频道迁移删除。

### 9. 服务端限额与安全

- 每 holder（页面/浏览器标签）最多 8 条连接，每连接最多 64 个频道，单帧上限 1 MiB，
  终端输入帧上限 64 KiB；超限返回 `sub_err` / 关闭码，并记录日志。
- 握手复用现有 `security_middleware`：校验 Host/Origin，要求 cookie 认证；
  `SameSite=Strict` 的 cookie 在同源 WS 握手中自动携带，无需新 token 机制。
- 未通过校验直接 403，不升级；升级请求计入现有请求体上限与日志。
- 心跳：服务端每 25 秒 `ping`，连续 2 次未 `pong` 关闭连接；客户端另有 60 秒无帧判定为断线。
- 关闭：优雅关闭时向所有 socket 发送 `{"op":"bye","reason":"shutdown"}` 并以 1001 关闭；
  客户端按退避（1s、2s、4s，上限 30s，带抖动）重连。

### 10. 诊断：从"数连接"变成"看频道"

- `debug_streams` 升级为 Hub 的视图：连接数、每连接的频道、每频道的订阅者、`seq`、
  ring 占用、丢弃字节、最后 `gap`。
- Debug 面板展示 `socket: 1/1`、`channels: N`、`agent 1 / terminal 2 / file 1`、
  `dropped`、`gaps`；5/6 槽指示与 `HTTP1_CONNECTIONS` 常量删除（不再适用）。
- 保留 `/api/agent/debug` 的统计字段并对齐；新增 `hub` 段，避免破坏现有客户端。

### 11. 旧端点与灰度

- 旧 SSE 路由保留一个发布周期，响应头加 `Deprecation`，日志按 holder 记录使用量；不为旧端点
  单独提供迁移文档，弃用说明只写在响应头、日志与 `CHANGELOG.md` 中。
- 客户端由 `?stream=legacy` 或 localStorage 的 `pi-star:stream=legacy` 选择逐功能流，便于
  线上回滚，无需重新构建。
- 一个发布周期后删除旧路由、`agent-event-connection` 的重连逻辑、`usePageHidden` 的释放
  特例、`page-debug` 的逐源跟踪，以及 `after` 这个从未被服务端解析的参数。

## Risks / Trade-offs

- **[单连接放大故障]** 一条 socket 断开会影响所有频道 → 心跳、退避重连、游标恢复是必须项，
  且连接级故障必须与业务状态解耦；测试要覆盖"断线后 agent 快照一致、终端无重复输出"。
- **[多路复用引入队头阻塞]** 高频终端输出可能挤占 Agent 事件 → 有界队列 + 频道优先级 +
  丢弃策略；用压测（`yes`、`cat` 大文件）验证 Agent 事件延迟上限。
- **[环形缓冲内存]** 大流量终端可能把单频道推到 20 MiB，多频道叠加会超出预算 → 全 Hub 预算
  与按类型淘汰（terminal/file 可重建）、扩容/收缩水位，Diagnostics 暴露容量与淘汰次数；
  压测要覆盖"多个终端同时刷屏到上限"与"空闲后内存回落"。
- **[SharedWorker 兼容性]** 旧 WKWebView 可能不支持 → Web Locks + BroadcastChannel 降级，
  再退化为每页 1 条；e2e 需要覆盖三条路径。
- **[共享 Worker 中的状态]** ring 缓存位于 Worker，页面刷新后要从 Worker 取快照 →
  定义"attach 时回放最新 ring 尾部"的契约，并限制缓冲上限。
- **[迁移期两套实现并存]** 旧 SSE 与新总线同时存在会加倍测试面 → 灰度开关只影响客户端，
  服务端共享同一批频道源，旧 SSE 处理器改为 Hub 的单一订阅者，避免逻辑分叉。
- **[WebSocket 的浏览器上限]** 每主机约 255、Firefox 全局 200；本设计只需 1 条，但降级路径
  可能到"每页 1 条"，需要在 50+ 标签页场景下确认不退化。
- **[调试与可观测性变化]** 原有 6 槽指示与逐源列表被频道视图取代 → 保留字段兼容，并在
  发布说明中解释新的诊断方式。

## Migration Plan

1. **Hub 与协议**：实现 `crates/pi-star/src/stream/`、`/api/stream`、帧协议与诊断注册表；
   先只接 `terminal:*`，用单测与 `wscat` 验证订阅、offset 续传、背压与关闭。
2. **终端频道**：`TerminalPanel` 改用 `subscribe("terminal:<id>")`，输入/resize 走 `cmd` 帧；
   修复 `after` 语义（服务端按 offset 切片），删除 `esRef`/`verifyTerminal` 重连分支。
3. **Agent 频道**：`useAgentSession` 用频道替换 `AgentEventConnection`；删除隐藏页释放、
   租约探活、看门狗与重连；断线恢复走 `seq` + `loadSession` 快照。
4. **文件与登录频道**：`FileViewer` 与 `ModelsConfig` 迁移，删除逐源 `trackEventSource`。
5. **跨标签页**：接入 SharedWorker，补 Web Locks + BroadcastChannel 降级；删除
   `usePageHidden` 的释放逻辑，改为退订频道。
6. **请求策略与护栏**：`apiFetch` 全量替换、ESLint 规则、服务端限额、e2e 连接数断言。
7. **清理**：删除旧 SSE 路由与开关；更新 `docs/`、Debug 面板与发布说明。
8. **回滚**：灰度开关切回旧路径；旧路由保留期间数据格式与 PTY/Agent 语义完全不变。

## Open Questions

- 全局预算默认 256 MiB 是否要按机器可用内存自适应（`sysinfo`）？当前固定值 + 环境变量覆盖。
- 是否在设置面板暴露终端单频道上限（20 MiB）？当前只保留环境变量，避免用户误调后丢回放窗口。
- 标签页被浏览器冻结（而非关闭）时不会发送 `bye`，Worker 会暂时保留它的频道引用；是否需要
  在 Worker 侧加一个心跳清理？当前依赖 `pagehide` 与浏览器回收 Worker。
