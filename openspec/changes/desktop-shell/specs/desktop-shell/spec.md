# Spec Delta

## Purpose

定义 Tauri 桌面壳如何单实例启动、定位并监督独立的 pi-star 进程，如何展示启动与失败状态，
以及如何通过安装包、托盘和权限边界提供安全可恢复的桌面体验。

## ADDED Requirements

### Requirement: 单实例启动

桌面壳 SHALL 单实例运行。首次启动 MUST 启动一个受监督的 pi-star 子进程；后续启动 MUST 复用现有
实例并把窗口带到前台，MUST NOT 启动第二个 pi-star。

#### Scenario: 首次启动

- **WHEN** 用户第一次启动桌面应用且没有其他实例
- **THEN** 桌面壳创建自身窗口并启动一个 pi-star 子进程

#### Scenario: 重复启动

- **WHEN** 应用已在运行时再次双击图标
- **THEN** 第二个进程立即退出，现有实例显示并聚焦主窗口，`pi-star` 进程数仍为 1

#### Scenario: 登录自启与手动启动并发

- **WHEN** 第二个实例带有 `--hidden` 参数
- **THEN** 第二个进程退出且不抢焦点，现有窗口状态保持不变

### Requirement: pi-star 可执行文件定位

发布构建 SHALL 只从桌面应用可执行文件同目录和 Tauri 资源目录定位随包分发的 `pi-star`
（Windows 为 `pi-star.exe`）；MUST NOT 从 `PATH` 执行同名程序。调试构建 MAY 额外查找仓库
`target/debug` 与 `target/release`。定位失败 MUST 显示已尝试的路径。

#### Scenario: 从资源目录启动

- **WHEN** 安装包把 `pi-star` 放在 macOS `Contents/Resources` 或 Linux 资源目录
- **THEN** 桌面壳找到并启动该二进制

#### Scenario: PATH 中的同名程序被忽略

- **WHEN** `PATH` 中存在一个可执行的 `pi-star`，但应用目录和资源目录都没有
- **THEN** 发布构建报“未找到 pi-star”，不执行 PATH 中的程序

#### Scenario: 调试构建回落仓库产物

- **WHEN** 在 `app/` 下运行 `cargo tauri dev`，且仓库根 `target/debug/pi-star` 已构建
- **THEN** 桌面壳使用该产物启动

### Requirement: 启动参数与健康确认

桌面壳 SHALL 以 `--hostname 127.0.0.1`、显式 `--port` 和 `--exit-on-stdin-eof` 启动 pi-star，
管道化 stdin/stdout，并把 stderr 写入桌面日志。创建主窗口前 MUST 请求 `/api/health` 并确认
`name == "pi-star"`；否则 MUST NOT 在该地址加载页面。环境中存在 `PI_WEB_PASSWORD` 时，健康请求
MUST 使用 Basic Auth（用户名 `pi`），且 MUST NOT 把密码写入日志。

#### Scenario: 正常握手

- **WHEN** 子进程输出 `pi-star: listening addr=127.0.0.1:30151` 且健康检查返回 `pi-star`
- **THEN** 桌面壳创建主窗口并导航到 `http://127.0.0.1:30151/`

#### Scenario: 端口被其他程序占用

- **WHEN** 监听端口由非 pi-star 的进程提供服务
- **THEN** 健康确认失败，桌面壳显示错误页，不加载该进程的页面

#### Scenario: 设置了访问密码

- **WHEN** 环境中设置了 `PI_WEB_PASSWORD`
- **THEN** 桌面壳带 Basic Auth 完成健康确认，日志中不出现密码

#### Scenario: 子进程事件解析

- **WHEN** 标准输出出现 `bind-failed`、`assets` 或未知事件
- **THEN** 桌面壳更新内部状态或记录日志，不把事件误当作页面地址

### Requirement: 稳定监听端口

桌面壳 SHALL 默认使用 `30151`，并允许通过桌面偏好修改。同一安装的端口 MUST 在两次启动之间
保持稳定，除非用户显式修改。端口被占用时 MUST 显示失败并允许重试，MUST NOT 静默改用随机端口。

#### Scenario: 默认端口稳定

- **WHEN** 用户未修改端口并连续启动两次
- **THEN** 两次都使用 `30151`，webview 的 localStorage、cookie 与缓存来源保持一致

#### Scenario: 端口冲突不静默切换

- **WHEN** `30151` 已被占用
- **THEN** 桌面壳显示端口冲突错误与修改端口/重试提示，不自动选择其他端口

### Requirement: 启动与失败反馈

