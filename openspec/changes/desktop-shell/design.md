# Design

## Context

pi-star 现在是一个独立的 Axum 服务：release 构建从 `ui/dist` 读取前端资源，默认监听
`127.0.0.1:30151`，日志通过 tracing 输出，浏览器是唯一宿主。`app/DESIGN.md` 记录过一次
“frameless + 注入桥”的窗口控制实验，但没有桌面生命周期、安装包和资源内置方案。

本次设计参考 `~/workspace/solidway/solidway/app`：Tauri 只做壳，业务进程独立；壳通过子进程
标准输出的事件获知监听地址，用 splash 承载启动/失败状态，用托盘管理常驻。与 solidway 的差异是
本项目的 Web 资源编入 pi-star 二进制，因此桌面壳不需要 `frontendDist` 之外的 UI 资源，也不依赖
`ui/dist` 在安装目录中存在。

## Goals / Non-Goals

**Goals:**

- 一个 `pi-star` 二进制同时满足“独立服务 + 桌面应用的后端”，前端资源内置。
- 桌面壳负责启动、健康确认、监督、退避重启和整棵进程树清理。
- 主窗口加载回环地址，但不把 Tauri 能力暴露给远程页面。
- 端口稳定，保证 webview 的 localStorage、cookie 与缓存来源跨启动一致。
- 安装包只依赖系统 WebView 运行时；前端资源不依赖 Node.js、`ui/dist` 或网络。Agent 后端沿用
  现有 pi RPC/QuickPI 依赖，本变更不改变这一边界。

**Non-Goals:**

- 不在本变更中实现 frameless 自定义标题栏和 `window.__PI_STAR__.windowControl`。
  `app/DESIGN.md` 的桥方案留作后续独立变更；届时需要重新评估远程 origin 的 ACL，并同步修订
  既有 `web-ui` delta 中的窗口控制场景。
- 不实现“连接到已在运行的外部 pi-star”。端口冲突时提示用户关闭外部实例或修改端口，避免把
  不受本壳监督的进程当作自己的后端。
- 不实现自动更新、代码签名/公证、原生通知和崩溃上报；安装包先产出未签名的本地构建。
- 不改变浏览器访问 pi-star 的现有交互与 API 形状。
- 不把 pi-star 的 Agent/会话数据迁到桌面应用数据目录；仍使用 `~/.pi/agent`。

## Decisions

### 1. 前端资源在 release 编译期内置，debug 读磁盘

在 `crates/pi-star` 新增 `web_assets` 模块：

```rust
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../ui/dist"]
#[allow_missing = true]
struct UiAssets;
```

- `--ui-dir` / `PI_STAR_UI_DIR` 显式给出时：磁盘来源，启动时校验 `index.html`。
- 未给出且是 release：内置来源；若 `UiAssets::iter().count() == 0`，启动失败并提示先构建 UI。
- 未给出且是 debug：磁盘来源，路径为 `CARGO_MANIFEST_DIR/../../ui/dist` 的绝对路径。

静态响应从 `tower_http::services::ServeDir` 改成一个 fallback handler：路径穿越检查 → 精确命中 →
`index.html` 回退；设置 `Content-Type`/`Content-Length`/`Cache-Control`。`GET` 路由同时响应
`HEAD`。`/api` 的 JSON 404 由外层路由保持。`AppState` 持有解析后的 `WebAssets`，`app::router`
用它创建 fallback；`Config` 只保留 `ui_dir: Option<PathBuf>` 作为显式覆盖。

备选方案：

- **继续要求 `ui/dist` 随包分发**：安装包多一个目录、易被用户移动/删除，且“单二进制”目标落空。
- **把前端打进 Tauri 的 `frontendDist`**：壳会以 `tauri://localhost` 提供页面，API 却来自
  `http://127.0.0.1:<port>`，变成跨源请求，需要重做 CORS/cookie，且 pi-star 独立运行时仍缺 UI。

`rust-embed` 在 release 默认把文件编入；debug 默认读磁盘。为了避免 `allowed_missing` 掩盖发布
错误，release 启动时显式检查文件数。无需在 debug 重新编译 Rust 即可验证 UI 改动。

