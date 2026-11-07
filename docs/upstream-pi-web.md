# pi-web 0.13 → 0.15：可迁移功能对比

本文对比 pi-web `v0.13.0`（pi-star 的 UI 基线）与 `v0.15.0`，标出哪些变化可以直接搬到
pi-star、哪些需要 Rust 后端配套、哪些不适用。

## 基线确认

- pi-star 的 `ui/src` 与「用 `scripts/sync-ui-from-pi-web.mjs` 从 pi-web `v0.13.0` 同步出的
  结果」逐字节一致，另有 44 处本地差异：37 个文件改动（品牌/版本/i18n、后端选择、桌面与
  测试适配等）和 7 个本地新增（`main.tsx`、`shims/`、`types/`、`vite-env.d.ts`、
  `components/AgentBackendSelector.tsx`、`lib/desktop.ts` 及其测试）。
- 0.13 的客户端可达模块 231 个；0.15 是 235 个：新增 `SessionChatPanel.tsx`、
  `side-tab-state.ts`、`side-chat-client.ts`、`favicon-badge.ts`、`html-sandbox-polyfill.ts`、
  `panel-tab-order.ts`，删除 `SubagentViewer.tsx`、`subagent-viewer-state.ts`。
- 0.15 同时把依赖从 pi 1.4.0 升到 **pi 1.5.0**（Next 16.3.8 → 16.4.4，与 pi-star 无关）。
  pi-star 打包的是 pi 1.4.0（`packaging/pi-runtime.json`），见文末「pi 1.5.0」一节。
- 复核方式：把 sync 脚本复制到临时目录，用 `PI_WEB_DIR` 指向 `v0.13.0` / `v0.15.0` 的
  worktree 各跑一次，再 `diff -rq`。结论不依赖上游 git 历史。

## 结论概览

| 功能 | 上游 | 类型 | 建议 |
| --- | --- | --- | --- |
| 右侧标签拖拽排序 | `5e0e47f` #1511 | 纯前端 | 直接搬 |
| 标签页 favicon 未读徽标 | `0b1eeff` #1484 | 纯前端 | 直接搬 |
| 引用只插入所选文本（blockquote） | `6d0946b` #1502 | 纯前端 | 直接搬（「侧聊提问」随侧聊一起） |
| Git changes 列表/树视图 + 记忆 | `2251c3f` #1490 | 纯前端 | 直接搬 |
| 文件预览：散文默认换行 | `f07f368` #1471 | 纯前端 | 直接搬 |
| HTML 沙箱预览的 storage shim | `5269cb0` #1494 | 纯前端 | 直接搬 |
| 手机端触控尺寸 / Agents 按钮 | `57078bb` #1506 | 纯前端 | 直接搬 |
| 手机 Settings 逐级导航 + 系统 Back | `9afc508` #1510 · `1420b81` · `1615347` | 纯前端 | 直接搬 |
| 模型面板拖动 / Alt+方向 / A–Z 排序 | `6386fa4` | 纯前端 | 直接搬 |
| 自定义消息保持 TUI 文本布局 | `b25fa0a` | 纯前端 | 直接搬 |
| 扩展 UI ANSI 调色板 | `da6d538` | 纯前端一半 | 搬客户端一半 |
| 流式 token / t/s 指示稳定显示 | `79425f8` #1520 | 纯前端 | 直接搬 |
| 窄屏消息操作按钮 / composer 图标化 | `cb3cebe` #1487 · `3bad742` | 纯前端 | 直接搬 |
| 粘贴丢失修复（Chrome） | `8d2fb36` #1517 | 纯前端 | 直接搬 |
| 通知去重（页面 + push 只响一次） | `5f31fb6` #1518 | 前端 + Rust push | 需同步改 sw/Rust |
| PWA manifest 带凭据 | `b54a63d` #1477 | 前端 + Rust 路由 | 直接搬（Vite 版改法不同） |
| 模型选择器 scoped/all 切换 | `14c0c94` #1503 | 前端 + `/api/models` | 需补后端字段 |
| provider 显示名而不是 id | `f6c8699` #1514 | 前端 + `/api/models`、`/api/auth/providers` | 需补后端字段 |
| 已登出 provider 的 enabledModels 计入未匹配 | `7977daf` #1529 | 前端 lib + enabled-models 路由 | 需补后端 |
| 运行中切换模型 | `7a815d1` #1527 | 前端 + agent 路由 | 小改，需验证 RPC |
| Windows 终端优先 PowerShell | `fff941a` #1508 | Rust 终端 | 小改 |
| 子代理 id 找不到时列最近子代理 | `6fbb36b` #1491 | agent-extension（JS） | 直接搬 |
| 子代理被重启切断后收敛 run 状态 | `98ad390` #1473 | Rust 运行期 | 中等 |
| 工具顺序保持以命中 prompt cache | `c319c85` #1481 | Rust 工具选择 | 中等，低优先 |
| 侧边栏空项目组 Pin / archive 空标签 | `2b7c403` · `b12b908` | 纯前端 | 直接搬 |
| Side chats（`/side`） | `3bad742` #1469 + 后续 | 大功能 | 单独立项 |
| Docker / Next 16 / 共享 model runtime | `e97d00b` · `ad1b381` · `15c52bb` | 不适用 | 跳过 |
| 导出 HTML 补丁删除 | `1fbf6c6` | 不适用 | pi-star 自研导出，无此补丁 |
| Extension UI 主题（服务端一半） | `da6d538` | 不适用 | pi-star 不执行 TS 扩展 |