桌面壳 SHALL 在普通启动时立即显示 splash 并报告启动阶段；`--hidden`（登录自启）MAY 在成功路径
保持隐藏，但失败 MUST 显示 splash。从启动子进程到主窗口加载完成 MUST 在 20 秒内完成；超时或失败
MUST 在 splash 上显示分类原因、日志末尾、重试与退出操作。普通启动的主窗口页面加载完成前 MUST
保持 splash 可见。

#### Scenario: 正常启动

- **WHEN** pi-star 在期限内就绪且页面加载完成
- **THEN** 主窗口显示，splash 关闭

#### Scenario: 登录自启失败

- **WHEN** 以 `--hidden` 启动但 pi-star 未能就绪
- **THEN** 桌面壳显示失败 splash，而不是静默留在托盘

#### Scenario: 缺少二进制

- **WHEN** 找不到 `pi-star`
- **THEN** splash 显示“安装可能不完整”、已尝试路径和重试/退出按钮

#### Scenario: 启动超时

- **WHEN** 20 秒内没有完成监听或健康检查
- **THEN** splash 显示超时原因和日志末尾，子进程被终止

### Requirement: 崩溃监督与退避重启

桌面壳 SHALL 持续监控 pi-star。非用户请求的退出 MUST 按 1、2、4、8、16 秒退避，最多自动重启
5 次；达到上限 MUST 关闭主窗口并显示失败。端口冲突等启动失败 MUST NOT 自动重启。用户点击重试
MUST 清零退避次数。

#### Scenario: 子进程崩溃后恢复

- **WHEN** pi-star 在运行中异常退出且自动重启次数未达上限
- **THEN** 桌面壳按退避等待后重新启动，并在就绪后重新打开主窗口

#### Scenario: 连续崩溃停止重试

- **WHEN** 自动重启次数达到上限后再次崩溃
- **THEN** 桌面壳停止重启，关闭主窗口并展示失败原因与日志

#### Scenario: 手动重试

- **WHEN** 用户在失败页点击“重试”
- **THEN** 退避计数清零并立即重新启动

### Requirement: 退出与进程树清理

用户退出、关窗策略要求退出或系统信号到达时，桌面壳 SHALL 先请求 pi-star 优雅关闭，超时后强制
终止整个进程树。退出后 MUST 不残留 pi-star、Node/pi RPC、PTY 子进程，监听端口 MUST 释放。

#### Scenario: 托盘退出

- **WHEN** 用户选择“退出”
- **THEN** pi-star 收到终止信号并在期限内退出，桌面壳随后退出

#### Scenario: 桌面壳被 SIGTERM

- **WHEN** 操作系统向桌面壳发送 SIGTERM/SIGINT/SIGHUP
- **THEN** 信号处理器先清理子进程，再退出桌面壳

#### Scenario: 子进程不响应

- **WHEN** pi-star 在 5 秒内没有退出
- **THEN** 桌面壳强制杀死其进程组（Windows 为 Job Object），确认端口释放后退出

### Requirement: 主窗口与原生装饰

主窗口 SHALL 使用系统原生装饰，标题为 `Pi Star`，初始尺寸 1280×840，最小尺寸 900×600，加载
`http://127.0.0.1:<port>/`。桌面壳 MUST NOT 为主窗口授予 Tauri IPC 权限。

#### Scenario: 原生窗口边框

- **WHEN** 主窗口打开
- **THEN** 窗口由操作系统提供标题栏、最小化、最大化和关闭按钮

#### Scenario: 主窗口无 IPC 权限

- **WHEN** 主窗口页面尝试调用任意 Tauri command
- **THEN** 调用被 ACL 拒绝，页面无法访问文件系统、shell 或窗口控制

### Requirement: 托盘与菜单

桌面壳 SHALL 提供状态、打开 Pi Star、重启 Pi Star、开机自启动、关闭窗口时最小化到托盘和退出
菜单项。状态 MUST 反映启动中、运行中、已停止或异常。macOS SHOULD 在有主窗口时显示 Dock 图标，
无窗口时隐藏，并响应 Dock 图标 Reopen。

#### Scenario: 托盘重启

- **WHEN** 用户选择“重启 Pi Star”
- **THEN** 当前子进程被清理并以清零后的退避重新启动

#### Scenario: macOS 无窗口

- **WHEN** 用户关闭 macOS 主窗口且关窗到托盘开启
- **THEN** Dock 图标隐藏、托盘保留，点击 Dock 或托盘可重新打开主窗口

### Requirement: 桌面偏好

桌面壳 SHALL 在应用数据目录维护 `desktop.toml`，包含 schema、autostart、close_to_tray 和 port，
并通过临时文件 rename 原子写入。文件缺失、字段缺失或损坏时 MUST 回落到默认值并自愈。只有桌面壳
写入该文件；设置失败 MUST 不写入新值，并在界面给出可读提示。

#### Scenario: 首次启动

