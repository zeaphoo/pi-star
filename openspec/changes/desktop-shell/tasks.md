# Tasks

## 1. pi-star 内嵌前端资源

- [x] 1.1 在 `crates/pi-star/Cargo.toml` 加入 `rust-embed`（启用 `interpolate-folder-path`），新增 `crates/pi-star/build.rs` 对 `../../ui/dist` 设置 `rerun-if-changed`，并确认 `cargo build -p pi-star` 在 UI 已构建/未构建两种情况下都能编译
- [x] 1.2 新增 `crates/pi-star/src/web_assets.rs`：实现 `resolve(override_dir)`、`Disk`/`Embedded` 两种来源、`describe()` 与文件计数；release 下内置文件数为 0 时返回启动错误，并用单元测试覆盖三条解析分支
- [x] 1.3 把 `Config.ui_dir` 改为 `Option<PathBuf>`，`--ui-dir`/`PI_STAR_UI_DIR` 改为显式覆盖；覆盖目录缺少 `index.html` 时进程以非零状态退出；更新所有受影响的 `Config::new` 调用与测试
- [x] 1.4 用自定义 fallback handler 替换 `ServeDir`/`ServeFile`：精确命中、SPA 回退、`/api` JSON 404、`Content-Type`/`Content-Length`、`HEAD`、`assets/*` 长缓存、`sw.js`/manifest 不缓存、`..` 路径穿越防护；为每种行为补单元/集成测试
- [x] 1.5 在 `/api/health` 增加 `assets { source, path?, files }`；测试 release 来源为 `embedded`、`--ui-dir` 来源为 `disk`
- [x] 1.6 更新 `README.md` 构建章节：release 需要先构建 `ui/dist`、`--ui-dir` 的新语义、单二进制可独立运行；按文档命令实际构建并启动一次验证

## 2. pi-star 桌面运行契约

- [x] 2.1 新增 `crates/pi-star/src/protocol.rs`：定义 `pi-star: ` 前缀、`listening`/`bind-failed`/`assets` 事件格式与 flush 输出；用单元测试固定字符串形状（含 `--port 0` 实际端口）
- [x] 2.2 把 tracing subscriber 改为写标准错误；用 `RUST_LOG=debug` 启动并断言标准输出只有 `pi-star:` 事件行、标准错误包含日志
- [x] 2.3 分类绑定错误（`address-in-use`、`permission-denied`、`other`），输出 `bind-failed` 后以退出码 2 结束；写一个集成测试先占用端口再启动二进制，断言事件与退出码
- [x] 2.4 新增 `--exit-on-stdin-eof`：tokio 任务读 stdin，EOF/错误时取消现有 shutdown token；集成测试启动子进程后关闭 stdin，断言进程在 5 秒内退出且端口释放；不传参数时断言服务继续运行
- [x] 2.5 为 `--port 0` 增加集成测试：解析 `listening addr=` 得到实际端口，GET `/api/health` 与 `/` 均成功
- [x] 2.6 更新 `docs/status.md`/`README.md`，记录 stdout 事件契约、日志去向、退出码和 `--exit-on-stdin-eof`；按文档用管道读取一次事件并核对

## 3. Tauri 壳骨架与窗口

- [x] 3.1 在根 `Cargo.toml` 加 `exclude = ["app"]`，并在根 `.gitignore` 忽略 `app/target`、`app/gen`、`app/pi-star`、`app/pi-star.exe`；创建 `app/Cargo.toml`（独立 `[workspace]`）、`app/build.rs`、`app/tauri.conf.json`、`app/capabilities/default.json`、`app/ui/index.html` 和 `app/icons/`；`main.rs` 在 release 使用 `windows_subsystem = "windows"`；`cd app && cargo build` 通过
- [x] 3.2 配置 `tauri.conf.json`：`productName = "PiStar"`、`identifier = "com.pistar.desktop"`、`mainBinaryName = "PiStarApp"`、`frontendDist = "ui"`、`withGlobalTauri = true`、`app.windows = []`、资源由打包脚本注入；用 `cargo tauri build --no-bundle` 验证配置可解析
- [x] 3.3 实现 `app/src/config.rs`：解析 app data 目录、`--hidden`、稳定端口与 `PI_STAR_DESKTOP_URL`（仅 debug）；定位 `pi-star` 的顺序为 exe 同目录 → resource dir →（仅 debug）仓库 target →（仅 debug）PATH；用 `locate_in` 纯函数单测覆盖去重、资源目录、开发路径和 PATH 忽略
- [x] 3.4 实现 `app/src/prefs.rs`：`desktop.toml` 的加载/默认值/损坏回退/原子保存；单测覆盖缺失、缺字段、损坏、往返读写
- [x] 3.5 实现 `app/src/window.rs`：splash 创建与状态注入、主窗口 hidden 创建/`PageLoadEvent::Finished` 后显示、共享 20 秒启动期限、配置失败时仍显示 fatal splash；手动启动验证 splash 和错误页
- [x] 3.6 实现 `app/src/commands.rs` 的 `desktop_action("retry"|"quit"|"open-log")`，在 `build.rs` 用 `AppManifest::commands` 生成 ACL，capability 仅授权 `splash` 窗口；手动验证 splash 按钮可用、主窗口调用被拒绝

## 4. 监督、托盘、偏好与清理

