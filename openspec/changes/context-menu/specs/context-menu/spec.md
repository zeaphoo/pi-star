# Spec Delta

## Purpose

定义 Pi Star 在所有平台（桌面壳与普通网页）上的右键/长按菜单行为：浏览器默认菜单何时被抑制、
App 菜单如何打开与关闭、每个区域的菜单内容、键盘与无障碍要求，以及桌面动作和调试放行的边界。

## ADDED Requirements

### Requirement: 默认菜单抑制策略

App SHALL 在所有平台和所有构建（桌面壳与普通网页）上抑制自身界面区域的浏览器/WebView 默认
菜单。以下目标 MUST 保留系统菜单：可编辑字段（`input`、`textarea`、可编辑的 `contenteditable`）、
存在非空文本选区的页面、以及标记了 `data-native-context-menu` 的子树。已经调用
`preventDefault()` 的组件 MUST 优先，全局策略不得覆盖它。

#### Scenario: App 空白区域右键

- **WHEN** 用户在消息区、会话区或工具栏等非编辑区域右键
- **THEN** 不出现浏览器/WebView 默认菜单，也不出现 Inspect Element（发布构建）

#### Scenario: 输入框右键

- **WHEN** 用户在 Composer、重命名输入框等可编辑字段右键
- **THEN** 出现系统编辑菜单（复制/粘贴/拼写检查等），App 策略不拦截

#### Scenario: 选中文本右键

- **WHEN** 页面上存在非空文本选区且用户右键
- **THEN** 系统菜单保留，复制/Look Up 等能力可用

#### Scenario: 声明式放行

- **WHEN** 某个嵌入预览的子树带有 `data-native-context-menu`
- **THEN** 该子树内右键仍显示系统菜单

#### Scenario: 组件自行处理

- **WHEN** 组件在 `contextmenu` 中调用 `preventDefault()` 并打开自己的菜单
- **THEN** 全局策略不再处理该事件

### Requirement: 菜单打开与作用对象

App 菜单 SHALL 支持指针右键、键盘 `Shift+F10` / ContextMenu 键、以及触屏长按打开。菜单 MUST
作用于指针或焦点下的对象，MUST NOT 因为右键而改变当前选中项、选中会话或文本选区。长按 MUST
只对声明了菜单的元素生效，MUST NOT 拦截文本区域的选择与放大镜。

#### Scenario: 右键会话行

- **WHEN** 用户右键一个不是当前会话的会话行
- **THEN** 菜单针对该会话打开，当前打开的会话保持不变

#### Scenario: 键盘打开菜单

- **WHEN** 焦点在会话行或消息上，用户按 `Shift+F10` 或 ContextMenu 键
- **THEN** 菜单以该元素的矩形为锚点打开

#### Scenario: 触屏长按

- **WHEN** 用户在声明了菜单的会话行上长按
- **THEN** 菜单打开；在文本区域长按则保持系统选择行为

### Requirement: 单一菜单宿主与关闭语义

App MUST 保证同一时刻只有一个右键菜单。菜单 SHALL 在 Escape、点击菜单外部、窗口失焦、滚动、
路由/会话切换或打开新菜单时关闭。指针打开的菜单关闭后 MUST NOT 改变焦点；键盘打开的菜单关闭后
SHOULD 把焦点归还给打开它的元素（若仍在文档中）。

#### Scenario: 打开第二个菜单

- **WHEN** 一个菜单已打开，用户右键另一个对象
- **THEN** 旧菜单关闭，新菜单在指针位置打开

#### Scenario: Escape 关闭

- **WHEN** 菜单打开且用户按 Escape
- **THEN** 菜单关闭并恢复此前的焦点

#### Scenario: 滚动关闭

- **WHEN** 菜单打开后其滚动容器发生滚动
- **THEN** 菜单关闭

### Requirement: 菜单定位

