# Changelog

本项目的所有重要变更记录在此文件。

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。发布时由 `scripts/release-notes.sh` 抽取对应
版本段落作为 GitHub Release 正文；发布流水线会校验该段落存在。

## [Unreleased]

## [0.1.0-rc.5] - 2026-11-25

### Added

- **子代理运行时（QuickPI）**：`subagent`、`get_subagent_result`、`steer_subagent` 三个工具
  由 Rust 的 `SubagentController` 驱动，支持 profile、`max_turns`、并发队列、后台运行、
  孤儿状态收敛，以及把后台报告以 steer 送进父代理当前 run（空闲时改为 follow-up 唤醒）。

- **子代理运行时（RPC）**：Node 扩展补齐同样的能力：profile（model/thinking/tools/max_turns）、
  并发队列、子会话 `pi-star:subagent`/`status`/`result` 元数据、turn limit、结果消费去重、
  “不是用户消息”的报告前缀；后台报告在父代理 `turn_end` 处以 steer 投递。

- **子代理轨迹**：右侧子代理面板新增“对话 / 轨迹”切换，轨迹复用主 Trace 的事件模型，
  只读 `/entries`，不会启动或干扰子代理 run。

- **子代理开关生效**：内置子代理默认关闭，只有打开 Settings › Sub-agents 的开关并重载
  会话后才注册 `Agent`/`subagent` 工具；QuickPI 的 reload 现在会重建运行时。工具在注册时
  即启用，模型不需要先手动开启。

- **子代理随父代理整体停止**：没有独立控制、没有 resume、不做 worktree 隔离；父代理 Stop
  会同时停止它的子代理，且停止的运行不再回送完成报告。

- **子代理并发按父会话隔离**：`maxConcurrent` 现在按父会话计数，一个项目排队不会阻塞另一个；
  RPC 的后台队列会先创建子会话并写入 `queued` 状态，重启后显示为 `interrupted` 而不是消失。

- **子代理 profile 遵循项目信任**：项目/工作区 profile 只在受信任目录加载（Settings 面板
  仍然显示文件），与 pi 对 `.pi/` 资源的规则一致；`run_in_background` 在调用未指定时
  使用 profile 的默认值。

- **子代理 profile 资源**：`prompt_mode`（replace/append）、`load_skills`/`skills:` 预加载、
  `mcp` 开关和 `inherit_context` 现在两套引擎都会应用；RPC 还支持 `load_extensions`、
  路径或 `builtin:` 形式的 `extensions:` 列表，以及 `codemode: on/inherit`。
  QuickPI 不执行 TypeScript 扩展、也没有 Code mode，遇到要求它们的 profile 会明确报错，
  而不是静默忽略。

- **修复子代理关系识别**：会话文件中先出现其他 custom entry（例如后端选择）时，侧栏仍能
  把它识别为 subagent 而不是 fork。

- **未发送草稿跨刷新保留**：会话草稿写入 `pi-star:draft:<session id>`（停止输入 300ms 后写，
  `pagehide`/隐藏时立即写，清空或发送立即删除），刷新或被 WebView 重载后恢复文本；图片、
  超过 20,000 字符的文本和新建会话的临时 key 只留内存，最多保留最近 50 条。历史编辑的待发送
  文本和正在运行的内置命令会暂时不落盘，避免刷新后语义错乱；退出登录时清除全部已存草稿。

- **单文件上传上限放到 100MB**：取消 25MB 的单文件限制，请求总量仍为 100MB；上传写入继续在
  阻塞线程池中执行，不会卡住实时流和其他请求。

- **项目子进程不再继承宿主密钥**：git status/diff/worktree、npm 安装与 `npm view`、git clone、
  Skills 安装以及 QuickPI 的 shell 都会去掉 `PI_WEB_PASSWORD`、provider API Key、GitHub/npm
  Token、云厂商密钥，以及 `PORT`/`NODE_ENV`/`NEXT_*` 等宿主运行时变量。项目里的
  `core.fsmonitor`、diff driver、worktree hook 和 npm 生命周期脚本因此无法再读到这些凭据。

