# Tasks

## 0. 前置（已完成，Phase 1）

- [x] 0.1 新增 `ui/src/lib/context-menu/policy.js`：全平台抑制 App 区域默认菜单，放行输入框、
      非空选区、`data-native-context-menu`；验证：`policy.test.mjs` 4 个用例 + `tsc --noEmit` 通过
- [x] 0.2 在 `ui/src/main.tsx` 安装策略（无 UI 元素）；验证：普通浏览器与桌面壳右键行为符合策略

## 1. 抽出 PopupMenu（Phase 2）

- [x] 1.1 新建 `ui/src/components/PopupMenu.jsx`，从 `SidebarMenu.tsx` 迁移与业务无关的锚点/
      翻转/portal/分组/子菜单/键盘/typeahead/sheet/焦点归还；验证：组件测试覆盖 point/rect、
      四边翻转、Escape、上下键、首字母、点击外部
- [ ] 1.2 `SidebarMenu.tsx` 改为渲染 `PopupMenu`，菜单条目与外观保持不变；验证：现有侧栏单测
      与 Playwright `sidebar.mjs`/`project-navigation.mjs` 通过，手工对比菜单截图无差异

## 2. 菜单宿主与注册表（Phase 2）

- [x] 2.1 新建 `ui/src/lib/context-menu/types.js`（JSDoc）：`ContextMenuItem` / `ContextMenuRequest` /
      `Anchor`，不含 React；验证：类型被 `PopupMenu` 与 `useContextMenu` 复用，`tsc` 通过
- [x] 2.2 新建 `ui/src/hooks/useContextMenu.js` + `ContextMenuHost.jsx`：context 暴露 `open`/`close`，
      全局唯一 portal，保证同一时刻一个菜单；验证：单测覆盖“打开第二个菜单会替换”“关闭恢复焦点”
- [x] 2.3 新建 `ui/src/lib/context-menu/registry.js`：按 `data-context-menu` id 解析条目构造器，
      支持若干内置区域；验证：注册/查找/未知 id 的单测
- [x] 2.4 在 App 根节点挂载 `ContextMenuHost`（不改变既有 DOM 结构）；验证：应用启动无可见变化，
      现有 1833 个前端单测与 `npm run build` 通过

## 3. 会话行接入（Phase 2）

- [ ] 3.1 会话行右键改用 `useContextMenu()`，保留 `pi-star:session-row-contextmenu` 优先拦截；
      验证：扩展 claim 时内置菜单不出现，未 claim 时在指针处打开
- [ ] 3.2 菜单项与现有 `SidebarMenu` 行为对齐（打开/新窗口/重命名/置顶/已读/归档/Fork/复制路径/
      删除，Shift 跳过确认）；验证：Playwright 覆盖右键、键盘 `Shift+F10`、Escape、删除确认
- [ ] 3.3 项目组与项目标签、工作区标签迁移到宿主；验证：`project-navigation.mjs` 通过，
      菜单锚点与翻转与迁移前一致

## 4. 消息与代码块菜单（Phase 3）

- [x] 4.1 `MessageView` / `ChatWindow` 消息区接入菜单：复制文本、复制 Markdown、引用选中、
      编辑并重发（用户消息）、从这 Fork；验证：`message-menu.test.mjs` + 全量单测通过
- [x] 4.2 Markdown 代码块接入菜单：复制代码、下载、在代码视图打开；验证：单元测试从代码块
      提取文本/语言，Playwright 复制后剪贴板内容正确
- [x] 4.3 选区与菜单的边界：存在非空选区时系统菜单保留，App 菜单通过消息区域右键（无选区）
      或 `Shift+F10` 打开；验证：macOS/Chromium 手工确认 + 自动化断言 `defaultPrevented`

## 5. 文件树与代码标签菜单（Phase 3）

- [x] 5.1 `FileExplorer` 节点接入：打开、插入路径、复制相对/绝对路径、下载、重命名、删除、
      在系统文件管理器中显示；验证：`files_rename_and_delete_respect_the_allowed_roots` + `tsc`/单测/build 通过
- [ ] 5.2 `TabBar`/`CodeWorkspace` 代码标签接入：关闭、关闭其它、关闭右侧、复制路径；验证：
      与现有 `onTabContextMenu` 的入口合并，标签状态与菜单项一致
- [x] 5.3 模型、主题等纯展示节点不提供菜单；验证：空白/未知节点右键不再出现浏览器菜单

## 6. Diff 菜单（Phase 3）

- [x] 6.1 `GitWorkspace` 变更文件与 diff 行接入：复制 patch、打开文件、展开上下文、
      在访达中显示；验证：仅在有选中文件/支持文本 diff 时启用对应项
- [x] 6.2 历史视图与 commit 文件复用同一套文件动作（`lib/context-menu/file-menu.js`：复制相对/绝对路径）；验证：`tsc`、单测与 `npm run build` 通过

## 7. 终端菜单（Phase 3）

- [x] 7.1 `TerminalPanel` 接入：复制、粘贴、清屏、重启、关闭；分屏仍走工具栏按钮，未进菜单；验证：
      无选区时复制禁用，有选区时复制内容与 `getSelection()` 一致
- [x] 7.2 运行中的命令不因右键/菜单操作被打断（不发送额外输入）；验证：Playwright 终端用例
      连续运行输出后打开菜单，进程仍存活
- [x] 7.3 不用“右键即粘贴”的隐式行为；验证：右键只打开菜单，粘贴仅在点击菜单项时发生

## 8. 键盘、长按与无障碍（Phase 3）

- [x] 8.1 全局 `Shift+F10` / ContextMenu 键：对带 `data-context-menu` 或显式接入的焦点元素
      以矩形为锚点打开；验证：会话行、消息、文件树各一条键盘用例
- [x] 8.2 触屏长按：仅对 opt-in 元素生效，位移超过阈值取消；验证：移动端视口 Playwright
      长按打开菜单，文本区长按不拦截
- [x] 8.3 无障碍：`role`/`aria-*`/roving tabindex/首字母跳转/焦点归还；验证：axe 或
      键盘脚本检查菜单可完全操作，Escape 后焦点回 opener

## 9. 调试构建与文档

- [x] 9.1 `app/src/window.rs` 的 `marker_script` 增加 `debug` 标记（`cfg!(debug_assertions)`）；
      前端策略在 debug 且未声明菜单时放行系统菜单；验证：debug 构建可右键 Inspect，
      发布构建不可；`cargo check` 通过
- [x] 9.2 更新 `CHANGELOG.md` 的 Unreleased 段落与本变更 `design.md`（若实现中偏离）；
      验证：`openspec validate context-menu --strict` 与发布脚本的 CHANGELOG 检查通过

## 10. 集成验收

- [ ] 10.1 Playwright 覆盖：会话行右键、消息右键、空白区无浏览器菜单、输入框保留系统菜单
      （断言 `defaultPrevented`）、`Shift+F10`、移动端长按；验证：`npm run test:e2e` 全绿
- [ ] 10.2 手工验收矩阵：macOS / Linux / Windows 桌面壳 + 浏览器页面各执行一次右键清单；
      验证：菜单定位不越界、无系统菜单泄漏、无焦点丢失

## Workflow follow-up

- 实现完成并全部验证后，运行 `openspec archive context-menu` 归档并把 `context-menu` 能力写入主 specs。
- 若 macOS 实机确认 WKWebView 仍为链接/图片弹出系统菜单，另开变更评估原生 `WKUIDelegate` 插件；
  本变更的前端策略与菜单目录不受影响。