## A. 纯前端，可直接迁移

这些只改 `ui/src`，不改 Rust。可以按文件从 0.15 取，注意重新套用本地改造。

1. **右侧面板标签拖拽排序**：`lib/panel-tab-order.ts`（新）、`components/TabBar.tsx`
   （HTML5 drag + drop 指示线）、`AppShell.tsx` 的 `panelTabOrder` 状态。顺序只存在内存，
   关闭的标签会从顺序里移除，重开时追加。
2. **favicon 未读徽标**：`lib/favicon-badge.ts`（新，canvas 画角标，颜色与侧栏状态点一致）、
   `SessionSidebar.tsx`（把 awaiting/unread 计数喂给它）、`AppShell.tsx` 新增
   `hiddenFinishedSessionIds`（后台标签页完成、侧栏不算未读的会话）。
3. **引用只插入所选文本**：`lib/quoted-selection.ts` 的 `buildQuotedSelection(text)` 只生成
   `> ...` 加空行，新增 `quoteSeparator()`；`ChatInput` 增加 `insertQuote`；`ChatWindow`
   去掉 `sourceEntryId` 与「Ask in new chat」。注意「Ask in side chat」依赖侧聊，若暂不做侧聊，
   只搬 Quote 部分即可。
4. **Git changes 列表/树**：`FileExplorer.tsx` 的 `ChangesTreeRows`/`ChangesFolderRow` 与
   切换按钮；`lib/sidebar-prefs.ts` 增加 `pi-web:sidebar-changes-view-mode`（localStorage）。
   树的构建复用 `lib/search-tree.ts`；行高改用 `--file-row-height`。
5. **散文默认换行**：`lib/file-viewer-state.ts` 增加 `wrapsByDefault()` /
   `resolveInitialWrapLines()`，`wrapLines` 变为可选（`undefined` 表示用户没选过）；
   `FileViewer` 的 diff 视图也支持换行。
6. **HTML 预览 storage shim**：`lib/html-sandbox-polyfill.ts`（新）把内存版
   `localStorage`/`sessionStorage` 注入 `srcDoc`，修复不透明来源下预览里读写 storage 抛异常。
7. **手机端**：`app/globals.css` 增加 `--top-bar-height`（36 / 44px）、`--file-row-height`
   （24 / 36px）、`.message-action-label` 在小屏隐藏；`AppShell` 把 Agents 按钮从 More 菜单
   挪到旁边并统一图标尺寸；`ChatInput` 的 `controlSize`；`MessageView` 给操作按钮文字包
   `<span className="message-action-label">`。`.chat-composer-row` 容器查询（面板旁窄于 480px
   时 Steer / Follow-up 只显图标）随侧聊引进，但只依赖 CSS，可以单独先上。
