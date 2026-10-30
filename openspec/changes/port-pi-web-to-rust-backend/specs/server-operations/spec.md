# Spec Delta

## Purpose

定义 pi-star 作为本地 Web 服务的运行边界：启动参数、同源静态资源、请求安全、密码登录、系统集成、
Web Push、应用更新和进程生命周期。

## ADDED Requirements

### Requirement: 启动参数与环境变量
`pi-star` SHALL 支持 `--port/-p`、`--hostname/-H`、`--no-open`、`--help`，
并识别 `PORT`、`PI_WEB_HOSTNAME`、`PI_WEB_NO_OPEN`、`PI_WEB_PASSWORD`、
`PI_WEB_ALLOWED_HOSTS`、`PI_WEB_IDLE_TIMEOUT_MS`、`PI_WEB_SHUTDOWN_DEADLINE_MS`、
`PI_WEB_APP_NAME`、`PI_WEB_SKIP_VERSION_CHECK`、`PI_CODING_AGENT_DIR`。
默认绑定 `127.0.0.1:30151`。

#### Scenario: 默认启动
- **WHEN** 用户直接运行 `pi-star`
- **THEN** 服务只监听回环地址，成功后在可用时打开浏览器

#### Scenario: 未知参数
- **WHEN** 用户传入未知选项
- **THEN** 打印用法并以非零状态退出，不启动服务

### Requirement: 同源静态服务
服务端 SHALL 在 `/` 提供 UI 构建产物，为带哈希的资产设置长缓存，为 `sw.js` 和
`manifest.webmanifest` 设置不缓存，并对未知非 API 路径回退到 `index.html`。

#### Scenario: 加载前端路由
- **WHEN** 浏览器直接请求 `/login`
- **THEN** 返回 UI 入口，前端路由显示登录页

#### Scenario: API 404
- **WHEN** 请求未知 `/api/...` 路径
- **THEN** 返回 JSON 404，不回退到 HTML

### Requirement: Host 与 Origin 校验
服务端 SHALL 只接受回环、配置的监听地址和 `PI_WEB_ALLOWED_HOSTS` 中的 Host；
对带 Origin 的写请求 MUST 校验 Origin 与 Host 同源，防止 DNS rebinding 和跨站请求。

#### Scenario: 允许的局域网 Host
- **WHEN** 请求 Host 在 allow-list 中
- **THEN** 请求被接受

#### Scenario: 恶意 Origin
- **WHEN** 浏览器从外部站点向本地 API 发起写请求
- **THEN** 服务端返回 403，不执行操作

### Requirement: 密码认证
当设置 `PI_WEB_PASSWORD` 时，服务端 SHALL 要求浏览器登录并签发 HttpOnly、SameSite 会话 cookie；
API 客户端 MAY 使用 Basic Auth（用户名 `pi`）；MUST 对失败尝试限速，且 MUST NOT 把密码写入日志或 URL。

#### Scenario: 浏览器登录
- **WHEN** 用户提交正确密码
- **THEN** 设置会话 cookie 并跳转到原目标页

#### Scenario: 未认证访问 API
- **WHEN** 未携带有效 cookie 或 Basic Auth 请求受保护 API
- **THEN** 返回 401，不泄露文件或会话内容

### Requirement: 请求体与超时限制
服务端 SHALL 对上传请求允许至少 100 MB，对其他 API 使用更小的默认上限；
所有外部网络请求 MUST 设置连接/读取超时，流式请求 MUST 可被取消。

#### Scenario: 超大上传
- **WHEN** 上传超过上限
- **THEN** 返回 413 和可读错误，临时文件被清理

### Requirement: 系统集成
服务端 SHALL 提供 home、default-cwd、cwd validate/browse、open-in-explorer 和 app-update 接口；
`open-in-explorer` MUST 仅对回环请求开放。

#### Scenario: 打开文件管理器
- **WHEN** 回环客户端请求打开已授权 cwd
- **THEN** 服务端调用平台文件管理器并返回成功

#### Scenario: 远程请求打开文件管理器
- **WHEN** 非回环客户端请求该接口
- **THEN** 返回 403

### Requirement: Web Push
`GET /api/push/config` SHALL 返回 VAPID 公钥；`POST /api/push/subscribe` SHALL 保存订阅；
会话完成时若用户已订阅且页面不可见，服务端 SHOULD 发送通知，发送失败 MUST 只记录日志。

#### Scenario: 注册订阅
- **WHEN** 浏览器提交合法 push subscription
- **THEN** 服务端持久化并返回成功

#### Scenario: VAPID 未配置
- **WHEN** 服务端没有 VAPID 密钥
- **THEN** 配置接口明确返回不可用，UI 不反复重试

### Requirement: 调试快照
`GET /api/agent/debug?session=` SHALL 返回只读诊断快照：运行时会话、SSE 流、子代理、MCP、
终端、文件监听和协议版本；任何来源失败 MUST 被隔离并标记，不影响整个快照。

#### Scenario: 无运行时会话
- **WHEN** 请求调试快照时没有活动会话
- **THEN** 返回空集合和服务器时钟，而不是错误

### Requirement: 优雅关闭
服务端 SHALL 在 SIGINT/SIGTERM 时停止接受新连接、取消 Agent 流、给扩展关闭期限、关闭 PTY 和
文件监听，并在期限后强制退出。

#### Scenario: 正常停止
- **WHEN** 用户按 Ctrl-C
- **THEN** 所有子进程退出，端口释放，会话文件保持完整

#### Scenario: 关闭超时
- **WHEN** 扩展不响应关闭
- **THEN** 超过 `PI_WEB_SHUTDOWN_DEADLINE_MS` 后强制关闭并记录
