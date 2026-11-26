# Design

## Context

Phase 1 已落地 `ui/src/lib/context-menu/policy.js`：在 document 的冒泡阶段取消 App 区域的
`contextmenu`，输入框、非空文本选区和 `data-native-context-menu` 子树保留系统菜单，已自行
`preventDefault()` 的组件不受影响。它只决定“是否抑制”，不绘制任何东西。

现有菜单能力集中在 `ui/src/components/SidebarMenu.tsx`：支持 `point` / `rect` 锚点、边缘翻转、
分组、子菜单、勾选项、危险项、单项快捷键、键盘导航、移动端底部 sheet、portal 到 `body` 和
焦点归还。会话行还提供了同步扩展拦截事件 `pi-star:session-row-contextmenu`。

平台约束：wry 只在 WebView2（Windows）提供 `with_default_context_menus`，Tauri 未透出；
macOS/WKWebView 没有可用的原生开关，也没有实现 `WKUIDelegate` 的 context menu 回调。因此
统一走 DOM 事件 + HTML 菜单，不依赖系统菜单（文本编辑区除外）。

## Goals / Non-Goals

**Goals:**

- 所有平台（桌面壳与普通网页）右键行为一致：App 区域不再出现浏览器菜单，改由 App 菜单接管。
- 只有一套菜单宿主和一套定位/键盘/焦点实现，侧栏现有菜单行为不变。
- 菜单定义靠近使用方，按区域扩展；新增菜单不需要改全局策略。
- 输入框、文本选区、嵌入预览仍可拿到系统菜单。
- 新增代码用 `.js` / `.jsx`，与现有 TypeScript 文件互操作。

**Non-Goals:**

- 不接原生 `NSMenu`，也不新增 Tauri command / ACL；桌面动作复用服务端 `/api/open-in-explorer`。
- 不在本变更重构既有 `.ts` / `.tsx` 文件的语言。
- 不实现 Windows/Linux 的系统级菜单关闭开关（Tauri 未暴露）；JS 策略已经覆盖三平台。
- 不改变侧栏、项目组、标签页菜单的条目与外观，只迁移实现。

## Decisions

### 1. HTML 菜单，而不是原生 NSMenu

原生菜单更接近 macOS 观感，但需要主窗口的 Tauri IPC 能力（当前按设计没有 ACL），每个动作都要
新 command，且弹窗窗口的 origin 也要同步放权。现有 `SidebarMenu` 已经是 HTML + portal，三平台
一致，主题/图标/i18n 都复用前端。故选 HTML；系统菜单只保留在文本编辑区与选区场景。

替代方案：Tauri `popup_menu` + Rust 菜单事件。放弃原因是 ACL 面扩大、菜单项状态（勾选、禁用、
动态列表）需要跨 IPC 同步，收益仅是外观。

### 2. 从 `SidebarMenu` 抽出 `PopupMenu`，不另写一套

抽取范围限定在与业务无关的部分：锚点计算与翻转、portal、分组/子菜单 DOM、键盘导航、typeahead、
sheet 模式、焦点归还。`SidebarMenu` 继续持有“侧栏菜单有哪些项”的业务逻辑，改为渲染 `PopupMenu`。
公共组件放在 `ui/src/components/PopupMenu.jsx`（新代码用 JSX），并补组件级测试保证迁移前后行为一致。

替代方案：新建 `ContextMenu` 并在若干区域逐步替换。会同时存在两套定位/焦点实现，回归风险更高。

### 3. 事件模型：组件优先，全局只做兜底

- 组件在自己的 `onContextMenu` 里决定菜单内容，调用 `menu.open({ x, y, items })` 并
  `preventDefault()`；document 冒泡监听看到 `defaultPrevented` 后不再处理。
- 无状态、只需简单菜单的区域可以用 `data-context-menu="<id>"` 声明，由注册表解析。
- 注册表按 id 返回条目构造器；条目里需要会话/文件等上下文时，通过 `data-*` 或 WeakMap 传给
  宿主，避免把大对象序列化进 DOM 属性。
- 全局策略（Phase 1）不改：输入框、选区、`data-native-context-menu` 放行。

`ContextMenuHost` 放在 App 根节点，只渲染一个 portal；`useContextMenu()` 用 context 暴露
`open` / `close`，避免每个组件各建 portal。

### 4. 单一宿主、关闭与焦点语义

- 同一时刻只有一个菜单；打开新菜单先关闭旧的。
- 关闭时机：Escape、点击/右键外部、窗口 blur、滚动、路由/会话切换、触发元素卸载。
- 指针打开的菜单不移动选区、不抢焦点；关闭后焦点回到打开前的元素（若还在 DOM 中）。
- 键盘打开（`Shift+F10` / ContextMenu 键）以当前焦点元素的矩形为锚点，关闭后焦点回到该元素。
- 移动端长按只在 `data-context-menu` / 显式 `onContextMenu` 的元素上生效，文本区域保持系统
  选择与放大镜；长按阈值与位移容差复用 `useGroupDrag` 的常量思路。