8. **手机 Settings**：`SettingsPanel.tsx` 的 `usePhoneSettingsLevel`（home/section/detail 三级、
   `history.pushState` 让系统 Back 逐级返回、关闭时 `history.go(-pushed)`）、
   `animatePhoneLevelChange`（进入/返回动画，尊重 `prefers-reduced-motion`）、
   `components/SettingsUi.tsx` 的 `ConfigDetailNavigationContext`；`app/settings.css` 与
   `AppShell` 的 `openAtHome`。桌面窗口变窄时保持当前 section。
9. **模型面板排序**：`components/models-config-helpers.ts` 的 `moveModelEntry` /
   `modelDropIndex` / `sortModelEntries`（自然排序、稳定），`ModelsConfig.tsx` 的行拖拽、
   Alt+↑/↓、A–Z 按钮。models.json 由现有写接口整体保存，Rust 无需改动。
10. **自定义消息文本布局**：`lib/markdown.ts` 的 `remarkTextLayout` 从源文本恢复段落缩进，
    `MarkdownBody` 增加 `keepTextLayout`，`MessageView` 的自定义消息使用它；
    `globals.css` 的 `.markdown-custom-message` 改为等宽字体 + `pre-wrap` + 段落间距，
    并用 `@font-face` 的 `unicode-range` 覆盖箭头/制表符（`Terminal Drawing`）。
11. **扩展 UI ANSI 调色板（客户端一半）**：`components/AnsiText.tsx` 打开
    `ansiUp.use_classes = true`，`globals.css` 增加 16 色 `--ansi-*` 与
    `.ansi-*-fg/-bg`（明暗两套，取自 `lib/terminal-theme.ts`）。服务端一半
    （`lib/extension-ui-theme.ts`，给扩展组件一个 pi `Theme`）只有 pi-star 真的在进程内跑
    TS 扩展时才有意义，见 `docs/status.md` 的运行时边界。
12. **流式指标稳定**：`MessageView.tsx` 抽出 `AssistantMessageHeader`，模型名始终显示，
    token 数和 t/s 只在流式时出现，并用 `minWidth: calc(6ch + 1px)` / `9ch + 1px` 预留宽度，
    避免数字跳动时整行抖动。
13. **杂项修复**：Skills 搜索时不再按 dormancy 分组重排（`ChatInput.buildSlashCommandLayout`
    多一个 `query` 参数）；目录选择器拖拽到组件外不再关闭、导航中禁用目录选择；模型
    provider 选择器在 IME Escape 时保留；provider 登录反馈本地化（i18n key 见下）；
    侧栏空项目组显示「Pin to keep」；archive 折叠时空标签不再显示。
14. **PWA manifest**：0.13→0.15 把 `app/manifest.ts` 换成 `app/manifest.webmanifest/route.ts`，
    并给 `<link rel="manifest">` 加 `crossOrigin="use-credentials"`。pi-star 的 manifest 是
    Rust 路由（`routes/misc.rs::manifest`），只需在 `ui/index.html` 的 link 上加
    `crossorigin="use-credentials"`；顺带把该路由的 content type 改成
    `application/manifest+json`（现在走 `no_store(json!())`，是 `application/json`）。

新增 i18n key（en / zh-CN / zh-TW 各一份）：`sideChat.*`、`chat.commandSide`、
`chat.askInSideChat`、`chat.sideChat*`、`chat.selectionActions`、`chat.modelScopeAll`、
`chat.modelScopeScoped`、`chat.selectModel`、`chat.switchingModel`、`files.viewAsTree`、
`files.viewAsList`、`settings.backTo`、`sidebar.pinToKeep`、`sessionPanel.*`、`models.serverError`
等。pi-star 的 i18n 本地改动较大（品牌、桌面文案），建议按 key 合并而不是整文件覆盖。

## B. 需要 Rust 后端配套

### 1. 模型 API：`providerName` / `isScoped` / `?scope=all`

上游 `f6c8699`、`14c0c94` #1503。

- `GET /api/models` 的 `modelList[]` 增加 `providerName`（显示名，取 runtime provider 的
  `name`，models.json 里也支持 `name`），并返回 `isScoped`。
- 新增 `?scope=all`：忽略 `enabledModels`，返回全部可用模型（缓存键要区分 `cwd#all`）。
  客户端选「all」或当前选中模型不在 scoped 列表时懒加载。
