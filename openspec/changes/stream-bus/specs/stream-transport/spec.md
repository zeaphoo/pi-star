# Spec Delta

## Purpose

定义 pi-star 浏览器端与服务端之间的实时传输：所有流式数据经由一条多路复用长连接传递，
连接数不随组件、分屏或标签页增长；断线、隐藏与冻结标签页是可恢复的常态；普通请求永远
不会因为实时流占满浏览器连接池而假死。

## ADDED Requirements

### Requirement: 每个浏览器只保持一条长连接

前端 SHALL 通过一条 WebSocket（`GET /api/stream`）承载全部实时数据；对同一个源，一个浏览器
profile MUST NOT 同时持有超过一条长连接。任何页面内组件、终端分屏或标签页数量都不得改变
这个数量。

#### Scenario: 打开多个标签页
- **WHEN** 同一浏览器的三个标签页同时打开 pi-star
- **THEN** 服务端只看到来自该 profile 的一条 `/api/stream` 连接，三个标签页共享它

#### Scenario: 页面内大量实时组件
- **WHEN** 页面同时打开 4 分屏终端、2 个 dock 终端和一个文件预览
- **THEN** 浏览器仍只有一条长连接，服务端只看到该连接上的多个频道订阅

### Requirement: 频道订阅与引用计数

前端 SHALL 以频道（`agent:<sid>`、`terminal:<id>`、`file:<path>`、`auth:<provider>`）声明
实时数据需求，并通过订阅句柄取消；同一频道在同一页面内 MUST 只向上游注册一次，
最后一个订阅取消时 MUST 释放对应的上游资源。

#### Scenario: 两个组件订阅同一频道
- **WHEN** 两个组件订阅同一个 `terminal:<id>`
- **THEN** Hub 只向上游注册一个订阅，两个组件各自收到同一份事件

#### Scenario: 取消最后一个订阅
- **WHEN** 某个频道再无订阅者
- **THEN** Hub 释放上游（停止文件监听或登录流程），终端与 Agent 运行时保持原有生命周期

### Requirement: 游标恢复与幂等重连

每个频道 SHALL 携带单调游标，客户端重连 MUST 能用游标恢复：终端按字节偏移续传、文件按
`(size, mtime)` 版本、Agent 按序号并在窗口外以会话快照重读。恢复过程 MUST NOT 产生重复的
终端输出或丢失的 Agent 事件。

#### Scenario: 终端断线重连
- **WHEN** 终端连接中断并携带最后消费的字节偏移重新订阅
- **THEN** 服务端从缓冲中按偏移续传；偏移超出缓冲时下发重置基线，客户端清屏后从基线继续

#### Scenario: 重新挂载的终端
- **WHEN** 切换终端标签页，旧面板卸载后新面板订阅同一 `terminal:` 频道并带着旧游标
- **THEN** 新建的频道忽略该游标并以完整快照响应，终端重绘当前屏幕而不是保持空白

#### Scenario: Agent 断线重连
- **WHEN** Agent 频道在运行中断开并在事件窗口之外重新订阅
- **THEN** 客户端重读会话快照并把快照作为权威状态，后续事件按新序号继续应用

### Requirement: 背压与事件不丢失语义

Hub SHALL 为每条连接维护有界发送队列，并按频道优先级处理拥塞：终端与文件频道允许丢弃
中间帧并下发缺口标记，Agent 频道 MUST NOT 静默丢弃事件；消费者持续过慢时 MUST 关闭连接并
由客户端重连恢复。

#### Scenario: 高频终端输出
- **WHEN** 一个终端每秒产生数百 KB 输出，同时 Agent 正在流式回复
- **THEN** 终端频道按需合并或丢弃中间帧并标记缺口，Agent 事件按顺序完整送达

#### Scenario: 慢消费者
- **WHEN** 某条连接的队列持续满且无法排空
- **THEN** 服务端以可识别的关闭码断开该连接，客户端退避重连并按游标恢复

### Requirement: 频道缓冲按负载伸缩且有硬上限

频道缓冲 SHALL 以初始容量起步、按写入水位动态扩容，并 MUST 遵守每频道与全 Hub 的硬上限；
达到上限时 MUST 按频道类型截断并可识别地标记缺口，MUST NOT 无界增长。

#### Scenario: 大流量终端输出
- **WHEN** 一个终端持续产生数百 MB 输出
- **THEN** 缓冲按水位扩容至该频道上限（默认 20 MiB）后停止增长，超出的旧帧被淘汰并下发缺口或重置基线

#### Scenario: 频道空闲
- **WHEN** 频道连续空闲且使用率低于阈值
- **THEN** 缓冲收缩回初始容量，并把内存归还给全局预算

#### Scenario: 全局预算耗尽
- **WHEN** 所有频道的缓冲总量达到 Hub 预算
- **THEN** 先淘汰终端与文件频道的旧帧，Agent 频道最后处理并由会话快照补偿，诊断记录淘汰次数

