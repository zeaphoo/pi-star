# Proposal

## Why

Pi Star 已经是桌面应用，但右键仍然落在 WebView/浏览器的默认菜单上：侧栏、会话行、项目组和标签页
各有一套 `SidebarMenu`，其余区域（消息、代码树、Diff、终端）右键要么无反应，要么弹出浏览器菜单。
Phase 1 只做了“抑制默认菜单”的策略（`ui/src/lib/context-menu/policy.js`），没有提供任何替代菜单，
所以桌面端和网页端的右键体验都不完整。

本变更把右键菜单从零散实现收敛为一套 App 级能力，并记录 Phase 2/3 的实现方式：先抽出公共菜单
宿主，再按区域补齐菜单内容。

## What Changes

- 从 `ui/src/components/SidebarMenu.tsx` 抽出与业务无关的 `PopupMenu`（点/矩形锚点、边缘翻转、
  分组/子菜单/勾选/危险项、键盘导航、移动端 sheet、portal、焦点归还），行为保持不变。
- 新增唯一的 `ContextMenuHost` + `useContextMenu()`（`.jsx` / `.js`）：同一时刻只允许一个菜单，
  统一 z-index、关闭时机（Escape、点击外部、滚动、路由切换、失焦）和焦点恢复。
- 沿用并完善 Phase 1 的默认菜单策略：全平台（含网页）抑制 App 自身区域的浏览器菜单；输入框、
  非空文本选区、`data-native-context-menu` 子树保留系统菜单；已调用 `preventDefault()` 的区域优先。
- 新增 `data-context-menu` 声明式 opt-in 与菜单注册表（按区域 id 解析），组件持有状态时也可直接
  调用 `useContextMenu().open(...)`。
- 按区域补齐菜单：会话行/项目组/标签页（迁移现有）、聊天消息与代码块、文件树/代码标签、
  Diff、终端；空白区域的菜单仅调试构建提供 Inspect。
- 键盘（`Shift+F10`、ContextMenu 键）与触屏长按打开“指针下对象”的菜单；焦点归还、`role="menu"`、
  roving tabindex、首字母跳转、`aria-disabled`。
- 桌面专属动作复用 pi-star 已有的 `/api/open-in-explorer`，不新增 Tauri command、不扩大 ACL。
- 调试构建保留浏览器原生菜单（含 Inspect Element）；发布构建不显示。
- 新增代码默认使用 `.js` / `.jsx`；不重写既有 `.ts` / `.tsx` 文件。

## Capabilities

### New Capabilities

- `context-menu`: 全平台的右键/长按菜单——默认菜单抑制策略、菜单宿主与定位、区域菜单目录、
  键盘与无障碍交互、桌面动作与调试放行。

### Modified Capabilities

<!-- 无：主 specs 目录为空；本变更只新增 context-menu。 -->

## Impact

- 前端：`ui/src/lib/context-menu/`、`ui/src/components/PopupMenu.jsx`、
  `ContextMenuHost.jsx`、`ui/src/hooks/useContextMenu.js`，以及侧栏、消息、文件、Diff、终端组件。
- 既有行为：`SidebarMenu` 的公共定位/键盘逻辑迁移到 `PopupMenu`；侧栏、项目组、标签页菜单
  外观与功能保持不变。`session-row-context-menu` 扩展拦截事件仍先于内置菜单。
- 服务端：无接口新增；复用 `GET/POST /api/open-in-explorer`。
- 桌面壳：调试构建放行需要把 `debug` 标记加入 `app/src/window.rs` 的 `marker_script`；
  不新增 Tauri IPC 权限。
- 测试：`PopupMenu` / 策略 / 注册表的单测；Playwright 覆盖会话行、消息、空白区和键盘右键。
