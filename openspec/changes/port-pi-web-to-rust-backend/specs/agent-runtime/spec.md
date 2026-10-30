# Spec Delta

## Purpose

定义 pi-star 的 Agent 运行时：把浏览器命令转换为模型请求、工具执行与持久化事件，并让 pi-web
客户端无需修改即可消费。

## ADDED Requirements

### Requirement: 创建与会话复用
服务端 SHALL 支持 `POST /api/agent/new` 创建新会话（`prompt` 或 `ensure_session`），并按 cwd、
模型、thinking level、工具选择初始化；对同一已有会话的请求 MUST 复用仍存活的运行时实例。

#### Scenario: 新建并发送首条消息
- **WHEN** 客户端提交 `{ cwd, type: "prompt", message }`
- **THEN** 服务端创建会话文件、返回真实 session id，并开始一轮 Agent 运行

#### Scenario: 复用运行中会话
- **WHEN** 客户端对同一个 id 再次发送命令
- **THEN** 服务端复用已有运行时、保留其模型和队列状态

### Requirement: 命令协议
`POST /api/agent/:id` SHALL 接受 pi-web 使用的命令集合，至少包括 `get_state`、`get_tools`、
`get_commands`、`prompt`、`abort`、`abort_bash`、`bash`、`set_model`、`set_thinking_level`、
`set_auto_compaction`、`set_session_name`、`compact`、`abort_compaction`、`fork`、`clone`、
`navigate_tree`、`set_tools`、`clear_queue`、`reload`、`get_session_stats`、
`get_last_assistant_text`、`extension_ui_response` 和 `extension_ui_input`。

#### Scenario: 读取状态
- **WHEN** 客户端发送 `{ type: "get_state" }` 且 `systemPrompt=0`
- **THEN** 返回 running=true 和状态对象，但不包含 system prompt

#### Scenario: 中止运行
- **WHEN** 客户端发送 `{ type: "abort" }`
- **THEN** 当前模型流和工具执行被取消，事件流收到结束事件，未发送的队列消息可被 Recall

### Requirement: SSE 事件流
`GET /api/agent/:id/events` SHALL 以 `text/event-stream` 输出 `data: <json>` 事件，事件类型与
pi-web 客户端一致，并在无事件时每 30 秒发送 `{"type":"heartbeat"}`。

#### Scenario: 建立事件流
- **WHEN** 客户端连接已有或磁盘会话的事件端点
- **THEN** 服务端先返回可用状态，再持续推送 `message_start`、`message_update`、
  `tool_execution_*`、`message_end`、`agent_end` 等事件

#### Scenario: 隐藏事件过滤
- **WHEN** 运行产生 `turn_start`、`turn_end`、`entry_appended` 或 system 消息事件
- **THEN** 服务端不把这些事件发送给浏览器

#### Scenario: 流断开
- **WHEN** 浏览器断开 SSE 连接
- **THEN** 服务端释放该订阅，但会话仍可由租约和空闲策略保持存活

### Requirement: 工具循环与工具事件
Agent Runtime SHALL 执行内置工具 `read`、`write`、`edit`、`bash`、`glob`、`grep`、`ls`，
并为每次调用发送 start/update/end 事件；工具结果 MUST 以 `toolResult` 消息追加到会话。

#### Scenario: 模型请求读取文件
- **WHEN** 模型返回 `read` 工具调用
- **THEN** 服务端执行受根目录限制的读取、发送 `tool_execution_start`/`end`、把结果回传给模型并继续循环

#### Scenario: 工具失败
- **WHEN** 工具返回错误
- **THEN** 服务端发送 `isError: true` 的结束事件，把错误文本写入 toolResult，会话继续而不是崩溃

### Requirement: 消息流式投影
服务端 SHALL 以增量事件报告 assistant 文本、thinking 和 toolCall 参数，并在消息结束时提供完整
消息；MUST 过滤内容签名和重复的大体积图片内容。

#### Scenario: 流式文本
- **WHEN** provider 返回文本增量
- **THEN** 客户端收到 `message_update` 和可拼接的 `partial` 内容，最终收到完整 `message_end`

#### Scenario: 流式工具参数
- **WHEN** provider 分片返回 toolCall 参数
- **THEN** 客户端收到带 `toolCallId`/`toolName` 的增量，最终生成可执行的完整参数

### Requirement: prompt 队列与运行中消息
运行中收到的新 prompt SHALL 按 `steer`/`followUp` 语义排队；`clear_queue` 与 `abort` MUST 能把
未发送消息返回给客户端以便恢复输入。

#### Scenario: 运行中追加指令
- **WHEN** agent 正在运行时客户端提交 `streamingBehavior: "steer"`
- **THEN** 消息在当前工具边界后进入模型上下文，而不创建第二个并发运行

#### Scenario: 中止并恢复队列
- **WHEN** 客户端在排队后发送 `abort`
- **THEN** 返回 steering/followUp 文本和图片，队列清空

### Requirement: 压缩与分支
服务端 SHALL 支持手动和自动压缩，把摘要写入 `compaction` 条目；SHALL 支持 `navigate_tree` 和
`fork`/`clone`，并保持条目父子关系与 entry id 不变。

#### Scenario: 手动压缩
- **WHEN** 客户端发送 `{ type: "compact" }`
- **THEN** 服务端生成摘要、写入 compaction 条目、更新上下文，并返回压缩前后 token 数

#### Scenario: 在历史分支切换
- **WHEN** 客户端发送 `navigate_tree` 到某个 entry id
- **THEN** 后续 prompt 从该 entry 继续，原分支保持可恢复

### Requirement: 子代理
服务端 SHALL 支持内置 subagent 配置与并发上限，子代理会话 MUST 在侧边栏以父子关系显示，
父代理等待子代理完成后才能结束对应回合。

#### Scenario: 启动子代理
- **WHEN** 主代理调用 subagent 工具
- **THEN** 服务端创建带 parentSession 的子会话、流式转发其状态，并在完成后回传摘要

#### Scenario: 子代理失败
- **WHEN** 子代理运行失败或被中止
- **THEN** 父会话收到错误结果，子会话状态标记为 failed/aborted，不阻塞后续工具

### Requirement: 扩展 UI 请求
服务端 SHALL 把扩展发起的 select/confirm/input/editor/notify/setStatus/setWidget/setTitle 等请求
转发为 SSE 事件，并接受客户端的响应；阻塞型请求 MUST 让该轮运行等待或超时取消。

#### Scenario: 扩展要求确认
- **WHEN** 扩展发出 `extension_ui_request` confirm
- **THEN** 客户端看到对话框，响应通过命令接口回传，运行继续或按取消处理

#### Scenario: 请求超时
- **WHEN** 阻塞型扩展请求超过其 timeout
- **THEN** 服务端以取消响应结束等待，并记录可诊断信息

### Requirement: 租约与空闲生命周期
`POST /api/agent/:id/lease` SHALL 重置选中会话的空闲计时而不启动流；GET running SHALL 返回
running、awaitingInput、subagentsActive 和侧边栏 revision。

#### Scenario: 页面保持活跃
- **WHEN** 可见页面周期性续租
- **THEN** 该会话不被空闲回收，返回值包含 running 和 awaitingInput

#### Scenario: 关机
- **WHEN** 服务端收到 SIGTERM
- **THEN** 先给扩展关闭期限、取消运行、关闭 PTY 和 SSE，再退出进程
