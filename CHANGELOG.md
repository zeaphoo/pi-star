# Changelog

本项目的所有重要变更记录在此文件。

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。发布时由 `scripts/release-notes.sh` 抽取对应
版本段落作为 GitHub Release 正文；发布流水线会校验该段落存在。

## [Unreleased]

## [0.1.0-rc.1] - 2026-10-31

首个公开预发布版本。

### Added

- **Rust 后端**：用 Axum 实现 pi-web 的会话、Agent、文件、Git、终端、模型/Provider、
  MCP、skills、plugins、subagents、认证与安全接口，运行时使用 `~/.pi/agent` 数据。
- **Web UI**：从 pi-web 迁移的 Vite/React 界面，保留主题、i18n、PWA、会话树、聊天、文件
  预览、终端和设置面板。
- **Agent 运行时**：按会话选择 `rpc`（`pi --mode rpc`）或进程内 `quick`（QuickPI）后端；
  SSE 事件、prompt 队列、工具循环、压缩、分支、子代理、扩展 UI。
- **桌面应用**：独立 Tauri 2 壳位于 `app/`，单实例、splash/错误页、原生主窗口、托盘、
  开机自启动、关窗到托盘、崩溃退避重启与进程树清理。
- **内嵌前端**：release 构建用 `rust-embed` 把 `ui/dist` 编入 `pi-star`；debug 读磁盘，
  `--ui-dir` 可显式覆盖。
- **自包含 pi 运行时**：安装包携带固定版本的 Node、`@earendil-works/pi-coding-agent`
  1.4.0 生产依赖和 `agent-extension/`；Windows 额外携带 MinGit，目标机器无需预装 Node/pi/Git。
- **跨平台打包**：Linux deb/AppImage、Windows NSIS/MSI/portable zip、macOS .app/dmg；
  macOS 支持 Developer ID 签名与可选公证。
- **CI / Release**：GitHub Actions 在 Linux/Windows/macOS 做编译与测试检查；`v*` 标签触发
  三平台构建并创建 GitHub Release。
- **Windows bash**：优先使用内置 MinGit，其次探测系统 Git for Windows，支持 `PI_STAR_BASH`
  覆盖，排除 WSL 启动器，不回退 cmd.exe。
- **侧边栏品牌**：左侧栏顶部品牌区增加 Pi Star logo。

### Changed

- **静态资源服务**：`ServeDir` 替换为内置/磁盘双来源处理器，保留 SPA 回退、缓存头、
  HEAD 与路径穿越防护；`/api/health` 增加 `assets` 信息。
- **进程生命周期**：新增 `--exit-on-stdin-eof`；stdout 只输出 `pi-star:` 生命周期事件，
  日志写 stderr；端口占用输出 `bind-failed` 并以退出码 2 退出。
- **桌面端口**：默认固定 `30151`（可用桌面偏好覆盖），保持 webview origin 稳定。
- **Linux 分发**：只发布桌面应用，不再单独发布 service 包。

### Fixed

- **版本信息**：欢迎页显示实际版本号与内置 pi 版本（`v0.1.0 · pi v1.4.0`），不再显示
  `pi vpi-compatible-rust`。
- **失效工作区**：启动时记住的新会话目录失效不再卡在“无法打开工作区”；自动丢弃记忆并回到
  默认工作区，显式 `?cwd=` 错误页提供重试与选择项目。
- **macOS 公证**：`stapler` 取票遇到 Apple CloudKit 瞬时网络错误（Error 65/68、NSURLError
  -1005）时自动重试 4 次、间隔 20 秒，不再让已通过公证的 dmg 因取票抖动失败。
- **Release 构建**：Windows 下用 PowerShell 解压 Node/MinGit 的 zip，并给 GNU tar 加
  `--force-local`，避免 `D:` 被当成远程主机；pi 依赖安装加 `--legacy-peer-deps`，规避 npm 10
  的 `edgesOut` 崩溃；Windows 运行时 staging 放在仓库同盘的 `app/pi-runtime`，避免 Tauri
  规范化资源路径时丢掉盘符（`C:\pirt` → `\pirt`）。NSIS 内容断言会先把 7-Zip 输出的
  反斜杠归一化，避免把已打包的 `pi-runtime\node.exe` 误判为缺失。
- 桌面壳被 `SIGKILL` 时通过 stdin EOF 与进程组/Job Object 清理子进程树，避免孤儿进程。
- 桌面模式跳过 service worker/PWA 注册，避免 Tauri webview 的缓存行为干扰本地服务。
- Windows 安装包启用 long paths，避免内置 `node_modules` 路径超限。

[Unreleased]: https://github.com/zeaphoo/pi-star/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.1