- `GET /api/auth/providers` 增加 `providerNames`（所有 runtime provider，含没有聊天模型的）。
- `enabled-models`：provider 的 `name` 用显示名；`stalePatterns` 改为「匹配不到任何可用模型」
  （`7977daf` 删除了 `known` 概念，登出 provider 的条目也算 stale，`prune` 会清掉）。

pi-star 现状：`crates/pi-star-core/src/models.rs` 的 `ModelListItem` 只有
`id/name/provider(/input)`，排序是 `provider + name`（上游是 `orderSelectorModels`）；
`routes/settings.rs::enabled_models_view` 的 provider `name` 直接等于 id，
`stalePatterns` 恒为 `[]`，没有 pins。做这一项时顺带把视图补齐，客户端面板才能完整。

### 2. 运行中切换模型

`7a815d1` #1527。客户端只改 `hooks/useAgentSession.ts`：运行中（`agentRunningRef` 或
`bashRunningRef`）不要 `loadSession()`（会把流里的消息换掉），改为读 `/api/agent/:id` 的
live state 同步模型/thinking，运行结束后再以文件为准。pi-star 已有 `syncLiveModel` 和
`agentStatusUrl`，这段可以几乎原样落。

Rust 侧：`POST /api/agent/:id` 的 `set_model` 现在直接透传给 RPC/QuickPI，未见运行中拒绝；
需要实测两点：pi 1.4.0 的 `--mode rpc` 在 run 中接受 `set_model`；QuickPI 的 agent loop
在每次请求时读当前模型。两者都通过就能启用，不需要新接口。

### 3. Windows 终端 shell

`fff941a` #1508。pi-web 在 Windows 上优先用 pi `getPowerShellConfig()` 解析出的 PowerShell
（PowerShell 7 → Windows PowerShell，保留 Store 的 WindowsApps 别名），失败才回退
`ComSpec`/cmd。pi-star 的 `crates/pi-star-core/src/terminal.rs` 目前是「`SHELL` 环境变量 →
Windows 的 `powershell.exe` → `/bin/sh`」，既没有 PowerShell 7 优先，也没有 `where` 探测和
cmd 回退（`SHELL` 若指向 Git Bash 会直接用它）。可以按上游顺序重写这段探测，不必依赖 pi 的
JS API。

### 4. 子代理

- `6fbb36b` #1491：`get_subagent_result` 找不到 id 时，从本会话最近的 tool result
  `details` 里列出最多 10 个 `sessionId: description`。pi-star 的
  `agent-extension/subagent.ts` 与上游结构一致，可直接搬（纯 JS）。
- `98ad390` #1473（**暂缓**）：上游在「子会话自己的一轮对话结束」时读文件、按最后一条
  assistant 的 stop reason 写 `completed/aborted/failed`，用于修复重启切断后 run 永远停在
  `running/queued`。pi-star 目前 RPC 子代理由 `agent-extension/subagent.ts` 直接 spawn，
  既不写 `pi-web:subagent` 元数据也不写 `pi-web:subagent-status`（QuickPI 只在元数据里写
  `running`），而且 pi-star 的子代理面板仍是只读的 `SubagentViewer`，用户无法「在子会话里
  发一轮」——该场景不存在。等子代理运行期生命周期补齐或有可交互面板时再做。
- `lib/subagent-extension.ts` 在 0.15 改为从 `typebox` 取 `Type`（pi 1.5），pi-star 该文件
  不在客户端同步范围，跟着 pi 1.5 升级再处理。

### 5. 工具顺序与 prompt cache

`c319c85` #1481（**暂缓**）。`resolveActiveToolNames()` 现在按「`carry` 的顺序」输出：先保留
transcript 里记录的（且仍被选中的编码工具）、再追加新选的，而不是 `selected` 在前。pi 按这个
顺序声明工具，顺序变了就重写 `<tools>` 段、丢失 provider 的 prompt cache。pi-star 的 RPC
后端把客户端算好的 `toolNames` 直接传给扩展，顺序不经过服务端，等价改法要在客户端
`tool-presets`/`set_tools` 路径上做，收益只是缓存命中，暂缓。

### 6. 通知只响一次