### 2. 端口固定为 30151，而不是由内核随机分配

Webview 的 localStorage、cookie、IndexedDB、Cache Storage 和 service worker 都以 origin 为键，
其中 origin 包含端口。若每次启动随机端口，用户主题、侧边栏宽度、登录 cookie 每次都会丢失。
因此桌面壳默认传 `--port 30151`，并把用户覆盖值持久化到 `desktop.toml`；端口冲突时失败并提示，
不静默换端口。

pi-star 仍支持 `--port 0` 并把实际端口写入 `listening` 事件，供测试和独立运行使用。

备选方案：

- **`--port 0` + 运行时动态 ACL**：省掉端口冲突，但 origin 每次变化，用户状态全部重置；
  动态 ACL 还会把权限绑定到未知端口，安全面更大。
- **自定义协议反代 `pi://localhost`**：origin 稳定，但 wry 的 custom protocol handler 对 SSE
  流式响应的支持不可靠，Agent 事件流会退化；否决。
- **随机端口 + 持久化上次端口**：冲突时仍会换端口并丢状态，复杂度却更高。

### 3. 标准输出只承载机器事件，日志走标准错误

定义稳定的事件行（前缀 `pi-star: `）：

```text
pi-star: assets source=embedded files=234
pi-star: listening addr=127.0.0.1:30151
pi-star: bind-failed kind=address-in-use addr=127.0.0.1:30151
```

- 事件通过带锁的 `println!` 后立即 flush，避免与日志交错。
- tracing subscriber 显式 `.with_writer(std::io::stderr)`。
- 绑定失败时先输出 `bind-failed`，再以退出码 `2` 结束；其他启动错误退出码 `1`。
- 事件格式用纯函数生成并单测，壳侧用同样的解析规则单测。

备选方案：解析现有 `Pi Star is running at http://...` 人类日志。它会随日志格式和 locale 漂移，
且端口被占用时没有结构化原因；否决。

### 4. 父进程存活用 stdin EOF，跨平台且不依赖信号

桌面壳以 `Stdio::piped()` 打开子进程 stdin，并把 `ChildStdin` 保存在监督器里；退出时关闭管道。
pi-star 新增 `--exit-on-stdin-eof`：tokio 任务读 stdin，读到 EOF 或错误就取消同一个
`CancellationToken`，走现有的优雅关闭路径。未指定该参数时完全不启动该任务。

Unix 额外用 `setsid()` 让子进程成为进程组组长，清理时 `kill(-pid, SIGTERM)`，5 秒后
`kill(-pid, SIGKILL)`。Windows 使用 Job Object（`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`）并
`CREATE_NO_WINDOW`，保证壳异常退出时子进程树一起结束。

备选方案：Linux `PR_SET_PDEATHSIG` 只能覆盖 Linux 且只作用于直接子进程；`kill -9` 处理父进程
无法跨平台；都保留为兜底而不是主机制。

### 5. 桌面壳是独立 workspace，Tauri 根目录就是 `app/`

```text
app/
  Cargo.toml            [workspace] 独立；tauri 2 + 插件
  Cargo.lock
  build.rs              tauri_build + AppManifest::commands(&["desktop_action"])
  tauri.conf.json       productName PiStar, identifier com.pistar.desktop, mainBinaryName PiStarApp
  capabilities/default.json   仅 splash 窗口的 allow-desktop-action
  icons/                tauri icon 生成
  src/
    main.rs             Builder、插件、setup、RunEvent
    config.rs           data_root、端口、locate_pi_star、health_ok
    supervisor.rs       spawn / 事件解析 / 监控 / 重启 / 清理
    window.rs           splash / main / 状态注入
    prefs.rs            desktop.toml
    desktop.rs          自启动、关窗策略、托盘勾选项
    commands.rs         desktop_action("retry"|"quit"|"open-log")
  ui/index.html         splash + 错误页（无构建步骤）
```

根 `Cargo.toml` 加 `exclude = ["app"]`，避免两个 workspace 的 feature/target 互相影响。`app/`
有自己的 `Cargo.lock`，构建脚本用 `cargo metadata` 解析两个 target 目录。