### 5. 定位复用锚点模型

沿用 `SidebarMenuAnchor` 的 `point` / `rect` 两种锚点：指针位置或触发按钮矩形。菜单优先出现在
锚点右下方，越界时向左/上翻转并夹在视口内；子菜单同样翻转。移动端（窄视口）改为底部 sheet。
滚动容器内不做“跟随”，滚动即关闭，避免 transform 祖先把 `position: fixed` 固定到局部坐标系的问题
（现有侧栏注释已记录）。

### 6. 菜单目录靠近使用方

| 区域 | 菜单项（首版） |
| --- | --- |
| 会话行 | 打开、在新窗口打开、重命名、置顶/取消、标为已读、归档、Fork、复制路径、在访达中显示、删除 |
| 项目组 | 新建会话、在新标签打开、重命名、置顶、折叠其它、归档 7 天前、删除全部会话 |
| 项目/工作区标签 | 关闭、关闭其它、关闭右侧、复制路径 |
| 聊天消息 | 复制文本、复制 Markdown、引用选中、编辑并重发（用户消息）、从这 Fork |
| 代码块 | 复制代码、下载、在代码视图打开 |
| 文件树 / 代码标签 | 打开、在右侧打开、重命名、删除、复制相对/绝对路径、在访达中显示、下载 |
| Diff | 复制 patch、打开文件、展开上下文、在访达中显示 |
| 终端 | 复制（有选区）、粘贴、清屏、右侧分屏、下方分屏、重命名、关闭 |
| 空白区 | 发布构建不弹；调试构建仅 Inspect |

动作实现：复制用 `navigator.clipboard`；“在访达中显示/用默认应用打开”调用现有
`POST /api/open-in-explorer`（桌面壳与网页都可用，服务端已有 root 校验）。

### 7. 扩展拦截保持兼容

`pi-star:session-row-contextmenu` 仍是同步、可取消的事件。会话行先派发扩展事件，被 claim 时
不打开内置菜单；没被 claim 时走新的 `useContextMenu()`。这样外部扩展不需要跟着迁移。

### 8. 调试构建保留原生菜单

`marker_script` 增加 `debug` 字段（Rust 侧 `cfg!(debug_assertions)`）。前端策略在
`debug === true` 时对没有声明菜单的区域放行系统菜单，保留右键 Inspect Element；发布构建
保持抑制。若后续希望统一为 App 菜单里的 “Inspect Element”，再单独加命令，不在本变更内。

### 9. 新代码语言

新增文件使用 `.js` / `.jsx` + JSDoc；`tsconfig.json` 已开启 `allowJs`（`checkJs` 为 false），
现有 `.ts` / `.tsx` 不改写。JSX 组件通过 `import` 直接使用，类型提示由 JSDoc/编辑器提供。

## Risks / Trade-offs

- [WKWebView 可能不把 DOM 的 `preventDefault` 反馈给系统菜单（链接/图片场景）] → 在 macOS 实机
  验证；若仍弹出，退路是 Tauri 插件实现 `WKUIDelegate` 返回 nil，本变更的前端策略不变。
- [HTML 菜单缺少系统服务（Look Up / Translate / Services）] → 选区与输入框保留系统菜单；
  静态文本的 Copy/Quote 由 App 菜单提供，属于有意取舍。
- [从 `SidebarMenu` 抽公共组件会引入回归] → 先做行为等价的纯迁移，补组件测试与现有 e2e；
  迁移期间不改菜单条目。
- [`position: fixed` 菜单被 transform 祖先影响] → 统一 portal 到 `body`，滚动即关闭。
- [移动端长按与文本选择冲突] → 只对 opt-in 元素生效，文本区域不拦截。
- [终端右键与 xterm 的选区/粘贴习惯冲突] → 明确为 App 菜单（复制/粘贴/清屏/分屏），不做
  “右键即粘贴”。
- [调试放行需要 Rust 改动] → `debug` 标记独立提交，失败时回退为“调试构建也抑制”，不影响发布。

## Migration Plan

1. 抽出 `PopupMenu` 并把 `SidebarMenu` 改为使用它；验证现有侧栏/项目/标签菜单与 e2e 全部通过。
2. 引入 `ContextMenuHost` + `useContextMenu`，先只接入权限最小的一处（会话行右键），保持
   `session-row-contextmenu` 事件语义。
3. 按目录逐区域接入；每个区域独立回滚（移除该区域的 `onContextMenu` 即恢复原行为）。
4. 最后加键盘/长按/a11y 与 Playwright 覆盖。

回滚策略：删除 `installContextMenuPolicy()` 调用即恢复浏览器默认菜单；移除宿主与各处接入即
回到现状。无数据迁移。

## Open Questions

- 无。文本选区/输入框保留系统菜单、其余区域使用 App 菜单已在上面定稿；若后续要“全 App 菜单
  包括文本”，只需调整策略与补 Copy/Quote 项，不影响宿主与目录设计。
