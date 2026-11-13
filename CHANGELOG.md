# Changelog

本项目的所有重要变更记录在此文件。

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。发布时由 `scripts/release-notes.sh` 抽取对应
版本段落作为 GitHub Release 正文；发布流水线会校验该段落存在。

## [Unreleased]

### Added

- **轨迹视图**：在「对话」右侧新增与会话绑定的「轨迹」标签，把 session 的追加日志压成
  单行事件列表（用户 / 助手 / 思考 / 工具 / 压缩 / 模型切换 / 上下文编辑…），点开一行在
  右侧「概述 / 参数 / 结果 / Schema / 计时」面板里看详情。右侧竖向三轨（输入 / 模型 /
  工具）是整条会话的方位图，点轨道跳到对应行；最左 4px 按真实时间分布，长等待一眼可见。
  列表支持「时长 / 轮次 / 调用 / 子调用」过滤与搜索，键盘 ↑↓/PageUp/PageDown/Home/End
  导航。数据来自新增的 `GET /api/sessions/{id}/entries`（原始条目，不投影、不丢
  `model_change` 这类事件）和 `GET /api/sessions/{id}/entries/{entry_id}`（单条完整内容）。
  列表页按 `previewChars` 截断文本与工具 `details`，23.5 MB 的会话加载 1.2 MB。
- **轨迹视图记忆位置**：按会话记住滚动到的行、选中的事件和详情页签，切到「对话」或切到
  别的会话再回来都回到原处；四个过滤开关与详情面板宽度按浏览器记住（`pi-star:trace-view:v1`）。
  定位用「行 key + 偏移」而不是像素，日志增长或过滤变化后仍落在同一行。
- **轨迹事件配色**：用户 / 助手 / 思考 / 工具 / 配置变更各自一色，列表徽章与右侧轨道
  mark 共用同一组变量（改主题时自动跟随），一眼可分；错误无论密度多高都保持高亮。
  轨道按 2px 槽位聚合，mark 高度跟随间距，700 个事件也不再糊成一条实心色带。

### Fixed

- **轨迹视图在会话刚建立时一直空着**：新建对话后切到「轨迹」会停在「这个会话还没有任何
  事件」，只有切到别的会话再切回来才有内容。视图原先只在「会话切换」和「运行中轮询」两条
  路径上重新加载，而首条记录写入时这轮运行可能已经结束，于是没有任何人再去问一次。现在按
  会话的 `modified` + 消息数组成 revision，变化就补齐一次尾部分页；切回标签页时也会补。
  同一处还修掉一个竞态：加载被新的会话切换中止后，重试会被「已有请求在飞」的闸门直接丢弃，
  于是列表永远空着 —— 现在改成代际标记，新的加载直接接管，旧响应一律丢弃。
  新增 `ui/e2e/trace.mjs`：向会话文件追加两行后断言轨迹自行补上（撤销修复时该用例失败）。

### Removed

- **「完整历史」按钮**：这个按钮只在新的标签页里调用 `/api/sessions/{id}/export` 打开 pi
  生成的 HTML，效果不好（自带一套与界面无关的主题，大会话 8 MB 起，失败时回退到无样式的
  纯文本）。聊天本身已经能翻到第一条记录，轨迹视图也能看全部条目，所以它就不再保留，连同
  后端路由、`pi --export` 子进程调用、Rust 回退模板和 `history.full` / `history.unsaved` /
  `history.label` 三组文案一起删除；需要时从 `752b4cd` 取回。

## [0.1.0-rc.2] - 2026-11-11

第二个预发布版本：代码、Diff 和终端改为项目级工作区，右侧工作台和终端恢复流程成型。

### Added

- **项目级工作区**：Code、Diff（原 Git）和 Terminal 按项目保存各自的标签页、布局与终端；
  切换项目恢复对应工作区，切换标签或项目时接回仍然存活的 PTY。
- **右侧工作台**：用项目 dock 取代原文件面板，Code/Diff/Terminal 可以拆分并选择方向与
  内容；会话上下文设置移入 Settings。
- **终端多标签与递归分屏**：支持项目级 terminal tabs、pane 拆分和比例调整；新 tab 从当前
  pane 的 cwd 打开。
- **终端恢复**：应用或服务重启后按最后保存的 cwd 创建新 shell；目录失效时回退项目根目录，
  并提示恢复位置。Linux 通过 `/proc/<pid>/cwd` 跟踪实时目录，同时支持 shell 的 OSC 7 上报。
- **侧边栏与会话视图**：新增 New 菜单和会话 artifacts 面板；Code/Diff/Terminal 成为项目视图。
- **pi-web 0.15 客户端能力与 Models API**：同步上游客户端功能与模型列表接口。

### Changed

- **终端默认目录**：从当前会话的 cwd 改为项目根目录，避免终端跟随会话落到其他项目；已有
  pane 继续使用并恢复自己的 cwd。
