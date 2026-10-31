# Proposal

## Why

pi-star 目前是一个需要用户手动启动、再用浏览器访问的本地 Web 服务：端口、进程存活、崩溃恢复和
资源目录都暴露给用户。把它包装成桌面应用，可以在保留现有 Web UI 与独立服务端的同时，提供
双击即用、单实例、托盘常驻和失败可恢复的桌面体验；把 UI 编进二进制后，前端资源不再需要单独的
`ui/dist` 目录或随包分发的 Node.js 构建产物（Agent 后端依赖保持不变）。

## What Changes

- **pi-star 内嵌前端资源**：release 构建用 `rust-embed` 把 `ui/dist` 编入 `pi-star` 二进制并从内存
  提供；debug 构建继续读磁盘以便前端迭代；`--ui-dir` 保留为显式覆盖并在启动时校验。
- **pi-star 桌面运行契约**：标准输出改为逐行机器可读事件（`listening`、`bind-failed`、`assets`），
  日志统一写标准错误；绑定失败给出分类与稳定退出码；新增 `--exit-on-stdin-eof`，父进程消失时
  自动优雅关闭；`/api/health` 暴露资源来源，便于桌面壳判定“这是我自己的 pi-star”。
- **新增 `app/` 桌面壳**：独立 Cargo workspace + Tauri 2，负责定位、启动、健康确认、监督、
  退避重启、优雅关闭和整棵子进程树清理；提供 splash/错误页、主窗口、托盘、单实例、
  开机自启动和关窗到托盘。
- **WebView 安全边界**：主窗口使用系统原生装饰，加载回环地址，不授予任何 Tauri IPC 权限；
  仅内置 splash 页面可以调用桌面壳的重试/退出/打开日志命令。页面只得到一个只读的
  `window.__PI_STAR__` 桌面标记。
- **桌面模式 UI 适配**：检测到桌面标记时不注册 service worker、不显示 PWA 安装提示；普通浏览器
  行为保持不变。
- **构建与分发**：新增“UI → 内嵌 UI 的 pi-star → Tauri 安装包”的构建顺序和版本一致性校验；
  安装包携带独立 `pi-star` 二进制，覆盖 Windows/macOS/Linux 的命名与资源布局。
- **不做破坏性变更**：`pi-star` 仍可独立运行并由浏览器访问；桌面形态是新增的发布与运行方式。
- **不改变 Agent 后端依赖**：默认 `rpc` 后端仍需要 pi/Node，QuickPI 仍是可选的进程内后端；
  本变更只消除前端资源对 `ui/dist`/Node 构建产物的分发依赖。

## Capabilities

### New Capabilities

- `desktop-runtime`: pi-star 二进制在桌面托管场景下的运行契约，包括内嵌前端资源、静态资源响应、
  健康自诊断、机器可读启动事件、父进程存活信号和独立运行能力。
- `desktop-shell`: Tauri 桌面壳的行为契约，包括单实例、子进程定位与监督、端口稳定性、splash/
  错误反馈、托盘与偏好、WebView 权限边界、退出清理和安装包组成。

### Modified Capabilities

<!-- 无：本变更新增的能力覆盖桌面形态；既有浏览器形态的 requirements 不变。 -->

## Impact

- `crates/pi-star`：新增 `web_assets`、桌面事件协议、stdin watchdog、bind 错误分类；静态资源从
  `ServeDir` 改为内置/磁盘双来源；`/api/health` 增加 `assets` 字段。
- `crates/pi-star-core`：`Config.ui_dir` 由固定默认路径改为可选覆盖，HTTP 资源解析上移到服务端。
- `ui/`：新增桌面标记读取与 PWA 条件注册；不改变浏览器端交互与视觉。
- `app/`：新增独立 Tauri 项目、splash 页面、图标、capabilities、桌面偏好与监督器。
- 根 `Cargo.toml`：把 `app` 从 workspace 排除；`.gitignore` 忽略 `app/target`、`app/gen` 和打包时
  临时注入的二进制。
- `scripts/`、文档与 release 流程：新增桌面构建脚本、版本一致性检查和桌面运行说明。
- 依赖：`rust-embed`；Tauri 2、`tauri-build`、`tauri-plugin-single-instance`、
  `tauri-plugin-autostart`、`tray-icon`；Unix `libc`，Windows `windows-sys`（Job Object）。
