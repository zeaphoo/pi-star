# Tasks

## 1. 服务端 Hub 与协议

- [x] 1.1 新增 `crates/pi-star/src/stream/` 模块：`Frame`/`SubId` 协议类型、错误码与序列化测试
- [x] 1.2 实现 `Hub`：频道注册表、订阅引用计数、`seq` 分配、无人订阅时释放上游
- [x] 1.3 实现每频道环形缓冲：初始容量、按水位扩容/收缩、单频道硬上限（终端 20 MiB）与按 `cursor` 切片回放
- [x] 1.4 实现 `GET /api/stream` WebSocket 升级：`hello`/`sub`/`unsub`/`cmd`/`ack`/`ping` 处理
- [x] 1.5 实现每连接有界发送队列、频道优先级、`gap` 与 `slow_consumer` 关闭策略
- [x] 1.6 接入 `security_middleware`：Origin/Host 校验、cookie 认证、未授权 403 不升级
- [x] 1.7 实现限额：每 holder 连接数、每连接频道数、帧大小、终端输入帧大小
- [x] 1.8 实现心跳（25s ping，2 次未 pong 关闭）与优雅关闭（`bye` + 1001）
- [x] 1.9 单测：订阅/退订、引用计数、回放切窗、丢弃与优先级、限额、关闭清理
- [x] 1.10 实现全 Hub 缓冲预算（默认 256 MiB，可环境变量覆盖）与按类型淘汰顺序
      （terminal/file 先于 agent，auth 不淘汰），暴露容量/水位/淘汰诊断

## 2. 终端频道

- [x] 2.1 将 `TerminalManager` 广播与 backlog 接入 `terminal:<id>` 频道（绝对 `offset`）
- [x] 2.2 实现 `cursor=offset` 切片续传与 `reset` 基线（缺口超过 ring 时）
- [x] 2.3 实现 `cmd` 帧：`input`/`resize`，保留 POST 路由作为兼容路径
- [x] 2.4 前端 `TerminalPanel` 改用总线订阅，删除 `esRef`、`verifyTerminal` 与重连分支
- [x] 2.5 前端输入链路改用 `cmd` 帧并保留批量合并；删除 `terminal-client` 的逐请求队列
- [x] 2.6 测试：进程输出、重连无重复输出、`reset` 清屏、输入经由 socket、PTY 生命周期不变

## 3. Agent 频道

- [x] 3.1 将 Agent 运行时与 `pi_rpc` 广播接入 `agent:<sid>` 频道（沿用现有事件过滤与首帧）
- [x] 3.2 实现 `seq` 增量回放与窗口外快照重读契约
- [x] 3.3 前端 `useAgentSession` 用 `subscribe("agent:<sid>")` 替换 `AgentEventConnection` 的
      EventSource（状态机与租约保留，传输换成总线）
- [x] 3.4 删除 `AgentEventConnection` 的 stale 看门狗（由 WS 心跳接管）与文件 watch 的
      逐页面释放逻辑（总线统一处理）
- [x] 3.5 将租约（lease）语义并入频道生命周期：订阅即租约，退订/断开后由现有空闲回收接管
- [x] 3.6 测试：流式消息、工具事件、扩展 UI、隐藏页退订与回归后快照一致、慢消费者断开重连

## 4. 文件 watch 与登录频道

- [x] 4.1 将文件版本监听接入 `file:<path>` 频道（合并同一路径的订阅，零订阅时停止 watcher）
- [x] 4.2 前端 `FileViewer`（文本/图片/音频/视频/文档）改用总线订阅，删除 `trackEventSource`
- [x] 4.3 将登录流程接入 `auth:<provider>` 频道，前端 `ModelsConfig` 改用总线
- [x] 4.4 测试：文件变更推送、版本去重、页面隐藏退订后恢复、登录成功/失败/取消与断开重连

## 5. 客户端 StreamBus 与跨标签页

- [x] 5.1 实现 `ui/src/lib/stream-bus.ts`：`subscribe`/`unsubscribe`、引用计数、帧分发、ACK
- [x] 5.2 实现 `ui/src/lib/stream-worker.ts`（SharedWorker）：单 socket、跨标签页引用计数、ring 尾快照
- [x] 5.3 实现降级链：SharedWorker → Web Locks leader + BroadcastChannel → 每页 1 条
- [x] 5.4 页面隐藏超过 30 秒退订 `terminal`/`file` 频道，回前台按游标重新订阅
- [x] 5.5 移除 `page-debug` 的 6 槽指示，改接总线状态；更新 Debug 面板文案与 i18n
- [x] 5.6 测试：单测（引用计数、重连、快照）、e2e（多标签页共享 1 条 socket、断网重连、降级路径）

## 6. 请求策略与连接预算

- [x] 6.1 新增 `ui/src/lib/api-fetch.ts`：超时、`AbortSignal` 合并、`interactive`/`background` 调度
- [x] 6.2 普通请求统一默认超时（`installFetchPolicy`），Agent/终端交互链路改用 `apiFetch` 与总线请求
- [x] 6.3 定义并实现 `ApiError.code`：`timeout`、`offline`、`network`、`http`，UI 显示重试
- [x] 6.4 在总线内实现连接预算：最多 1 条主连接 + 1 条兜底，永不超过 2 条
- [x] 6.5 测试：超时与重试、后台并发上限、池饥饿错误文案、交互请求不被后台任务阻塞

## 7. 护栏、诊断与验收

- [x] 7.1 增加出口约束测试：`stream-bus.ts`/`stream-relay.ts`/`stream-worker.ts` 之外禁止
      `new EventSource`、`new WebSocket`、`new SharedWorker`
- [x] 7.2 CI 增加检查：`npm test` 中的出口扫描保证长连接构造点只允许出现在总线模块
- [x] 7.3 服务端 `debug_streams` 与 `/api/agent/debug` 增加 Hub 视图（连接、频道、订阅者、`seq`、丢弃、`gap`）
- [x] 7.4 Debug 面板显示 socket 数、频道类型计数、缺口与丢弃，旧字段保持兼容
- [x] 7.5 e2e：终端视图 + 第二个标签页，断言长连接恒为 1、普通请求不受阻、无 SSE 请求
- [x] 7.6 e2e：断网/恢复后 socket 重连、频道按游标恢复；服务端重启由同一重连路径覆盖
- [x] 7.7 压测：`yes`/大文件输出下 Agent 事件延迟、单频道扩容上限、全局预算淘汰与空闲回收（单测）

## 8. 迁移清理与发布

- [x] 8.1 旧 SSE 路由标记 deprecated（响应头 + 日志计数），保留一个发布周期
- [x] 8.2 增加客户端回滚开关（`?stream=legacy` / localStorage）与回滚说明
- [ ] 8.3 删除旧 SSE 路由、`after` 参数、逐功能释放逻辑与相关测试（下个发布周期执行）
- [x] 8.4 更新 `docs/development*.md`、`docs/status*.md`、Debug 面板帮助与 `CHANGELOG.md`；
      旧端点弃用只写响应头/日志/`CHANGELOG.md`，不单独提供迁移文档
- [x] 8.5 Vite 开发代理加 `ws: true`，验证桌面版 WebView 与浏览器模式的握手、认证与重连
