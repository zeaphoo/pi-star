# Spec Delta

## Purpose

定义 pi-star 的设置与资源管理面：MCP 服务器、skills、plugins、subagent 配置、工具开关、项目信任
和全局 settings，使浏览器可以完成与 pi CLI 相同的配置操作。

## ADDED Requirements

### Requirement: MCP 配置读取与编辑
`GET /api/mcp` SHALL 只读列出全局 `<agent-dir>/mcp.json` 和项目 `<cwd>/.pi/mcp.json` 的服务器、
文件问题、变量引用和暴露级别；`POST` SHALL 支持 add/enable/disable/remove/undo/set-enabled/
set-exposure/set-in-project/sign-out，写入 MUST 加锁、原子替换并保留未知字段。

#### Scenario: 读取项目 MCP
- **WHEN** 客户端携带 cwd 请求 MCP 概览
- **THEN** 返回全局与项目条目、项目覆盖关系、需要认证的服务器和文件级问题

#### Scenario: 移除后撤销
- **WHEN** 客户端删除服务器后 60 秒内请求 undo
- **THEN** 原条目被恢复，token 失效后撤销返回可读错误

#### Scenario: 非法配置
- **WHEN** mcp.json 不是合法 JSON 或结构错误
- **THEN** 返回问题列表，不启动任何服务器，不覆盖原文件

### Requirement: MCP 密钥与进程环境
服务端 SHALL 对 `env`、header、OAuth client secret 中的密钥进行遮罩；连接 stdio 服务器时 MUST
使用清洗后的环境，且 MUST NOT 传递 `PI_WEB_PASSWORD` 等服务端秘密。

#### Scenario: 展示服务器
- **WHEN** 配置包含 `${API_KEY}` 引用
- **THEN** 响应只显示变量名和遮罩值，不展开真实秘密

#### Scenario: 启动服务器
- **WHEN** 服务端连接 stdio MCP 服务器
- **THEN** 子进程只收到允许的环境变量和配置中显式声明的变量

### Requirement: MCP 测试与 OAuth 登录
`POST /api/mcp/test` SHALL 对一个条目发起有界连接并返回工具/资源摘要；
`POST /api/mcp/sign-in` SHALL 启动或加入 OAuth 流程，`GET/DELETE /api/mcp/sign-in/:flowId` SHALL
轮询状态或取消，`POST` SHALL 接收重定向 URL 或 code。

#### Scenario: 测试成功
- **WHEN** 服务器可达且认证有效
- **THEN** 返回连接状态、工具数量和脱敏后的诊断

#### Scenario: 登录流程取消
- **WHEN** 客户端 DELETE 登录 flow
- **THEN** 流程终止、临时监听端口关闭、凭据不写入

### Requirement: Skills 管理
`GET /api/skills` SHALL 列出已加载 skills、诊断和项目资源状态；
`PATCH` SHALL 批量切换 `disable-model-invocation`；
`GET /api/skills/content` SHALL 返回单个 skill 的有界 Markdown 或其目录内的文档；
`POST /api/skills/install|search|update` SHALL 支持安装、搜索和更新。

#### Scenario: 查看 skill
- **WHEN** 客户端请求已加载 skill 的内容
- **THEN** 返回截断到上限的 Markdown 和元数据，不读取目录外文件

#### Scenario: 模型调用开关
- **WHEN** 客户端批量切换多个 skill
- **THEN** 每个文件独立成功或失败，失败项返回原因且不写坏文件

### Requirement: Plugins 管理
`GET/POST /api/plugins` SHALL 列出、安装、启用、禁用和移除插件包；
`POST /api/plugins/check` SHALL 查询 npm 上的可用更新；网络失败 MUST 不影响已安装插件列表。

#### Scenario: 插件更新检查
- **WHEN** npm registry 可达
- **THEN** 返回每个包当前版本与最新版本

### Requirement: Subagent 设置与 profile
`GET/PUT /api/subagents/settings` SHALL 读写内置 subagent 开关和最大并发；
`GET/POST/DELETE /api/subagents/profiles` SHALL 列出、保存和删除 profile；
`GET /api/subagents/:id` SHALL 返回子代理会话的只读视图。

#### Scenario: 关闭内置 subagent
- **WHEN** 用户关闭开关
- **THEN** 新会话不注册内置 subagent 工具，已有会话在下一次重建后生效

#### Scenario: profile 校验
- **WHEN** 保存缺少必填字段或引用不存在 skill 的 profile
- **THEN** 返回 400 和字段错误，现有文件不变

### Requirement: 工具与全局设置
`GET/PUT /api/tools/settings` SHALL 读写 PowerShell、Code mode 模式与 inline budget；
`GET/PUT /api/cache-miss-notices` SHALL 读写 pi 的 `showCacheMissNotices`；
所有 settings 写入 MUST 使用加锁的读-改-写并保留未知字段。

#### Scenario: 切换 Code mode
- **WHEN** 用户选择 Always
- **THEN** 全局 `defaultTools` 增加 `+codemode`，模式与预算字段按选择写入

#### Scenario: 并发设置写入
- **WHEN** 两个设置面板同时保存不同字段
- **THEN** 两个字段都保留，文件保持合法 JSON

### Requirement: 项目信任
`GET /api/project-trust` SHALL 返回当前 cwd 是否受信任、最近决策、继承关系和项目 MCP 服务器；
`POST` SHALL 记录 trust/untrust 并按需重建该 cwd 的运行时。

#### Scenario: 首次打开含 MCP 的项目
- **WHEN** 项目有 `.pi/mcp.json` 且没有信任决策
- **THEN** UI 收到 requiresTrust=true 和服务器清单，未确认前不运行项目扩展

#### Scenario: 信任继承
- **WHEN** 父目录已被信任
- **THEN** 子目录返回 inherited=true 和决策路径