- **WHEN** 应用数据目录没有 `desktop.toml`
- **THEN** 桌面壳使用默认值并物化写入完整文件

#### Scenario: 文件损坏

- **WHEN** `desktop.toml` 无法解析
- **THEN** 应用仍能启动，使用默认值，并重写为合法文件

#### Scenario: 自启动设置失败

- **WHEN** 系统拒绝注册登录项
- **THEN** 偏好文件保持旧值，托盘勾选回滚并显示错误提示

### Requirement: 关闭窗口行为

当 `close_to_tray` 为真时，关闭最后一个窗口 SHALL 保持 pi-star 与托盘继续运行；显式退出 MUST
放行并执行清理。默认值 SHALL 为真。

#### Scenario: 关闭窗口留在托盘

- **WHEN** 用户关闭主窗口且 `close_to_tray = true`
- **THEN** 窗口消失，托盘图标仍在，pi-star 继续运行，可从托盘重新打开

#### Scenario: 显式退出

- **WHEN** 用户从托盘选择“退出”
- **THEN** 关窗到托盘策略不阻止退出，子进程被清理

### Requirement: 桌面标记与权限边界

桌面壳 SHALL 在页面脚本执行前注入只读的
`window.__PI_STAR__ = { version, desktop: true, platform, shell: "tauri" }`。
主窗口 MUST NOT 获得窗口控制、文件系统、shell 或任意自定义命令权限；只有内置 splash（local
origin）MAY 调用 `desktop_action` 完成重试、退出和打开日志。

#### Scenario: 普通浏览器

- **WHEN** 同一页面在普通浏览器打开
- **THEN** `window.__PI_STAR__` 不存在，页面行为与现在一致

#### Scenario: 桌面 webview

- **WHEN** 主窗口加载 pi-star 页面
- **THEN** `window.__PI_STAR__.desktop === true`，且页面没有可用的 Tauri command

#### Scenario: 远程页面越权

- **WHEN** 主窗口页面尝试调用 splash 可用的 `desktop_action`
- **THEN** ACL 拒绝调用，桌面壳不执行任何操作

#### Scenario: splash 重试

- **WHEN** 用户在本地 splash 点击“重试”
- **THEN** splash 通过 `desktop_action("retry")` 触发重启

### Requirement: 桌面模式下的 PWA 行为

当 `window.__PI_STAR__.desktop` 为真时，UI MUST NOT 注册 service worker、显示 PWA 安装提示或依赖
push subscription；普通浏览器 MUST 保持现有 PWA 行为。

#### Scenario: 桌面模式不注册 SW

- **WHEN** 页面在桌面 webview 中加载
- **THEN** `navigator.serviceWorker.register` 不被调用，控制台无 SW 注册错误

#### Scenario: 浏览器模式保持 PWA

- **WHEN** 页面在普通浏览器加载
- **THEN** 现有 service worker 注册和 manifest 行为不变

### Requirement: 安装包组成

桌面安装包 SHALL 同时包含桌面壳与独立的 `pi-star` 二进制。Windows 主程序名 MUST 与
`pi-star.exe` 在忽略大小写后不同；macOS `.app` SHALL 从 `Contents/Resources` 定位 pi-star；
Linux deb/AppImage SHALL 声明 WebKitGTK 等运行依赖。安装完成后首次启动 MUST 在无网络时可用。

#### Scenario: 离线首次启动

- **WHEN** 用户安装后断网启动应用
- **THEN** splash、内嵌 UI、API 和会话读取全部可用，不下载任何前端资源

#### Scenario: Windows 命名冲突

- **WHEN** 构建 Windows 安装包
- **THEN** 桌面主程序为 `PiStarApp.exe`，与 `pi-star.exe` 同时存在且不互相覆盖

#### Scenario: Linux 安装依赖

- **WHEN** 在不含 WebKitGTK 4.1 的系统安装 deb
- **THEN** 包管理器声明并安装 `libwebkit2gtk-4.1-0` 等依赖，应用可启动

### Requirement: 构建顺序与版本一致

发布构建 MUST 先构建 `ui/dist`，再以 release 构建内嵌 UI 的 pi-star，最后构建 Tauri 安装包。
`app/Cargo.toml`、`app/tauri.conf.json` 与 workspace 版本 MUST 一致，不一致时构建 MUST 失败。

#### Scenario: 干净构建

- **WHEN** 在干净检出上执行桌面构建脚本
- **THEN** 脚本依次完成 UI 构建、pi-star release 构建、桌面壳打包，并产出一个可安装文件

#### Scenario: 版本不一致

- **WHEN** `app/Cargo.toml` 与 workspace 版本不同
- **THEN** 构建脚本在打包前失败并打印两个版本