### 6. 定位、启动与健康确认

定位顺序：

1. 桌面可执行文件同目录（Windows 安装目录、便携版）。
2. Tauri `resource_dir()`（macOS `Contents/Resources`、Linux `/usr/lib/PiStar`、deb/AppImage）。
3. 仅 debug：仓库根 `target/{debug,release}`，再最后 `PATH`。

发布构建不查 `PATH`，避免执行用户环境里的同名恶意程序。每个候选路径都去重并记录，失败页展示。

启动命令：

```text
pi-star --hostname 127.0.0.1 --port <prefs.port> --exit-on-stdin-eof
```

`stdin` 管道保留；`stdout` 由读取线程解析；`stderr` 直接重定向到
`<app_data>/pi-star.log`（Windows 同时 `CREATE_NO_WINDOW`）；启动时若日志超过 5 MB，先轮转为
`pi-star.log.1`，避免长期运行无限增长。创建主窗口前对
`/api/health` 做一次原始 TCP GET，要求响应包含 `"name":"pi-star"`；若 `assets.files == 0`，按
“UI 未构建”处理。健康检查用 600ms 超时，失败重试到 20 秒总期限。若环境中存在
`PI_WEB_PASSWORD`，健康请求附带 Basic Auth（用户名 `pi`），避免密码保护把桌面壳挡在门外；
密码只从环境读取，不写日志。

仅 debug 构建支持 `PI_STAR_DESKTOP_URL`：设置后跳过 spawn，直接加载该 URL（通常指向 Vite
`30152`）用于 UI 热更新；此时不启用监督，缺失 `pi-star` 也不算失败。

备选方案：只依赖 `listening` 事件就导航。端口可能被其他本地进程占用，加载陌生页面既不安全，
也无法区分“我的 pi-star”与“别人的服务”；健康确认是必要的归属校验。

### 7. 窗口、splash 与状态注入

- splash：520×380、不可缩放、居中，`WebviewUrl::App("index.html")`。页面暴露
  `window.pistarSetStatus(status, hint, detail, failed, logPath)`，由 Rust 用 `webview.eval()`
  写入，避免依赖 JS 打包。
- 主窗口：原生装饰、标题 `Pi Star`、1280×840、最小 900×600、`visible(false)` 创建；页面
  `PageLoadEvent::Finished` 后 `show` + `set_focus`，关闭 splash。页面加载与握手共享 20 秒启动
  总期限，剩余时间用完则关闭主窗口并回到失败 splash。`--hidden` 启动成功时不创建/不显示窗口，只留托盘；失败仍创建 splash。
- 失败/崩溃：关闭主窗口，确保 splash 存在并注入原因与日志末尾；`retry` 走监督器重启。
- 配置/数据目录解析失败：仍创建 splash，显示 fatal 错误与退出按钮，避免无窗口静默退出。
- splash 的“重试 / 退出 / 打开日志”通过自定义命令 `desktop_action` 实现。capability 只对
  `splash` 窗口、`local: true` 生效；主窗口没有 capability，因此远程页面调不到任何 command。

`withGlobalTauri: true` 是必需的，因为 splash 是无构建步骤的原生 HTML；它会把 `__TAURI__`
注入所有 webview，但 ACL 决定主窗口什么都不能调用。

### 8. 监督状态机与重试策略

```text
Idle ──start──▶ Starting ──listening+health──▶ Running
                   │                              │
                   ├──spawn/bind/timeout──▶ Failed │
                   └──child exit────────────▶ Stopped
Running ──child exit──▶ Restarting ──backoff──▶ Starting
Running/Failed ──user retry──▶ Starting（restarts = 0）
```

- `generation: u64` 每次启动 +1；旧 stdout reader / monitor 线程看到 generation 不匹配即退出，
  避免重启后旧事件污染新状态。
- 自动重启只针对“运行中意外退出”；spawn 失败、bind 失败、健康失败和超时不进入自动重启。
- 退避 1、2、4、8、16 秒，最多 5 次；日志中记录原因但不在每轮弹窗。
- 窗口操作和子进程派生都通过 `run_on_main_thread` 投递；stdout/monitor 用普通线程（与 solidway
  一致），因为 GTK 只允许主线程操作窗口。