`5f31fb6` #1515。页面自己的事件流会先弹一个通知，随后同一次完成的 Web Push 再弹一个
（同 tag + `renotify` 又响一次）。上游给通知 data 加 `shownAt`，sw 收到 push 时查同 tag
通知：10s 内的视为同一次完成，`renotify: false` 静默替换。pi-star 的 push 发送在 Rust
（`crates/pi-star-core/src/push.rs`），`ui/public/sw.js` 是同步过来的；需要两边一起改：
前端通知 data 带 `shownAt`，sw 加 `hasRecentNotification()`，Rust 发送端保持 tag 一致即可。

### 7. 已登出 provider 的 enabledModels

`7977daf` #1529，见 B.1 的 `prune/stale` 部分；pi-star 的简化视图没有 `known` 概念，
实现时直接按「匹配不到可用模型即 stale」即可，不需要迁移旧语义。

## C. Side chats（`/side`）专题

上游 `3bad742` #1469（ADR `docs/adr/0007-side-sessions.md`），后续 `5644bcf` #1512（标签随主
会话恢复）、`502cc16` #1507（Agents 面板删除）、`48a158b`（删除确认行高）、`6d0946b`
（引用弹窗改为「侧聊提问」）。这是 0.13→0.15 唯一的大功能，建议单独立项。

数据模型：

- 侧聊就是「侧栏 Fork 的副本」：同一份 `forkSessionBranch` 复制逻辑 + `POST
  /api/sessions/[id]/fork` 的 `{side:true,name?}`。
- 复制出的文件第 2 行写 `pi-web:side` custom entry
  （`{version:1,parentSessionId,createdAt}`），并把原第一条 entry 的 `parentId` 链到它下面；
  末尾追加一条 `custom_message`（`customType: pi-web:side-boundary`, `display:false`）作为
  隐藏边界；边界前再写一次工具 pin（`pi-web:tool-selection`），防止父会话别的分支上的 pin
  丢失导致侧聊工具放大。
- 关系是 `{kind:"side",parentSessionId}`，和 subagent 一样隐藏：不出侧栏行、计入 Agents
  徽标、跟随父会话的 pin/archive、随父会话删除、可单独删除。
- 统计只算 `createdAt` 之后的 entry（`sideOwnEntries()`），否则父会话的用量会被重复计一次；
  搜索跳过侧聊；fork/clone 拒绝侧聊（`side` refusal）。
- 工具：父会话的工具 ∩ `SIDE_TOOLS`（`read, bash, powershell, grep, find, ls`），作为 pi 的
  allow-list，扩展/MCP/Code mode/子代理工具都不注册；Chat-only 父会话保持无工具。
- 客户端：`/side` 是 builtin slash command（流式中也可用），`ChatInput` 增加侧聊按钮，
  `ChatWindow` 把边界消息渲染成分隔线，右侧面板的 `SessionChatPanel` 复用 `ChatWindow`
  （`panel` 模式，不抢全局快捷键），`side-tab-state.ts` 按主会话把侧聊标签存 sessionStorage，
  `AgentSessionPanel` 增加「New side chat」和删除，`SessionChatPanel` 取代了
  `SubagentViewer`（子代理也变成可交互的 chat 面板）。

pi-star 落地要点：

1. `SessionRelation` 增加 `Side` 变体（`crates/pi-star-core/src/session.rs`），
   `SessionInfo` 增加该关系；`info()` 里识别 `pi-web:side`。
2. **摘要索引要注意**：`session_index.rs` 在 summary 模式下走
   `read_header_only_info()`，只读第一行 header，看不到第 2 行的 side entry（现有 subagent
   关系也有同样问题，靠 `details_pending` 后补）。上游的做法是把 side entry 写在第 2 行并用
   「有界头部读取」；pi-star 需要把 `read_header_only_info` 扩成读前 N 行，或者让侧聊在列表
   摘要里先按 `parentSession` 当 fork、详情加载后再纠正。索引里缓存的是整个 `SessionInfo`，
   加字段/枚举后要 bump `IndexFile.version`（现在是 1），否则旧缓存反序列化失败或读到旧关系。
3. Fork 路由：`routes/sessions.rs` 有两套（活动会话走 pi RPC 的 `fork`，否则 Rust 自己复制）。
   两套都要支持 `side:true`：复制后重写文件（第 2 行 metadata、边界 entry、工具 pin），
   并且不能对运行中的父会话抢写——上游就是在文件层面复制、从不触碰运行中的 wrapper。