菜单 SHALL 以指针位置或触发元素矩形为锚点，优先显示在锚点右下方；当视口空间不足时 MUST
向左/上翻转并夹在视口内。子菜单 MUST 同样翻转。窄视口（移动端）MAY 使用底部 sheet。菜单位于
滚动容器内时 MUST portal 到 `document.body`，避免 `position: fixed` 相对 transform 祖先定位。

#### Scenario: 靠近窗口右下角

- **WHEN** 用户在窗口右下角附近打开菜单
- **THEN** 菜单在锚点左上方显示，且完整位于视口内

#### Scenario: 侧栏抽屉内右键

- **WHEN** 菜单从移动端抽屉或带 transform 的容器内打开
- **THEN** 菜单相对视口定位，不被容器裁剪

### Requirement: 键盘与无障碍

菜单 SHALL 提供 `role="menu"` / `role="menuitem"`、roving tabindex、上下方向键、Enter/Space
激活、Escape 关闭、首字母跳转，以及 `aria-disabled` / `aria-checked`。菜单项 MUST 有可读的
名称，快捷键提示 MUST 可见。

#### Scenario: 键盘遍历

- **WHEN** 菜单打开，用户按上下方向键
- **THEN** 高亮在可用菜单项之间循环，跳过分隔符与禁用项

#### Scenario: 首字母跳转

- **WHEN** 菜单打开，用户按某个菜单项的首字母
- **THEN** 高亮移动到匹配的菜单项

### Requirement: 会话与项目菜单

会话行、项目组、项目标签的 App 菜单 SHALL 至少提供：会话的打开、在新窗口打开、重命名、置顶、
标为已读、归档、Fork、复制路径、删除；项目组的打开新会话、在新标签打开、重命名、置顶、折叠
其它、归档旧会话、删除全部会话；标签的关闭、关闭其它、关闭右侧、复制路径。扩展通过
`pi-star:session-row-contextmenu` 同步 claim 时 MUST 优先使用扩展菜单，内置菜单不得同时打开。

#### Scenario: 扩展接管会话行

- **WHEN** 扩展监听 `pi-star:session-row-contextmenu` 并取消事件
- **THEN** 内置会话菜单不打开，扩展自行处理

#### Scenario: 删除会话

- **WHEN** 用户选择删除会话
- **THEN** 沿用现有的确认流程；按住 Shift 选择时跳过确认（若现有菜单支持）

### Requirement: 消息、文件、Diff 与终端菜单

App SHALL 为以下区域提供菜单：聊天消息（复制文本/Markdown、引用、编辑并重发、从这 Fork）、
代码块（复制代码、下载、在代码视图打开）、文件树与代码标签（打开、在右侧打开、重命名、删除、
复制相对/绝对路径、在访达中显示、下载）、Diff（复制 patch、打开文件、展开上下文、在访达中显示）、
终端（复制、粘贴、清屏、右侧/下方分屏、重命名、关闭）。不可用的动作 MUST 以禁用项呈现，MUST NOT
消失或静默失败。

#### Scenario: 消息无选区右键

- **WHEN** 用户在一条消息上右键且没有文本选区
- **THEN** 打开 App 消息菜单，包含复制、引用、编辑（用户消息）等可用项

#### Scenario: 终端无选区右键

- **WHEN** 用户在终端上右键且没有选区
- **THEN** 打开 App 终端菜单；粘贴与分屏动作可用，复制为禁用项

### Requirement: 桌面动作与调试放行

桌面专属动作（在访达/资源管理器中显示、用默认应用打开）SHALL 复用服务端
`/api/open-in-explorer`，MUST NOT 新增 Tauri command 或扩大主窗口 ACL。调试构建 SHALL 在未声明
菜单的区域保留系统菜单（含 Inspect Element）；发布构建 MUST 抑制。

#### Scenario: 发布构建无 Inspect

- **WHEN** 用户在发布构建的空白区域右键
- **THEN** 不出现浏览器菜单，也不出现 Inspect Element

#### Scenario: 调试构建检查元素

- **WHEN** 用户在调试构建的未声明菜单区域右键
- **THEN** 系统菜单保留，可打开 Inspect Element
