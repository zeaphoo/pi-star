# Spec Delta

## Purpose

定义 pi-star 的浏览器前端：完整承载 pi-web 的交互与视觉，并通过同源 HTTP/SSE 与 Rust 服务端通信。

## ADDED Requirements

### Requirement: UI 与 pi-web 保持功能对等
pi-star SHALL 提供 pi-web 的全部用户可见功能，包括会话侧边栏、聊天、分支导航、文件树与预览、
Git、worktree、终端、设置面板、调试面板、PWA、主题、字体、多语言和键盘快捷键。

#### Scenario: 打开主界面
- **WHEN** 用户在浏览器访问 `/`
- **THEN** 页面渲染与 pi-web 相同的 App Shell，并加载会话列表、运行状态和当前工作区

#### Scenario: 使用深链接
- **WHEN** 用户访问 `/?session=<id>` 或 `/?cwd=<dir>`
- **THEN** UI 打开对应会话或新建会话上下文，且刷新后状态保持

### Requirement: 前端只通过同源 API 访问后端
前端 SHALL 使用相对路径 `/api/...` 访问 Rust 服务端；MUST NOT 依赖 Next.js Route Handler、
服务端组件或 Node.js 专有 API。

#### Scenario: 静态构建产物由 Rust 提供
- **WHEN** 用户从 Rust 服务加载页面
- **THEN** 页面的 JS、CSS、图标、manifest 和 service worker 均由同一源提供，API 请求不跨域

#### Scenario: 非 Next.js 宿主
- **WHEN** UI 在 Vite 构建后运行
- **THEN** 导航、查询参数、图片和字体仍正常工作，页面无 `next/*` 运行时依赖

### Requirement: 前端保持 pi-web 的降级行为
当后端不可达、SSE 断开或浏览器不支持某项能力时，UI SHALL 显示可恢复的错误状态，并保持
其余已加载内容可用；MUST NOT 因单个面板失败而白屏。

#### Scenario: 事件流断开
- **WHEN** Agent SSE 连接中断
- **THEN** UI 显示重连状态并按退避策略重连，已显示的会话内容不被清空

#### Scenario: 未知路由
- **WHEN** 浏览器请求不存在的页面路径
- **THEN** 服务端返回 UI 入口并由前端显示 404/空状态，而非返回服务器错误页

### Requirement: 前端资产与第三方许可可再分发
pi-star SHALL 保留从 pi-web 复制的 MIT 许可声明与第三方依赖许可信息，使 UI 可作为开源项目再分发。

#### Scenario: 仓库包含许可文件
- **WHEN** 用户查看仓库根目录
- **THEN** 可以看到 pi-star 自身的许可和 pi-web 来源说明

### Requirement: UI 适配桌面壳桥
前端 SHALL 通过可选的 `window.__PI_STAR__` 桥判断是否运行在 pi-star 桌面壳中；在普通浏览器中
MUST NOT 渲染桌面窗口控制按钮。

#### Scenario: 普通浏览器
- **WHEN** 页面在普通浏览器打开且不存在 `__PI_STAR__`
- **THEN** 不显示最小化/最大化/关闭按钮，且不产生桥调用错误

#### Scenario: 桌面壳
- **WHEN** 页面检测到 `__PI_STAR__.windowControl`
- **THEN** 标题栏显示窗口控制按钮，并调用桥完成最小化、最大化/还原和关闭