### 9. 托盘、偏好与关闭语义

托盘菜单：状态（disabled）、打开 Pi Star、重启 Pi Star、开机自启动（check）、关闭窗口时最小化
到托盘（check）、退出。状态变化同时更新菜单文本、托盘 tooltip 和 splash。

`desktop.toml`（位于 Tauri app data）：

```toml
schema = 1
autostart = false
close_to_tray = true
port = 30151
```

- 加载时缺失/缺字段/损坏 → 使用默认值并原子物化写回。
- 自启动默认关闭（桌面壳不主动注册登录项）；用户在托盘打开后调用 autostart 插件，失败则回滚
  勾选且不写文件。
- `close_to_tray = true` 时，`RunEvent::ExitRequested` 只在 `code.is_none()`（关最后一个窗口）
  时 `prevent_exit()`；托盘“退出”走 `app.exit(0)`，`code` 有值，必须放行。
- macOS 在窗口创建/销毁时切换 `Regular`/`Accessory` activation policy，并处理 Dock `Reopen`。

### 10. 主窗口不开放远程 IPC

v1 使用系统原生装饰，不做自定义标题栏。主窗口只获得一个初始化脚本注入的只读对象：

```js
Object.freeze({
  version: 1,
  desktop: true,
  platform: "linux",
  shell: "tauri",
});
```

不实现 `windowControl`，也不向页面授予任何可调用的 Tauri command（ACL 全部拒绝）。这样即使 30151
被本地恶意进程占用并伪造健康响应，页面也拿不到文件系统、shell 或窗口控制能力。

如果后续要做 frameless 标题栏，需要新开变更：把 capability 的 `remote.urls` 精确限定到
`http://127.0.0.1:30151/*`，只授予一个自定义 `window_action` 命令，并在 UI 里用
`data-tauri-drag-region` 和按钮区 `no-drag` 实现拖拽/双击最大化。

### 11. 桌面模式下的 PWA

`ui/src/lib/desktop.ts` 读取 `window.__PI_STAR__`，`PwaRegistration` 在 `desktop === true` 时
直接返回；`browser-notifications`/push 的 service worker 分支也不在桌面模式启用。普通浏览器没有
该对象，行为保持现状。注入脚本在文档脚本之前执行，因此首帧就能判断。

备选方案：靠 User-Agent 判断。WebView 的 UA 因平台/版本而异，且普通浏览器可伪造；显式注入标记
更可靠。

### 12. 打包：`bundle.resources` 携带独立二进制

沿用 solidway 的方式：`tauri.conf.json` 不声明资源，打包脚本在构建前把 release 的 `pi-star`
复制到 `app/pi-star`（Windows `app/pi-star.exe`），用临时 `--config` 注入
`bundle.resources`，打包后删除。这样普通 `cargo build`/`cargo test` 不需要二进制存在，安装包
又能把它放到资源目录。

| 平台 | 桌面主程序 | pi-star 安装位置 |
| --- | --- | --- |
| Linux deb | `PiStarApp` | `/usr/lib/PiStar/pi-star` |
| Linux AppImage | `PiStarApp` | `$APPDIR/usr/lib/PiStar/pi-star` |
| macOS | `PiStar.app` | `Contents/Resources/pi-star` |
| Windows | `PiStarApp.exe` | 安装目录 `pi-star.exe`（与壳同目录） |

Windows 主程序名必须与 `pi-star` 在忽略大小写后不同，否则同目录两个文件互相覆盖。Linux deb 声明
`libwebkit2gtk-4.1-0`、`libgtk-3-0`、`libayatana-appindicator3-1`；AppImage 自带 webkit。
图标从 `ui/public/icons/icon-512.png` 用 `cargo tauri icon` 生成。

### 13. 页面新窗口

主窗口注册 `on_new_window`，不依赖浏览器的 tab：

- 同一 origin（`http://127.0.0.1:<port>`）的 `window.open` → 创建新的 Tauri 窗口，使用
  `window_features(features)` 继承大小/位置与平台 webview 配置，注入同一个只读
  `__PI_STAR__`，不授予任何 capability。
