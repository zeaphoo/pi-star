# Spec Delta

## Purpose

定义 pi-star 二进制在桌面托管场景下的运行契约：前端资源内置、静态资源响应、启动事件、
父进程存活信号，以及脱离桌面壳独立运行的能力。

## ADDED Requirements

### Requirement: 发布构建内置前端资源

pi-star SHALL 在发布构建中把 `ui/dist` 的运行时资源编入二进制并从内存提供；为提供前端资源，
发布二进制 MUST NOT 依赖 `ui/dist` 目录、Node.js 或任何外部静态目录。调试构建 MAY 从磁盘读取
同一目录，以便前端热构建。

#### Scenario: 发布二进制单独运行

- **WHEN** 把 release 的 `pi-star` 复制到没有 `ui/dist` 的临时目录并启动
- **THEN** `/` 返回 UI 入口，JS/CSS/图标/manifest/service worker 均以 200 返回

#### Scenario: 调试构建读取磁盘

- **WHEN** debug 构建运行期间重新执行 `npm run build` 并刷新页面
- **THEN** 服务端返回新的 `ui/dist` 内容，无需重新编译 Rust

#### Scenario: 发布构建没有内置资源

- **WHEN** 编译 release 时 `ui/dist` 不存在或没有任何文件
- **THEN** 进程以非零状态退出，错误信息明确指出缺少前端构建产物

### Requirement: 前端资源目录覆盖

`--ui-dir <dir>`（或 `PI_STAR_UI_DIR`）SHALL 强制从该目录提供前端资源；启动时 MUST 校验
`<dir>/index.html` 存在，缺失时 MUST 以非零状态退出并打印该路径。该覆盖 MUST 只影响静态资源，
不影响 API、会话文件和 Agent 数据目录。

#### Scenario: 使用外部 UI 目录

- **WHEN** 用户传入包含 `index.html` 的 `--ui-dir`
- **THEN** 服务端从该目录提供页面，API 仍使用正常的 agent 目录

#### Scenario: 覆盖目录无效

- **WHEN** `--ui-dir` 指向的目录没有 `index.html`
- **THEN** 进程不启动 HTTP 服务并返回可读错误

### Requirement: 静态资源响应契约

服务端 SHALL 保持同源静态行为：未知非 API 路径回退到 `index.html`；未知 `/api/...` 返回 JSON 404；
`/assets/*` 使用一年 immutable 缓存；`/sw.js` 与 `/manifest.webmanifest` 不缓存；拒绝含 `..` 的
路径；为响应提供正确的 `Content-Type`，未压缩响应提供 `Content-Length`，并支持 `HEAD` 请求。

#### Scenario: 深链接回退

- **WHEN** 浏览器直接请求 `/login` 或不存在的前端路由
- **THEN** 返回 `index.html`，由前端路由渲染对应页面

#### Scenario: API 404 不回退

- **WHEN** 请求未知的 `/api/not-found`
- **THEN** 返回 `application/json` 的 404，不返回 HTML

#### Scenario: 路径穿越被拒绝

- **WHEN** 请求 `/../Cargo.toml` 或编码后的等价路径
- **THEN** 返回 404，且不读取 UI 目录之外的文件

#### Scenario: HEAD 请求

- **WHEN** 客户端对 `/assets/<hash>.js` 发送 `HEAD`
- **THEN** 返回与 GET 相同的状态和头，但响应体为空

### Requirement: 资源来源自诊断

`GET /api/health` SHALL 返回 `assets` 对象，至少包含 `source`（`embedded` 或 `disk`）和 `files`
数量；磁盘来源 MUST 额外包含 `path`。桌面壳 SHALL 能据此判断 UI 是否可用。

#### Scenario: release 健康响应

- **WHEN** release 构建收到 `/api/health`
- **THEN** `assets.source` 为 `embedded`，`files` 大于 0，且不包含磁盘路径

#### Scenario: 磁盘覆盖健康响应

- **WHEN** 使用 `--ui-dir` 启动
- **THEN** `assets.source` 为 `disk`，并返回实际目录路径与文件数

### Requirement: 机器可读启动事件

pi-star SHALL 在标准输出逐行发送以 `pi-star: ` 开头的机器可读事件，所有日志 MUST 写入标准错误。
监听成功后 MUST 输出 `listening addr=<host:port>`；`--port 0` 时 MUST 输出内核分配的实际端口。
监听失败 MUST 输出 `bind-failed kind=<address-in-use|permission-denied|other> addr=<host:port>`，
并以非零状态退出。

#### Scenario: 临时端口启动

- **WHEN** 以 `--port 0` 启动并在空闲端口上绑定成功
- **THEN** 标准输出出现 `pi-star: listening addr=127.0.0.1:<实际端口>`，且标准错误中只有日志

#### Scenario: 端口被占用

- **WHEN** 以已被其他进程占用的端口启动
- **THEN** 标准输出出现 `bind-failed kind=address-in-use`，进程以非零状态退出

#### Scenario: 日志与事件分离

- **WHEN** 设置 `RUST_LOG=debug` 启动
- **THEN** 所有 tracing 日志写标准错误，标准输出仍只有 `pi-star:` 事件行

### Requirement: 父进程存活信号

当以 `--exit-on-stdin-eof` 启动时，pi-star SHALL 在标准输入读到 EOF 后触发与 SIGTERM 相同的优雅
关闭；未指定该参数时 MUST 保持现有行为，不因 stdin 关闭而退出。

#### Scenario: 桌面壳异常消失

- **WHEN** 桌面壳进程被杀死，pi-star 的 stdin 管道关闭
- **THEN** pi-star 在关闭期限内停止监听、清理 Agent/PTY 子进程并退出，端口被释放

#### Scenario: 独立运行不受影响

- **WHEN** 用户直接运行 `pi-star` 并把 stdin 重定向到 `/dev/null`
- **THEN** 未指定 `--exit-on-stdin-eof` 时服务继续运行

### Requirement: 独立运行能力

pi-star SHALL 继续作为独立可执行文件运行，不要求 Tauri 桌面壳、图形环境或桌面应用数据目录；
CLI 参数、环境变量、浏览器访问和退出码行为保持可用。

#### Scenario: 无桌面壳启动

- **WHEN** 用户直接运行 `pi-star --port 30151`
- **THEN** 浏览器可以访问完整 UI，API 与 Agent 功能正常，进程退出后端口释放

#### Scenario: 内置资源不依赖桌面安装

- **WHEN** 只分发 `pi-star` 二进制而未安装 Tauri 应用
- **THEN** UI 仍可从二进制内提供，不需要 `app/` 或 `ui/dist`
