# Tasks

## 1. Workspace 与可运行骨架

- [x] 1.1 创建 Cargo workspace、`crates/pi-star-core`、`crates/pi-star`，加入 axum/tokio/serde/tracing 等基础依赖
- [x] 1.2 实现 `pi-star` CLI：`--port`、`--hostname`、`--no-open`、`--help`、环境变量与默认值
- [x] 1.3 实现 Axum 启动、静态资源目录、`/api/health`、优雅关闭与日志
- [x] 1.4 建立 `ui/` Vite + React + TypeScript 工程，配置 `@/` 别名与代理到 Rust
- [x] 1.5 编写 UI 复制脚本与 `ui/UPSTREAM.md`，记录 pi-web 来源 commit 和客户端模块清单

## 2. UI 复制与宿主适配

- [x] 2.1 从 pi-web 复制 `AppShell`、聊天、会话树、文件、Git、终端、设置、Debug 等客户端模块
- [x] 2.2 用本地 shim 替换 `next/navigation`、`next/image`、`next/font`，补齐 `index.html` 与入口
- [x] 2.3 移除前端对 pi SDK 的运行时导入，补齐客户端所需的类型声明和 `parseStreamingJson`
- [x] 2.4 复制 CSS、图标、manifest、service worker、i18n 资源并验证生产构建
- [x] 2.5 保留 MIT 来源声明，确保 `npm run build` 产物可由 Axum 直接服务

## 3. 会话存储与读取

- [x] 3.1 实现 pi 会话路径编码、agent 目录解析、JSONL 逐行解析与损坏行容忍
- [x] 3.2 实现会话列表扫描、增量索引、summary 模式、projectRoot/projectKey/branch/worktree 信息
- [x] 3.3 实现 `GET /api/sessions` 与运行中/等待输入/列表版本字段
- [x] 3.4 实现 `GET /api/sessions/:id`：header、tree、活动分支上下文、分页、stats、snapshotRevision
- [x] 3.5 实现 `PATCH/DELETE /api/sessions/:id`、`POST /api/sessions/delete`
- [x] 3.6 实现 fork/clone 与 `POST /api/sessions/fork`
- [x] 3.7 实现 search、stats、export、auto-name 路由
- [x] 3.8 实现 `GET/POST /api/sessions/ui-state` 与加锁原子持久化
- [x] 3.9 实现 transcript notices、tool-result image/thinking 懒加载端点

## 4. 文件、Git 与终端

- [x] 4.1 实现允许根集合、路径规范化/符号链接校验和 `allow-link`
- [x] 4.2 实现 `/api/files` 的 list/read/download/meta/preview/upload/upload-check
- [x] 4.3 实现文件 watch SSE 与版本去重
- [x] 4.4 实现 `/api/file-index`（gitignore 感知、数量上限）
- [x] 4.5 实现 `/api/git/status`、`/api/git/diff`
- [x] 4.6 实现 `/api/worktrees` 的 list/create/remove
- [x] 4.7 实现 PTY 终端创建、输入、resize、删除与事件流
- [x] 4.8 实现 home、default-cwd、cwd validate/browse、open-in-explorer

## 5. Agent Runtime

- [x] 5.1 实现运行时注册表、会话生命周期、租约、空闲回收和 running 快照
- [x] 5.2 实现命令枚举与 `POST /api/agent/new`、`POST/GET /api/agent/:id`
- [x] 5.3 实现 SSE 事件总线、事件精简/过滤、30 秒心跳和断线清理
- [x] 5.4 实现 prompt 队列、steer/followUp、abort、clear_queue
- [x] 5.5 实现模型流式适配与 message_start/update/end 投影
- [x] 5.6 实现工具循环与 read/write/edit/bash/glob/grep/ls 事件
- [x] 5.7 实现 set_model、set_thinking_level、set_tools、set_session_name、reload 等命令
- [x] 5.8 实现 compact/auto-compaction、navigate_tree、fork/clone
- [x] 5.9 实现 subagent 创建、并发限制、父会话等待与状态显示
- [x] 5.10 实现扩展 UI 请求/响应与超时

## 6. 模型与 Provider

- [x] 6.1 实现模型目录读取、内置目录、models.json 合并与缓存
- [x] 6.2 实现 `/api/models`、`/api/models/default`、`/api/models/enabled`、`/api/models/refresh`
- [x] 6.3 实现 `/api/models-config` CRUD、catalog、discover、test
- [x] 6.4 实现 OpenAI-compatible 与 Anthropic 流式 provider
- [x] 6.5 实现凭据存储、API Key 保存/删除、OAuth/device-code 登录与登出
- [x] 6.6 实现 `/api/auth/providers` 与 `/api/provider-usage/query`

## 7. 设置与资源

- [x] 7.1 实现全局 settings 的加锁读-改-写与 `/api/tools/settings`、`/api/cache-miss-notices`
- [x] 7.2 实现 MCP 配置读写、增删改/撤销、变量遮罩与状态
- [x] 7.3 实现 MCP test、OAuth sign-in flow 与 stdio/HTTP 传输
- [x] 7.4 实现 skills 列表/内容/批量开关/安装/搜索/更新
- [x] 7.5 实现 plugins 列表/安装/启用/禁用/移除/更新检查
- [x] 7.6 实现 subagent settings/profiles 与运行时
- [x] 7.7 实现 project-trust 判定、继承和信任后重建

## 8. 服务运行与安全

- [x] 8.1 实现静态资源缓存头、SPA fallback、API 404
- [x] 8.2 实现 Host/Origin 校验、body limit、请求超时和错误 JSON
- [x] 8.3 实现 `PI_WEB_PASSWORD` 登录、cookie、Basic Auth 与失败限速
- [x] 8.4 实现 Web Push 配置/订阅/完成通知
- [x] 8.5 实现 app-update、debug 快照、web-auth 路由
- [x] 8.6 实现 SIGINT/SIGTERM 清理、PTY/子进程进程组回收

## 9. 验证与发布

- [x] 9.1 为会话解析、路径安全、settings 原子写、SSE 投影增加 Rust 单测
- [x] 9.2 用真实 pi 会话夹具验证 list/read/context/fork 兼容性
- [x] 9.3 增加 API 集成测试（会话、文件、Git、Agent SSE）
- [x] 9.4 增加前端构建与浏览器 smoke test
- [x] 9.5 编写 README、配置说明、架构文档和从 pi-web 迁移说明