- 其他 URL → 交给 `open`/`xdg-open`/`cmd start`，返回 `Deny`，不在应用内创建 webview。
- 新窗口同样注册 `on_new_window`，支持嵌套打开。
- 重启/失败时关闭所有 console 窗口（保留 splash/notice）。
- 健康检查读取失败/超时视为 `Unreachable` 并重试到启动期限，不再误判成“外部服务”而杀掉
  pi-star。

这样 Web 端的“完整历史 / 在新标签页打开”语义在桌面端变成独立原生窗口，且不影响主窗口。

### 13. 构建顺序与版本

```text
PI_STAR_VERSION=<workspace version> \
  (cd ui && npm ci && npm run build)
cargo build --release -p pi-star           # 内嵌 ui/dist
cp target/release/pi-star app/             # 打包脚本临时注入
cd app && cargo tauri build                # 壳 + 资源
```

构建脚本先校验 `app/Cargo.toml`、`app/tauri.conf.json` 与 workspace 版本一致；不一致立即失败。
UI 构建时把同一版本写入 `PI_STAR_VERSION`，避免界面显示的版本落后于二进制。
提供 `scripts/package-desktop.sh` 作为 Linux 当前平台的入口，Windows/macOS 脚本按同一模式扩展。
`app/target`、`app/gen` 和临时二进制加入根 `.gitignore`。

## Risks / Trade-offs

- **端口被占用** → 失败页明确提示端口冲突，并给出修改 `desktop.toml` 的路径；不自动换端口。
- **用户修改端口会重置 webview 状态**（localStorage/cookie/缓存）→ 在偏好说明和 `desktop.toml`
  注释中写明；默认端口保持 30151。
- **release 构建依赖 `ui/dist` 存在** → 构建脚本先构建 UI；缺失时 pi-star 启动失败而不是静默提供
  空页面。
- **安装包体积增加约 10 MB+**（UI 资源 + 两个二进制）→ 可接受；UI 已 gzip/长度优化，后续可评估
  压缩嵌入。
- **WebKitGTK/Wayland 差异** → 使用原生装饰，避免 frameless 的边缘 resize 与拖拽问题；Linux deb
  声明 WebKitGTK 依赖，AppImage 自带运行时。
- **父进程被 `kill -9` 时子进程可能短暂存活** → stdin EOF watchdog + 进程组/Job Object 双重兜底；
  watchdog 在 5 秒内完成清理。
- **未签名安装包触发 SmartScreen/Gatekeeper** → 本变更不解决签名，文档明确“未知发布者”提示；
  后续独立变更接入签名与公证。
- **`app/` 独立 workspace 与根 workspace 版本漂移** → 版本一致性校验在打包前硬失败。
- **splash 的 `__TAURI__` 与远程页面共享注入** → 主窗口无 capability；验收用例显式测试远程页面
  invoke 被拒。
- **UI 的 PWA 逻辑在桌面模式被绕过** → 只跳过 SW/安装提示，不影响普通浏览器；单测覆盖两条分支。

## Migration Plan

1. 先落 pi-star 侧：内嵌资源、事件协议、stdin watchdog。此阶段 `pi-star` 单独运行即可验证，
   浏览器行为不变；旧的 `--ui-dir ui/dist` 仍可用。
2. 落 `app/` 壳和 dev 流程：`cargo tauri dev` 使用仓库 `target/debug/pi-star` 与磁盘 UI。
3. 落桌面模式 UI 标记与 PWA 跳过；浏览器回归测试保持通过。
4. 落打包脚本与安装包；在干净 Linux 环境验证“只装桌面包即可运行”。
5. 文档更新 README 的“构建与运行”，新增桌面章节，说明 `desktop.toml`、日志位置和端口。

回滚策略：桌面形态是新增发布物。若桌面壳不可用，继续发布/使用独立的 `pi-star` 二进制和浏览器
访问即可；本变更不修改 `~/.pi/agent` 的数据格式，不需要数据迁移。

## Open Questions

<!-- 无。frameless 标题栏、原生通知、自动更新和签名/公证明确留作后续变更。 -->
