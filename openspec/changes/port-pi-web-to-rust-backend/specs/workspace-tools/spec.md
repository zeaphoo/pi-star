# Spec Delta

## Purpose

定义 pi-star 对项目工作区的只读与受控写入能力：文件浏览/预览/上传、@ 文件索引、Git 状态与 diff、
worktree 管理、以及浏览器 PTY 终端。

## ADDED Requirements

### Requirement: 文件根目录边界
所有文件、Git、worktree 和终端操作 SHALL 只允许访问已授权根目录：已知会话 cwd、其 projectRoot、
本次进程显式允许的根，以及经过 `allow-link` 确认的目录链接目标。路径 MUST 在解析符号链接后
重新校验，拒绝 `..` 穿越和越界链接。

#### Scenario: 越界读取
- **WHEN** 客户端请求根目录外的 `../../etc/passwd`
- **THEN** 服务端返回 403，不返回文件内容

#### Scenario: 目录链接
- **WHEN** 项目中的目录链接指向根外目录且客户端请求 `allow-link`
- **THEN** 目标被加入允许根，后续读取成功；未确认时返回 403

### Requirement: 文件 API
`GET /api/files/<path>` SHALL 支持 `type=list|read|download|meta|preview|watch`；
`POST` SHALL 支持 `upload|upload-check|allow-link`。列表 MUST 标注目录/文件/隐藏项，
读取 MUST 限制大小并返回语言/编码信息，预览 MUST 支持文本、Markdown、图片、音频、PDF 和 DOCX。

#### Scenario: 列出目录
- **WHEN** 客户端请求 `type=list` 指向已授权目录
- **THEN** 返回条目名称、类型、大小、mtime 和可由 UI 使用的路径

#### Scenario: 大文件保护
- **WHEN** 客户端读取超过配置上限的文本文件
- **THEN** 服务端拒绝或截断并返回明确标记，不把整个文件读入内存

#### Scenario: 上传
- **WHEN** 客户端通过 multipart 上传文件到已授权目录
- **THEN** 服务端校验大小和路径后原子写入，返回最终路径和大小

### Requirement: 文件监听
`type=watch` SHALL 以 SSE 推送文件变化事件，携带 size+mtime 版本；连接断开 MUST 释放监听资源。

#### Scenario: 文件被外部修改
- **WHEN** 被监听文件内容改变
- **THEN** 客户端收到变化事件并能据此重新读取

#### Scenario: 文件未变化重连
- **WHEN** 客户端重连并使用相同版本
- **THEN** 服务端不重复发送完整内容，仅保持连接或发送心跳

### Requirement: @ 文件索引
`GET /api/file-index` SHALL 返回会话 cwd 下可供 @ 提及的文件列表，尊重 `.gitignore` 和忽略规则，
并限制返回数量。

#### Scenario: 大型仓库
- **WHEN** 项目包含数十万文件
- **THEN** 索引在限制内返回可用子集，不阻塞服务端其他请求

### Requirement: Git 状态与 diff
`GET /api/git/status` SHALL 返回 cwd 的当前分支、ahead/behind、变更文件及状态码；
`GET /api/git/diff` SHALL 返回单个变更文件的统一 diff，支持 staged/unstaged。

#### Scenario: 非 Git 目录
- **WHEN** cwd 不是 Git 仓库
- **THEN** 返回空状态而不是 500

#### Scenario: 二进制 diff
- **WHEN** 请求的文件是二进制
- **THEN** 返回二进制标记，不把原始字节放入 JSON

### Requirement: Worktree 管理
`GET/POST/DELETE /api/worktrees` SHALL 列出、创建和删除 Git worktree；同一仓库的 worktree
MUST 在会话分组中共享 projectRoot，删除有未提交改动的 worktree MUST 拒绝或要求确认。

#### Scenario: 创建 worktree
- **WHEN** 客户端提交分支名和目标路径
- **THEN** 服务端执行 Git worktree 创建并返回新 worktree 信息

#### Scenario: 分支已被占用
- **WHEN** 目标分支已被现有 worktree 检出
- **THEN** 服务端返回可读冲突错误并指出占用路径

### Requirement: PTY 终端
`POST /api/terminal` SHALL 在已授权 cwd 创建 PTY 会话；`/api/terminal/:id` SHALL 支持读取信息、
输入、resize 和删除；`/api/terminal/:id/events` SHALL 以 SSE 输出输出块和退出事件。

#### Scenario: 打开终端
- **WHEN** 客户端创建终端并连接事件流
- **THEN** 服务端启动登录 shell、转发输出，并接受键盘输入和尺寸变化

#### Scenario: 关闭终端
- **WHEN** 客户端删除终端或关闭页面
- **THEN** 服务端终止 PTY 及其子进程组，端口和文件描述符被释放
