# Spec Delta

## Purpose

定义 pi-star 对 pi 会话文件的完整工作区能力：发现、读取、分支、修改、删除、搜索和侧边栏状态，
并在运行时会话与磁盘会话之间保持一致视图。

## ADDED Requirements

### Requirement: 会话列表与项目分组
服务端 SHALL 扫描 pi agent 目录（默认 `~/.pi/agent/sessions`）下的 JSONL 会话文件，返回 id、路径、
cwd、标题、首条消息、时间、消息数、父会话、projectRoot、projectKey、branch、worktree 标记、
运行/等待输入状态。列表 MUST 支持 `summary=1` 的轻量模式，并能增量刷新。

#### Scenario: 首次加载侧边栏
- **WHEN** 客户端请求 `GET /api/sessions?summary=1`
- **THEN** 服务端返回所有已知会话的摘要、运行中 id、等待输入 id 和列表版本号

#### Scenario: 外部进程追加会话
- **WHEN** pi CLI 向同一会话文件追加内容后客户端使用 `force=1` 刷新
- **THEN** 服务端重新读取磁盘并返回最新消息、条目数和版本号

### Requirement: 会话读取与分支视图
服务端 SHALL 通过会话 id 或文件路径读取会话头、条目和树，返回活动分支的上下文分页、entryIds、
最旧条目、是否还有更早分页、thinking/model 状态、统计和快照版本。

#### Scenario: 读取最近一页
- **WHEN** 客户端请求 `GET /api/sessions/<id>?tail=50`
- **THEN** 服务端返回最近 50 个可见消息/压缩条目及其 entryIds，并标记 `hasMore`

#### Scenario: 向上分页
- **WHEN** 客户端携带 `before=<oldestEntryId>` 请求更早内容
- **THEN** 服务端从该条目的父条目继续向根方向返回一页，且不重复 `before`

#### Scenario: 会话不存在
- **WHEN** 请求的 id 在磁盘和运行时都不存在
- **THEN** 服务端返回 404 和 `{ "error": "Session not found" }`

### Requirement: 会话标题与运行时状态
服务端 SHALL 支持重命名会话、读取运行时状态、把运行中会话合并进持久化列表，并在会话结束或
空闲超时后释放运行时资源。

#### Scenario: 重命名磁盘会话
- **WHEN** 客户端 `PATCH /api/sessions/<id>` 提交非空 `name`
- **THEN** 服务端在会话文件追加 `session_info` 条目，并在后续读取中返回新标题

#### Scenario: 空闲回收
- **WHEN** 运行时会话超过配置的空闲时间且没有活动租约
- **THEN** 服务端关闭 Agent 会话、保留会话文件，并把该 id 从 running 列表移除

### Requirement: 会话删除与 fork
服务端 SHALL 支持删除会话及其子代理会话，且 MUST 保留下属会话的父关系可解析；SHALL 支持把
指定分支复制为新的独立会话文件，并返回新 id。

#### Scenario: 删除一个带子代理的会话
- **WHEN** 客户端删除父会话
- **THEN** 父会话与其子代理会话被删除，其他会话不被误删，正在运行的会话被保留并报告

#### Scenario: 从消息 fork
- **WHEN** 客户端在指定 entry 上请求 fork
- **THEN** 服务端创建只包含该分支祖先链的新会话文件，并返回新 session id

### Requirement: 会话搜索与统计
服务端 SHALL 提供按标题、首条消息和消息正文的搜索；SHALL 按请求的 id 集合返回 token、成本、
工具调用次数和活跃时长统计。

#### Scenario: 搜索关键词
- **WHEN** 客户端 `GET /api/sessions/search?q=<term>`
- **THEN** 服务端返回匹配会话及匹配片段，结果按最近修改排序

#### Scenario: 统计多个会话
- **WHEN** 客户端 `POST /api/sessions/stats` 提交 id 数组
- **THEN** 服务端对每个存在的会话返回统计；不存在的 id 以缺失项报告而不是使整个请求失败

### Requirement: 侧边栏 UI 状态持久化
服务端 SHALL 在 agent 目录的 `pi-web-session-state.json` 中保存 pin、archive、项目顺序和项目名，
写入 MUST 串行化、加锁、原子替换，并在文件损坏时备份而不是覆盖。

#### Scenario: 并发写入
- **WHEN** 两个请求同时修改侧边栏状态
- **THEN** 两次修改按顺序生效，文件始终是完整 JSON，revision 单调递增

#### Scenario: 损坏状态文件
- **WHEN** 状态文件无法解析
- **THEN** 服务端把原文件重命名为 `.corrupt-<timestamp>`，以空状态继续并提供 revision

### Requirement: 导出会话
服务端 SHALL 生成包含消息、工具结果和元数据的自包含 HTML 导出文件，并正确转义用户内容。

#### Scenario: 导出会话
- **WHEN** 客户端请求 `GET /api/sessions/<id>/export`
- **THEN** 下载的 HTML 可在无网络环境打开，脚本与用户文本不会注入执行

### Requirement: 转录提示与缓存命中信息
服务端 SHALL 在读取会话时按 pi 的全局 `showCacheMissNotices` 设置在消息上附加缓存未命中提示，
并 MUST 在设置关闭时不附加。

#### Scenario: 关闭缓存提示
- **WHEN** 全局设置为 `showCacheMissNotices=false`
- **THEN** 会话上下文响应不包含缓存未命中提示，且快照版本反映该变化