4. 会话读取/统计/删除/搜索：`GET /api/sessions/:id` 的 relation、`compute_stats`/
   `total_active_ms` 用 own entries、`session-delete` 的父子折叠、`sessions/search` 过滤。
5. Agent 打开：RPC 后端在 `spawn pi --mode rpc` 时已经能传 `--tools <csv>`（pi-star
   `pi_rpc.rs::open_path` 已有该分支），正好是侧聊需要的 allow-list；需要从会话文件里算
   `pin ?? 记录的工具` ∩ `SIDE_TOOLS` 再传。QuickPI 后端需要在打开会话时设置同样的工具集，
   并拒绝 `set_tools`/`navigate_tree` 把工具加回来。
6. 客户端按文件搬 A 组之外的那些：`session-family.ts` 的 `side`/`isHiddenRelation`/
   `sideChatTitle`、`types.ts`、`side-chat-client.ts`、`side-tab-state.ts`、
   `SessionChatPanel.tsx`、`AgentSessionPanel.tsx`、`TabBar.tsx`、`SessionSidebar.tsx`、
   `ChatWindow.tsx`、`ChatInput.tsx`、`useAgentSession.ts`、`session-stats-format.ts`。
   这部分改动横跨 16 个文件，和本地改造（i18n、AppShell 后端选择器）会冲突，建议单独分支做。

## D. 不适用

- **Docker**（`e97d00b`、`aa4be54`）、**Next 16.4.4**（`ad1b381`）：pi-star 是 Vite + Axum +
  Tauri，没有对应物。
- **共享 model runtime 一小时 TTL**（`15c52bb`）：Node 进程内模型运行时；pi-star 的模型目录
  是 Rust 自己缓存（5s）和 `models.dev` 刷新，语义不同。
- **导出 HTML 补丁删除**（`1fbf6c6`）：pi 1.5.0 的模板改成迭代遍历，不再栈溢出。pi-star
  优先用 `pi --export`，会随 pi 升级自动受益；Rust 的简化导出没有这段补丁，无需动作。
- **`lib/extension-ui-theme.ts` 服务端一半**：需要进程内执行 pi 的 TS 扩展；pi-star 的
  `docs/status.md` 已把「Node/TS 扩展执行」列为运行时边界。客户端配色（A.11）可以先上。

## pi 1.5.0

pi-web 0.15 依赖 pi 1.5.0；pi-star 打包 1.4.0。是否跟进要单独评估：

- 打包只需改 `packaging/pi-runtime.json` 的 `pi` 版本，然后跑
  `scripts/prepare-pi-runtime.mjs` 与三平台安装包校验。
- 需要回归：pi-star 的 `agent-extension/`（依赖 pi 扩展 API）、RPC 协议（`pi --mode rpc` 的
  命令与事件）、`pi --export`、QuickPI 与 pi 1.5 的会话格式兼容性。
- pi 1.5 还带来：MCP OAuth 的 `clientRegistration: "auto"` 默认值（pi-star 的 Rust MCP OAuth
  需要认识这个值）、`typebox` 取代 pi-ai 的 `Type`、Sign in with ChatGPT 需要传 device ID
  （pi-star 目前只内置 Anthropic / GitHub Copilot OAuth，暂不受影响）。
- 侧聊本身**不要求** pi 1.5：它用到的 `getCurrentTools` 在 1.4.0 就有。

## 同步流程建议

1. 先做 A 组（纯前端），按文件从 `v0.15.0` 取，重新套本地改造；每批跑
   `npm run typecheck && npm test`（pi-star 已迁移 178 个客户端测试，A 组的新测试也能直接搬：
   `AppShell.favicon-badge.test.mjs`、`AppShell.side-tabs.test.mjs`、`ModelSelector.test.mjs`、
   `ModelsConfig.test.mjs`、`panel-tab-order.test.mjs`、`favicon-badge.test.mjs`、
   `html-sandbox-polyfill.test.mjs`、`side-tab-state.test.mjs`、`settings` 相关测试等）。
