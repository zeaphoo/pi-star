# Pi Star 桌面壳

`app/` 是一个独立的 Cargo workspace（根 workspace 已 `exclude = ["app"]`），用 Tauri 2
启动并监督仓库里的 `pi-star` 二进制。业务代码、会话、Agent 和 HTTP API 都不在这里。

## 组成

```text
src/main.rs        Builder、单实例、自启动、托盘、RunEvent
src/config.rs      数据目录、端口、定位 pi-star、健康确认
src/prefs.rs       desktop.toml（autostart / close_to_tray / port）
src/desktop.rs     系统登录项与托盘勾选项
src/supervisor.rs  spawn / 事件解析 / 健康轮询 / 退避重启 / 进程树清理
src/window.rs      splash、主窗口、状态注入、Dock 行为
src/commands.rs    仅 splash 可用的 desktop_action
ui/index.html      内置 splash/错误页（无构建步骤）
capabilities/      只授权 splash 的 allow-desktop-action
```

## 与 pi-star 的契约

- 启动参数：`--hostname 127.0.0.1 --port <port> --exit-on-stdin-eof`。
- stdout 只读生命周期事件：`pi-star: assets ...`、`pi-star: listening addr=...`、
  `pi-star: bind-failed kind=...`。
- stderr 写到 `<app data>/pi-star.log`。
- 创建主窗口前请求 `/api/health`，必须返回 `"name":"pi-star"`；`PI_WEB_PASSWORD` 存在时带
  Basic Auth。
- 壳退出或崩溃时 stdin 管道关闭，pi-star 通过 `--exit-on-stdin-eof` 优雅关闭。

主窗口加载 `http://127.0.0.1:<port>/`，使用原生装饰，**没有任何 Tauri IPC 权限**。
只有内置 splash（local origin）可以调用 `desktop_action("retry"|"quit"|"open-log")`。

## 开发

```bash
cargo build -p pi-star          # 在仓库根构建 debug 服务
cd ui && npm run build && cd .. # debug 壳从 disk 读 UI
cd app && cargo tauri dev
```

UI 热更新：`cd ui && npm run dev`，然后在 debug 构建下设置
`PI_STAR_DESKTOP_URL=http://127.0.0.1:30152/`。该模式跳过 spawn 与监督。

## 打包

```bash
./scripts/check-desktop-version.sh
./scripts/package-desktop.sh          # 当前平台
./scripts/package-linux-app.sh        # deb + AppImage
./scripts/package-windows.sh          # NSIS + MSI + portable zip（Git Bash）
./scripts/package-mac.sh              # .app + dmg
```

脚本会先准备 `app/pi-runtime/`（Node + pi + agent-extension），构建 UI 和内嵌 UI 的 release
`pi-star`，复制到 `app/pi-star`，用临时 `--config` 注入 `bundle.resources`，最后调用
`cargo tauri build`。`app/pi-star`、`app/pi-runtime`、`app/.bundle-resources.json` 都是打包
临时文件，已在根 `.gitignore` 中忽略。

安装包内 `pi-star` 与 `pi-runtime/` 位于同一资源目录（Windows 的 `pi-runtime/git/` 为内置
MinGit）；`pi-star` 启动时自动发现并设置
`PI_STAR_PI_NODE`、`PI_STAR_PI_JS`、`PI_STAR_AGENT_EXTENSION`、`PI_PACKAGE_DIR`。用户仍可用
`PI_STAR_PI_CLI` 指向自己的 pi。

## 数据目录

Linux 为 `~/.local/share/xyz.pi-star.desktop/`（macOS/Windows 由 Tauri 按 identifier 解析）：

- `desktop.toml`：桌面偏好；
- `pi-star.log`：子进程 stderr 与事件（启动时 >5 MB 轮转）；
- `desktop.log`：壳自身的偏好/自启动错误。

pi-star 的会话与设置仍在 `~/.pi/agent`。

## 已知边界

- 不提供 frameless 标题栏和窗口控制桥；`app/DESIGN.md` 是更早的实验设计。
- 安装包未签名/未公证。
- 端口冲突时不自动换端口（稳定 origin 优先）。