- **工作台拆分流程**：顶部收敛为一个“拆分窗格”按钮，旁边箭头显式选择方向；新窗格先选
  视图，选定后不再切换。
- **Code 空窗格**：没有打开文件时自动展开文件树。
- **Git 视图改名 Diff**；项目操作收进侧边栏 New 菜单；移除侧边栏文件浏览器和 Web Push。
- **会话上下文设置**：系统提示词与工具定义移入 Settings › 会话上下文，并去掉无意义的工具
  注册数量。
- **Agent 扩展改为 JavaScript**：`agent-extension/index.ts`、`subagent.ts` 由 `tsc` 生成等价的
  `index.js`、`subagent.js`（ES2022，无类型注解、注释保留），并加 `package.json` 声明 ESM；
  Rust 的扩展探测、打包校验脚本与文档同步改为 `index.js`，仍兼容 `.ts` 回退。
- **桌面应用**：bundle identifier 更新为 `net.quadhub.pi-star`。

### Fixed

- **Vite 开发代理的请求被判定 Untrusted origin**：`/api` 代理原来用字符串 target，Vite 会
  自动 `changeOrigin: true`，把 `Host` 改写成 30151 而页面 `Origin` 仍是 30152，后端的
  同源检查因此拒绝所有 POST（输入消息失败、新会话提示 Untrusted origin）。改为对象形式并
  显式 `changeOrigin: false`，保留页面自己的 Host。需要重启 `npm run dev`。

- **模型选择器只列可用模型**：`/api/models` 现在只返回有凭据的 provider（`auth.json`、
  models.json 的 `apiKey`，或 `<PROVIDER>_API_KEY` 环境变量），不再把 `models-store.json`
  里未配置的目录 provider 列出来；设置里的「模型定义」和输入框选择器因此一致。
  `?scope=all` 仍表示忽略 `enabledModels`，但同样只含可用模型。

- **pi-web 侧聊不再出现在侧栏**：识别 `pi-web:side` 元数据，把侧聊折叠进父会话的 family，
  pi-web 创建的 `Side chat` 不再被当成顶层 fork 显示；侧聊用量只算自身条目、内容搜索跳过，
  删除父会话时一并删除。会话索引版本升到 2，旧缓存里的 fork 关系会重新解析。

- **对话右侧 minimap 触底**：小地图不再只覆盖消息区，而是作为整列浮层延伸到页面底部
  （输入框右侧原本就预留了 36px），锁图标和轨道都到页面底端；扩展状态栏同步留出右侧空间。

- **终端不再卡在 `Terminal expired or closed`**：404 自动按保存的 cwd 重建，不再把死 id
  当错误展示；切换标签或项目也不会重复启动 shell。

- **Code 文件标签关闭后被重新打开**：FileViewer 的卸载保存只写回仍然存在的标签页，清空
  Code 窗格后也不会被迟到的保存恢复。

- **终端切换目录**：实时跟踪 shell cwd，重启后回到最后目录；保存目录不存在时回退项目根目录。

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
  `pi vpi-compatible-rust`；最窄侧栏的版本浮层用 9px，容纳 `0.1.0-rc.1p1.4.0`。
- **失效工作区**：启动时记住的新会话目录失效不再卡在“无法打开工作区”；自动丢弃记忆并回到
  默认工作区，显式 `?cwd=` 错误页提供重试与选择项目。
- **macOS 公证**：`stapler` 取票遇到 Apple CloudKit 瞬时网络错误（Error 65/68、NSURLError
  -1005）时自动重试 4 次、间隔 20 秒，不再让已通过公证的 dmg 因取票抖动失败。
- **Release 构建**：Windows 下用 PowerShell 解压 Node/MinGit 的 zip，并给 GNU tar 加
  `--force-local`，避免 `D:` 被当成远程主机；pi 依赖安装加 `--legacy-peer-deps`，规避 npm 10
  的 `edgesOut` 崩溃；Windows 运行时 staging 放在仓库同盘的 `app/pi-runtime`，避免 Tauri
  规范化资源路径时丢掉盘符（`C:\pirt` → `\pirt`）。NSIS 内容断言会先把 7-Zip 输出的
  反斜杠归一化，避免把已打包的 `pi-runtime\node.exe` 误判为缺失。Windows artifact 只上传
  安装包和便携 zip，便携 staging 放到临时目录；发布前清理同名 Release 的旧 assets，避免把
  `pi-runtime` 内数百个文件逐个上传导致 GitHub API 超时。
- 桌面壳被 `SIGKILL` 时通过 stdin EOF 与进程组/Job Object 清理子进程树，避免孤儿进程。
- 桌面模式跳过 service worker/PWA 注册，避免 Tauri webview 的缓存行为干扰本地服务。
- Windows 安装包启用 long paths，避免内置 `node_modules` 路径超限。

[Unreleased]: https://github.com/zeaphoo/pi-star/compare/v0.1.0-rc.2...HEAD
[0.1.0-rc.2]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.2
[0.1.0-rc.1]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.1