2. 再做 B 组（后端小改），先补模型 API 的三个字段，再做运行中切模型和 Windows 终端。
3. Side chats 单独立项，按「文件层 → 只读展示 → 交互面板」拆三个阶段，每阶段都能独立验收。
4. 建议给 `scripts/sync-ui-from-pi-web.mjs` 加两个能力：`--report`（只列出与目标版本的差异
   文件，不写盘）和把上游 tag 写进 `ui/UPSTREAM.md`（现在只记录模块数量，看不出基线版本）。

## 附：0.13 → 0.15 提交索引

```
2621aa1 fix(directory-picker)      f07f368 feat(file-viewer) 散文换行
98ad390 fix(subagents) 收敛切断     c319c85 fix(tools) 工具顺序
0b1eeff feat(tab) favicon 徽标      cb3cebe fix(chat) 窄屏操作按钮
b54a63d fix(pwa) manifest 凭据      e97d00b feat: docker
15c52bb fix(models) runtime TTL    2b7c403 fix(sidebar) 空组 pin
6fbb36b fix(subagents) 候选列表     3bad742 feat(side) 侧聊
6d0946b feat(chat) 引用只插文本      5269cb0 fix HTML 沙箱 storage
6265065 / 4bb6d2f models 本地化     7b5cd68 fix(chat) skill 排序
2251c3f feat(files) changes 树        c2ef9a5 fix(files) 导航中禁用选择
c6789c5 fix(models) IME              b25fa0a fix(chat) 自定义消息布局
14c0c94 feat(models) scoped/all      b12b908 fix(sidebar) archive 标签
fff941a fix(terminal) PowerShell     502cc16 feat(sidebar) 删除侧聊
ff8de36 chore(deps) pi 1.5.0         ad1b381 chore(deps) Next 16.4.4
57078bb feat(mobile) 触控/Agents      9afc508 feat(settings) 手机逐级
1420b81 feat(settings) 滑动动画       48a158b fix(agents) 行高
1615347 fix(settings) 窄窗保持 section 1fbf6c6 fix(export) 删补丁
da6d538 feat(extensions) 调色板      5e0e47f feat(files) 标签拖拽
f6c8699 feat(models) provider 名称    8d2fb36 fix(chat) 粘贴
5f31fb6 fix(notifications) 去重       5644bcf feat(side) 标签恢复
6386fa4 feat(models) 模型排序         79425f8 fix(chat) 流式指标
797d94b fix(auth) OAuth device ID    7a815d1 feat(chat) 运行中切模型
7977daf fix(models) 登出 provider
```

## 移植状态（本次）

已按 A + B 落地（侧聊 C 不做）：

- **A 组全部**：标签拖拽排序、favicon 未读徽标、引用只插选中文本（保留「在新对话询问」）、
  Git changes 列表/树、散文默认换行、HTML 预览 storage shim、手机端触控尺寸与 Agents 按钮、
  手机 Settings 三级导航/系统 Back/动画、模型面板排序、自定义消息 TUI 文本布局、
  `AnsiText` 调色板、流式 token/t-s 指示、窄屏操作按钮与 composer 图标化、粘贴恢复、
  通知去重、PWA manifest 凭据、侧栏空项目组 Pin、目录选择器/Skills 排序等修复。
- **B 组**：`/api/models` 的 `providerName`/`isScoped`/`?scope=all`、`/api/auth/providers`
  的 `providerNames` 与显示名、enabled-models 的 `stalePatterns`/`thinkingPin`/`kind` 与
  `prune`、运行中切模型（客户端 + 现有 Rust 透传）、Windows 终端 PowerShell 优先、
  子代理候选列表、通知 `shownAt` 去重、manifest 的 `application/manifest+json`。
- **暂缓**：`98ad390` 子代理 run 收敛（pi-star 没有可交互的子会话，见 B.4）、`c319c85`
  工具顺序（客户端路径，收益是 prompt cache，见 B.5）。
- **未动**：pi 1.5.0 升级（`packaging/pi-runtime.json` 仍是 1.4.0）。本次移植不依赖它：
  侧聊没做，`getCurrentTools` 之外用到的都是 pi-star 自己实现的 Rust 路径。

重新同步时的本地差异见 [`ui/UPSTREAM.md`](../ui/UPSTREAM.md)。
