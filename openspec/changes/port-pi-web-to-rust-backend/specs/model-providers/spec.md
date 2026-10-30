# Spec Delta

## Purpose

定义 pi-star 的模型与 provider 管理：模型目录、默认/启用模型、`models.json`、API Key/OAuth 凭据、
模型测试与用量查询。

## ADDED Requirements

### Requirement: 模型目录
`GET /api/models?cwd=` SHALL 返回可用模型、模型列表、默认模型、默认 thinking level、每个模型的
thinking levels 和提示，且 MUST 读取与 pi 相同的 settings/models/凭据来源。

#### Scenario: 无凭据启动
- **WHEN** 用户尚未配置任何 provider
- **THEN** 返回空或仅内置目录，不返回 500，并允许 UI 打开 Models 设置

#### Scenario: 项目不可访问
- **WHEN** cwd 不在允许根目录或已不存在
- **THEN** 返回 403/400 和可翻译的错误原因，而不是模型错误

### Requirement: 默认模型与启用模型
服务端 SHALL 支持读写默认 provider/model、默认 thinking level 和 `enabledModels` 模式列表；
`enabledModels` MUST 支持 glob/模糊匹配，保存时只修改相关配置。

#### Scenario: 设置默认模型
- **WHEN** 客户端 `PUT /api/models/default` 提交 provider/modelId
- **THEN** 新会话默认使用该模型，已打开会话仅在再次选择模型时应用

#### Scenario: 禁用模型
- **WHEN** 客户端 `PUT /api/models/enabled` 移除某个模式
- **THEN** 匹配的模型不再出现在选择器，但会话文件和已保存的模型 id 不被删除

### Requirement: models.json 编辑
`GET/PUT /api/models-config` SHALL 读写 `~/.pi/agent/models.json`，支持 JSONC 注释与尾逗号，
写入 MUST 原子替换并保持文件权限；MUST 拒绝会在 pi 启动时被拒绝的 provider 条目。

#### Scenario: 保存自定义 provider
- **WHEN** 客户端提交合法的 provider、baseUrl、api 和模型列表
- **THEN** 文件被原子写入，后续 `/api/models` 能看到新模型

#### Scenario: 无效 provider
- **WHEN** 提交 oauth 类型但缺少 baseUrl 的 provider
- **THEN** 返回 400 和字段级错误，文件保持不变

### Requirement: 模型测试与发现
`POST /api/models-config/test` SHALL 对单个模型或 provider 发起有界测试请求；
`POST /api/models-config/discover` SHALL 从上游拉取模型列表；两者 MUST 超时并脱敏密钥。

#### Scenario: 测试成功
- **WHEN** 凭据有效且上游可达
- **THEN** 返回成功、延迟和可用模型信息

#### Scenario: 测试失败
- **WHEN** 凭据无效或上游超时
- **THEN** 返回结构化错误，错误文本不包含完整 API Key

### Requirement: Provider 登录与凭据
`GET /api/auth/providers` SHALL 列出 OAuth 与 API Key provider；
`GET /api/auth/login/:provider` SHALL 以 SSE 推进 OAuth/device-code 流程，
`POST` SHALL 接收手动 code；`POST/DELETE /api/auth/api-key/:provider` SHALL 保存或删除 API Key，
`POST /api/auth/logout/:provider` SHALL 清除 OAuth 凭据。

#### Scenario: API Key 登录
- **WHEN** 用户提交有效的 API Key
- **THEN** 服务端以 0600 权限原子写入凭据存储，并立即可用于模型请求

#### Scenario: OAuth 取消
- **WHEN** 用户关闭登录面板
- **THEN** SSE 结束、临时状态清理，凭据文件不变

### Requirement: 模型目录刷新与用量
`POST /api/models/refresh` SHALL 按需刷新 provider 目录；
`POST /api/provider-usage/query` SHALL 查询一个或多个 provider 的配额/用量，失败 MUST 按 provider
隔离并返回可读状态。

#### Scenario: 刷新目录
- **WHEN** 上游目录可用
- **THEN** 新模型进入缓存并在后续列表中可见

#### Scenario: 单个 provider 用量失败
- **WHEN** 一个 provider 拒绝或超时
- **THEN** 其他 provider 的结果仍返回，失败项带错误字段