### Requirement: 双工命令通道

终端输入、resize 与 Agent 命令 SHALL 通过同一连接的 `cmd` 帧发送；HTTP POST 路由 MAY 保留
为兼容路径，但常规交互 MUST NOT 为每次输入发起一次 HTTP 请求。

#### Scenario: 在终端输入
- **WHEN** 用户在两分屏终端中连续输入
- **THEN** 输入以 `cmd` 帧经既有长连接送达，浏览器不产生额外的终端 POST 请求

### Requirement: 跨标签页共享与降级

长连接 SHALL 由共享 Worker 在标签页之间共享；当共享 Worker 不可用时，MUST 用 Web Locks
选出唯一持有连接的标签页并通过广播频道转发；两者都不可用时 MUST 退化为每页一条连接，
且不得回到逐功能建立连接的旧行为。

#### Scenario: 不支持共享 Worker 的 WebView
- **WHEN** 运行环境没有 SharedWorker
- **THEN** 一个标签页通过 Web Locks 成为持有者，其他标签页经 BroadcastChannel 收发，
  页面总数仍只对应一条连接

#### Scenario: 持有连接的标签页被关闭
- **WHEN** leader 标签页关闭
- **THEN** 锁释放，另一个标签页接管并重连，各页面按游标恢复各自订阅

### Requirement: 连接预算与请求可恢复

前端 MUST 保证长连接不占用普通请求所需的连接配额，并为所有普通请求提供超时、优先级与
可读错误；连接池不可用时 MUST 在有限时间内失败并提示重试，MUST NOT 无限等待。

#### Scenario: 实时流活跃时发送消息
- **WHEN** 页面持有实时流并发送一条聊天消息
- **THEN** 请求在超时预算内得到响应或可读错误，不因连接池被占满而挂起

#### Scenario: 请求超时
- **WHEN** 普通请求超过配置的超时时间仍未完成
- **THEN** UI 显示可重试的错误状态，而不是保持加载中

### Requirement: 隐藏与冻结标签页的恢复

标签页隐藏超过既定期限后 SHALL 退订终端与文件频道并保留 Agent 频道；回到前台或恢复运行后
MUST 按游标重新订阅并补齐状态。该行为 MUST 由总线统一实现，各功能不得自行实现释放逻辑。

#### Scenario: 标签页长时间隐藏
- **WHEN** 一个带 4 分屏终端的标签页隐藏超过期限
- **THEN** 该标签页退订其终端与文件频道，共享连接仍然保留，其他标签页不受影响

#### Scenario: 回到前台
- **WHEN** 隐藏的标签页重新可见
- **THEN** 总线按最后游标重新订阅，终端补齐输出且不重复显示，文件按版本决定是否重读

### Requirement: 传输安全与生命周期

WebSocket 握手 MUST 复用现有 Host/Origin 校验与 cookie 认证；未通过校验的连接 MUST 被拒绝
且不得升级。服务端 SHALL 以心跳检测失联连接，并在优雅关闭时通知客户端以可重连的方式断开。

#### Scenario: 跨源升级请求
- **WHEN** 页面从非受信 origin 尝试建立实时连接
- **THEN** 服务端拒绝升级并返回错误，不建立通道

#### Scenario: 服务端重启
- **WHEN** 服务端优雅关闭后重新启动
- **THEN** 客户端收到关闭通知并按退避重连，恢复各频道订阅与状态

### Requirement: 连接出口的静态约束

长连接与共享 Worker 的创建 SHALL 只允许出现在总线模块中；代码库 MUST 通过 lint 与 CI 检查
阻止在其它模块出现 `EventSource`、`WebSocket` 或 `SharedWorker` 的直接构造。

#### Scenario: 新增实时功能
- **WHEN** 开发者在新组件中直接创建 `EventSource`
- **THEN** lint 或 CI 检查失败，并提示改用总线订阅

### Requirement: 可观测性与诊断

系统 SHALL 暴露实时传输的诊断信息：连接数、每连接的频道、各频道订阅者与序号、缓冲占用、
丢弃字节与缺口次数；Debug 面板 MUST 能显示这些信息以替代逐条连接计数。

#### Scenario: 诊断连接与频道
- **WHEN** 用户打开 Debug 面板
- **THEN** 面板显示当前连接为一条、频道数量与各类频道计数，以及最近一次丢弃或缺口

### Requirement: 迁移期兼容

在迁移期内，旧 SSE 端点 SHALL 继续可用并标记为 deprecated；使用旧端点的客户端 MUST 仍能
正常工作，新客户端 MUST NOT 依赖它们。

#### Scenario: 旧版外部脚本
- **WHEN** 外部脚本仍连接旧的 `/api/agent/{id}/events`
- **THEN** 在弃用周期内其行为保持不变，并通过响应头与日志被告知弃用