- **会话搜索支持多词与引号**：空格/全角空格分隔的每个词都要命中，双引号保留短语；
  结果带项目显示名；搜索文本按文件 size/mtime 缓存，未变化的会话不再每次按键重读。
  Unicode 大小写与截取不再按字节切分，避免 `İ` 等字符导致切片错误。

- **目录丢失时 Fork 到其他文件夹**：missing-folder 横幅新增“分叉到其他文件夹…”，
  经目录选择器把整份会话复制到新 cwd，更新 header 的 cwd/parentSession、加入文件访问
  根并保留 composer 草稿；拒绝目标无效、源目录仍存在、子代理和空会话。

- **运行中的 New session**：user `message_end` 后由事件流补发 `user_message_entry`，
  运行中已发送的用户消息也能立刻使用 New session；历史编辑仍只在运行结束后可用。

- **长会话选择性能**：聊天行离开滚动容器约一个视口后使用 `content-visibility: auto`
  跳过布局（保留 DOM 与可选性，滚动高度不跳）；引用选择弹窗的状态从 ChatWindow 拆出，
  选择/取消选择不再重渲染整条 transcript。

### Changed

- **内置 pi 升级到 1.6.0**：`packaging/pi-runtime.json` 的 `pi` 由 1.4.0 改为 1.6.0，Node 仍为
  22.20.0，重新暂存 `pi-runtime` 即可。已在 1.6.0 下验证宿主扩展的 `get_tools`/`set_tools`/
  `get_system_prompt`/`get_context_usage`/`reload`，以及 `get_state`/`get_messages`/
  `get_entries`/`get_session_stats`/`set_session_name` 等 RPC 命令和既有 v3 会话的读取。
  `scripts/prepare-pi-runtime.mjs` 同时修复了 `--node` 指向输出目录内文件时
  “src and dest cannot be the same”的报错。

### Fixed

- **UTF-16 文本预览**：带 BOM 的 UTF-16LE/BE 文件按编码解码，分块读取不再拆开代理对。

- **文件字节范围**：拒绝 `bytes=-` 这类没有任何边界的请求，支持 `bytes=-N` 后缀范围，空文件
  不再返回 416。

- **CSV 空记录**：单个带引号的空字段 `""` 不再被当成空行丢弃。

- **文件类型判定**：MIME 查找只认明确声明的扩展名，`constructor`、`__proto__` 等名字不再被
  误判为图片/音频/视频。

- **Markdown 流式渲染**：链接引用/脚注定义行会让整条回复作为一个块渲染；带尾随文本的
  fence 行不再提前关闭代码块，代码块内的 `$$` 示例保持原样。

- **对话中的终端转义**：模型写入正文或思考里的 ANSI 转义不再显示为 `[38;5;5m` 等原始文本，
  复制时也只复制可见内容。

- **工具结果提前显示**：`tool_execution_end` 带回的结果直接留在工具卡片上，等规范
  tool-result 消息到达后再替换，避免卡片在两者之间短暂空白。

- **终端回放不再向 shell 发送应答**：重连或恢复标签页时，xterm 对回放历史中光标/设备
  查询的应答不会再被当成输入；新建 shell 的首次连接仍会正常应答启动查询。

- **输入法 Enter 不再误触发操作**：填写 API Key、Skills 搜索和单行设置输入时，用于确认
  候选词的 Enter 不再保存/搜索/提交；同时识别 Safari 的 `keyCode === 229`。

- **Markdown 文件链接**：`path:12-34` 形式的行范围链接能正确解析到文件。

- **内置运行时的子代理可以启动了**：`agent-extension/subagent.js` 之前用 `PI_STAR_PI_CLI || "pi"`
  从 PATH 起子代理；安装包只带 `pi-runtime/node` + `pi/dist/bundle/cli.js`，没有 `pi` 命令，
  干净机器上会 `spawn pi ENOENT`。现在按 `PI_STAR_PI_CLI` → `PI_STAR_PI_NODE`+`PI_STAR_PI_JS`
  → 父进程的 `execPath`+`argv[1]` → `"pi"` 依次解析，并新增
  `agent-extension/subagent.test.mjs` 与 CI 步骤。