- [x] 4.1 实现 `app/src/supervisor.rs` 的 spawn：传 `--hostname 127.0.0.1 --port <prefs.port> --exit-on-stdin-eof`，stdin piped、stdout 读取事件、stderr 写 `<app_data>/pi-star.log`（启动时 >5 MB 轮转）；健康请求在 `PI_WEB_PASSWORD` 存在时带 Basic Auth；单测覆盖事件解析、健康响应归属校验和密码不落日志
- [x] 4.2 实现监控线程与状态机：generation 隔离、20 秒启动期限、健康确认、意外退出按 1/2/4/8/16 秒退避最多 5 次、bind/spawn 失败不重启、手动重试清零；用假子进程/单元测试覆盖状态转移和 `open_action` 分支
- [ ] 4.3 实现托盘：状态、打开、重启、开机自启动、关窗到托盘、退出；状态同步托盘文本/tooltip/splash；手动验证每个菜单项和状态文案
- [ ] 4.4 接通 `tauri-plugin-autostart` 与 `--hidden`：默认关闭、托盘勾选后注册、失败回滚勾选且不写偏好；手动验证登录项注册/注销与静默启动
- [x] 4.5 实现退出清理：Unix `setsid` + 进程组 SIGTERM→5 秒→SIGKILL，Windows Job Object + `CREATE_NO_WINDOW`；安装信号处理器；集成测试退出后 `pgrep` 无 pi-star/子进程且端口可再次绑定
- [ ] 4.6 接入 `tauri-plugin-single-instance`（第二个实例聚焦主窗口）、`ExitRequested` 的 `close_to_tray` 分支、macOS Dock policy 与 `Reopen`；手动验证单实例、关窗留托盘和显式退出

## 5. Web UI 桌面适配

- [x] 5.1 新增 `ui/src/lib/desktop.ts` 读取只读 `window.__PI_STAR__`，提供 `isDesktop`/`platform`；补单元测试覆盖对象缺失、字段缺失与正常桌面对象
- [x] 5.2 `PwaRegistration` 在桌面模式直接返回；检查 `push-client`/`browser-notifications` 的 service worker 分支在桌面模式不被启用；补测试并确认普通浏览器分支不变
- [x] 5.3 在桌面壳中通过 `initialization_script` 注入 `window.__PI_STAR__ = Object.freeze({ version, desktop: true, platform, shell: "tauri" })`；手动验证主窗口存在标记、普通浏览器不存在
- [x] 5.4 跑 `cd ui && npm run typecheck && npm test && npm run build`，确认 1648 个客户端测试和构建全部通过

## 6. 打包、版本与文档

- [x] 6.1 新增 `scripts/package-desktop.sh`：校验版本 → 用 `PI_STAR_VERSION=<version>` 构建 UI → `cargo build --release -p pi-star` → 复制二进制到 `app/` → 临时 `--config` 注入 `bundle.resources` → `cargo tauri build` → 清理临时文件；在 Linux 上实际产出 deb/AppImage 并列出包内两个二进制
- [ ] 6.2 配置 Windows/macOS：Windows `mainBinaryName = PiStarApp`、NSIS/WiX 语言与 `webviewInstallMode`、资源注入；macOS `.app` 资源布局与最低系统版本；用 `cargo tauri build --config` 的 dry-run/配置解析验证，并在可用平台实打包一次
- [x] 6.3 新增版本一致性检查（脚本或 `xtask`）：`app/Cargo.toml`、`app/tauri.conf.json`、workspace 版本不一致时失败；构造一次不一致版本验证失败信息
- [x] 6.4 用 `cargo tauri icon ui/public/icons/icon-512.png` 生成 `app/icons/` 全套图标，并在 Linux 上确认托盘与窗口图标加载
- [x] 6.5 更新 `README.md`/`docs/status.md`：桌面构建、运行、日志路径、`desktop.toml`、端口策略和未签名安装包提示；按文档从干净目录执行一次构建

## 7. 集成验收

- [x] 7.1 跑通桌面开发链路：`cargo build -p pi-star` + `cd ui && npm run build` + `cd app && cargo tauri dev`；确认 splash→主窗口、关窗到托盘、托盘重开
- [ ] 7.2 验证失败矩阵：缺少二进制、无执行权限、端口被非 pi-star 占用、端口被 pi-star 占用、子进程启动后崩溃、20 秒超时；每类都显示正确原因和日志末尾
- [ ] 7.3 验证权限边界：主窗口调用 `desktop_action` 被 ACL 拒绝；splash 重试/退出/打开日志可用；普通浏览器无 `__PI_STAR__`
- [x] 7.4 验证清理与恢复：托盘退出、SIGTERM 桌面壳、`kill -9` 桌面壳三种情况下无孤儿进程、端口释放、下次启动成功
- [ ] 7.5 在干净环境安装桌面包并断网启动，确认内嵌 UI、会话读取、Agent 页面可用；再单独运行安装目录中的 `pi-star` 并用浏览器访问
- [ ] 7.6 跑完整回归：`cargo test --offline`（workspace）、`cd app && cargo test`、`cd ui && npm test && npm run typecheck`；汇总桌面验收矩阵结果到 `docs/status.md`

## Workflow follow-up

- 在实现与验收完成后归档 `desktop-shell` change，并把 `desktop-runtime`、`desktop-shell` 的
  delta 合入主 specs。
- 后续独立变更再评估 frameless 标题栏、原生通知、自动更新和签名/公证。