## [0.1.0-rc.4] - 2026-11-21

第四个预发布版本：单连接实时流、请求超时保护，以及原生 PI 引擎的上下文占用显示。

### Fixed

- **原生 PI 引擎显示上下文占用**：`pi --mode rpc` 的 `get_state` 不返回 context usage，
  顶栏因此缺少上下文百分比与窗口大小。新增 `get_context_usage` 宿主扩展命令，状态查询与
  切换模型时刷新，原生 PI 和 QuickPI 一样显示当前会话的上下文比例（与 pi-web 一致）。

### Changed

- **实时流改为单连接多路复用（stream bus）**：Agent 事件、终端输出、文件 watch 与登录流程不再
  各开一条 SSE，而是共用一条 `/api/stream` WebSocket，并由 SharedWorker 在标签页之间共享；
  浏览器因此每个 profile 只持有一条长连接，不再受 HTTP/1.1 每源 6 连接的限制。频道带游标
  （终端按字节偏移、文件按版本、Agent 按序号）可断线续传；终端输入与 Agent 命令也走同一连接。
  旧的 `*/events` SSE 端点保留一个发布周期并标记为 deprecated。
- **缓冲按负载伸缩**：每个频道的回放缓冲从 1 MiB 起按水位扩容（终端上限 20 MiB），空闲后回收；
  全 Hub 默认 256 MiB 预算，超出时按 terminal → file → agent 顺序淘汰旧帧。可用
  `PI_STAR_STREAM_RING_*` 覆盖。
- **普通请求有超时**：新增统一请求策略，未单独设置超时的请求默认 60 秒失败，后台请求并发受限，
  不再因连接池被占满而无限等待。

- **许可证改为 Apache-2.0**：项目整体以 Apache License 2.0 发布；从 pi-web 与 pi-rust 复制的代码
  继续保持 MIT，上游署名与 MIT 全文保留在 `NOTICE`。
- **README 重新定位**：突出跨平台桌面一体化（Agent / 代码浏览器 / Git diff / 终端）与两个
  Agent 引擎（原生 PI、Rust PI Agent）。
- **README 与开发文档拆分**：README 只保留产品特性与下载安装，并改为英文默认
  （[`README.md`](README.md)），中文版为 [`README.zh-CN.md`](README.zh-CN.md)；架构、构建、
  开发测试、桌面打包与安全模型移到 [`docs/development.md`](docs/development.md)
  （[中文](docs/development.zh-CN.md)）。其余文档同样改为英文默认并提供中文版本。

## [0.1.0-rc.3] - 2026-11-13

第三个预发布版本：终端外观与内置字体、设置分区，以及终端恢复和交互细节。

### Added

- **终端外观**：主题改为预设库（Pi、One Dark、Dracula、Nord、Gruvbox、Solarized、
  GitHub Light、Catppuccin、Tokyo Night），可跟随应用明暗；新增终端字体、字重、字号、
  行高、光标、字间距、最小对比度和回滚行数设置，修改不会重建 PTY。
- **内置终端字体**：打包 Noto Sans Mono、完整 Noto Sans Mono CJK SC，以及
  JetBrainsMono/Hack Nerd Font（Regular + Bold），CJK 与 Nerd 图标开箱可用。
- **设置分区**：General 左侧栏新增「终端」和「代码」独立视图；「代码」收拢语法高亮与
  文件图标，版本信息固定在 sidebar 底部。
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

### Changed

- **终端操作位置**：重启/关闭 pane 的按钮从工具栏最右侧移到 pane 右上角，避免被误读
  为关闭全部窗口。
- **聊天字号**：滑块改名为「聊天字体大小」；终端跟随聊天时明确显示终端比聊天小 1px。
- **工具栏分隔线**：顶部左右分组和终端 Tab/分屏操作区的竖线固定在容器上，不再随按钮
  增减移动。

### Fixed

- **终端 split 恢复**：两个 pane 同时丢失 PTY 时只恢复一个；现在逐个恢复，并移除多余
  的恢复提示条。
- **`exit` 关闭终端**：shell 以退出码 0 结束时关闭对应 pane，最后一个 pane 连带关闭
  Tab；非 0 退出保留现场。
- **提交信息区域**：长提交信息默认限制高度，可拖拽调整（双击或方向键复位），不再把
  diff 挤出屏幕。
- **英文设置导航**：section tab 按标签自适应宽度，英文标签不再出现横向滚动条。
- **会话行 Fork 在有活动运行时失败**：会话一旦有运行中的后端，Fork 会返回 500「Invalid
  entry ID for forking」。RPC 分支在没给 entry 时拿叶子节点去 fork，而 pi 的 fork 是「在这
  条用户消息之前分叉」，叶子通常是助手的回复，于是必然被拒。现在没有 entry 时改用 pi 的
  `clone`（复制当前分支），与文件后端的做法一致：整条分支都被复制，`parentSessionId` 与
  `relation` 照旧带上。
- **会话读取不再假装有 wrapper 快照**：`GET /sessions/{id}` 曾为活动 wrapper 返回缓存快照、
  并要求 `?force=1` 才采纳文件。但 pi-star 的 harness 本来就是写穿到 v3 文件的，`/context`、
  `/entries`、`/stats` 全都直接读文件，这条路由是唯一不一致的地方，`?force=1` 也没有调用者。
  现在一律读文件，`wrapperRebuilt` 与空的 `replace_entries` 一并删除。
- **e2e 套件恢复可跑**：`resolveServerBinary` 原先固定优先 `target/release`，一个几周前的
  release 二进制会盖住刚构建的 debug，于是整套用例在测旧的、没有 `/entries` 路由的程序
  （表现成 404）。改为取两者中较新的一个并打印所用路径。另外三处陈旧断言跟上产品现状：
  provider 列表按显示名匹配（「Anthropic」而非 id）、会话读取改为断言「文件即真相」、
  归档入口改为点列表下方的固定条而不是已删除的 `footer-archived` 行；键盘 Fork 用例顺带
  被上面的 Fork 修复救活。整套 36 项 PASS。
- **轨迹视图在会话刚建立时一直空着**：新建对话后切到「轨迹」会停在「这个会话还没有任何
  事件」，只有切到别的会话再切回来才有内容。视图原先只在「会话切换」和「运行中轮询」两条
  路径上重新加载，而首条记录写入时这轮运行可能已经结束，于是没有任何人再去问一次。现在按
  会话的 `modified` + 消息数组成 revision，变化就补齐一次尾部分页；切回标签页时也会补。
  同一处还修掉一个竞态：加载被新的会话切换中止后，重试会被「已有请求在飞」的闸门直接丢弃，
  于是列表永远空着 —— 现在改成代际标记，新的加载直接接管，旧响应一律丢弃。
  新增 `ui/e2e/trace.mjs`：向会话文件追加两行后断言轨迹自行补上（撤销修复时该用例失败）。
- **「已归档」入口整个消失**：这个入口原本是列表最末的一行，只在有归档会话时才存在，没有
  归档就完全不显示，看起来像功能没了。现在改成列表下方的固定条：不随列表滚动，没归档也一直
  在，归档后才带上数量；归档视图自己的标题也不再显示「· 0」。

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

[Unreleased]: https://github.com/zeaphoo/pi-star/compare/v0.1.0-rc.5...HEAD
[0.1.0-rc.5]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.5
[0.1.0-rc.4]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.4
[0.1.0-rc.3]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.3
[0.1.0-rc.2]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.2
[0.1.0-rc.1]: https://github.com/zeaphoo/pi-star/releases/tag/v0.1.0-rc.1
