# pi-star app 实验一：Web（旁路进程提供）→ Tauri 窗口控制

状态：设计稿（未实现）
环境：tauri 2.12.4 / tauri-cli 2.12.4 / rustc 1.97.1 / node 24.15 / webkit2gtk 2.44 / DISPLAY=:1

项目关系：本仓库（pi-star）是唯一实现目标；`PI-Desktop`（Electron shell）与 `pi-web`（Next.js UI）
是两个**只读参考项目** —— 参考它们的窗口控制语义、sidecar 生命周期约定与 UI 拆分方式，
不借用它们的命名空间（因此注入的桥是 `__PI_STAR__`，而非 `__PI_DESKTOP__`）。

---

## 1. 问题

Tauri 打开一个 webview，页面**不是** Tauri 打包的静态资源，而是 Tauri 自己启动的本地进程提供的
`http://127.0.0.1:<port>`。页面上要画三个按钮（最小化 / 最大化·还原 / 关闭）。

问题不是"怎么调 API"，而是：**页面处于 remote origin，Tauri 的 IPC 是否还可用、需要放开什么、
以及桥接层放在哪一侧。**

## 2. 结论（推荐方案）

**A. 用 Tauri IPC，但把 Tauri 细节关进一个"注入式桥"里。**

```
┌──────────────────────────── Tauri 进程 ────────────────────────────┐
│  窗口 "main" (decorations=false, frameless)                        │
│   ├─ 启动时注入 init script → window.__PI_STAR__  (唯一 Tauri 感知层)│
│   │     └─ 内部用 window.__TAURI__.window.*  → plugin:window|*     │
│   └─ 页面（来自 sidecar）只写 window.__PI_STAR__?.windowControl()  │
│                                                                    │
│  ACL: capabilities/window-controls.json                            │
│     local:false + remote:{ urls:["http://127.0.0.1:<port>"] }       │
│     permissions: core:window:allow-minimize|toggle-maximize|close …│
│                                                                    │
│  sidecar 子进程（Node/pi-web 或实验用 harness）: 端口就绪/退出清理     │
└────────────────────────────────────────────────────────────────────┘
```

三条关键事实（已在本机 tauri 2.12.4 源码中核对，不是记忆）：

1. **`http://127.0.0.1:<port>` 在 ACL 里算 remote，不算 local。**
   `Webview::is_local_url()` 只认 `tauri://localhost`、`get_app_url()`（`devUrl` / `frontendDist`）
   和自注册 protocol。旁路进程的 URL 都不匹配 → `Origin::Remote`。
   证据：`tauri-2.12.4/src/webview/mod.rs`（`is_local_url`、`on_message` 里 `acl_origin`）。
   → capability 必须写 `remote.urls`，否则 IPC 被拒。

2. **remote origin 下，自定义 command 默认被拒，内置 `core:window:*` 可用（只要给了权限）。**
   证据：`webview/mod.rs` 里 `if (plugin_command.is_some() || has_app_acl_manifest || !is_local) && invoke.acl.is_none() { reject }`。
   → 要么只用内置 window 命令（零 Rust 代码），要么在 `build.rs` 里
   `tauri_build::AppManifest::new().commands(&["pi_window"])` 生成 app ACL 清单，
   再在 capability 里授权 `allow-pi-window`。

3. **`app.withGlobalTauri: true` 注入的是 init script，对 remote 页面同样生效。**
   证据：`manager/webview.rs:218` 把 `scripts/bundle.global.js`（= `window.__TAURI__`，含 `window` 模块）
   作为 main-frame init script 推入每个 webview；`Webview::navigate` 文档明确"导航后 init script 重跑，
   `window.__TAURI__` 继续可用，但 ACL origin 随 URL 改变"。
   → web 端**不需要**依赖 `@tauri-apps/api`，不需要打包器感知 Tauri。

因此 web 侧只需要 `window.__PI_STAR__?.windowControl("minimize")`，
在普通浏览器里 `undefined` → 按钮不渲染，pi-web 可继续独立开发。

## 3. 被否决 / 备选方案

| 方案 | 结论 |
| --- | --- |
| **B. Rust 里再起一个本地 HTTP 控制口**（页面 `fetch` + token + CORS） | 可行且框架无关，但要额外端口、token 分发、CORS/Origin 校验、CSRF 面；本实验不需要。留作"web 也跑在浏览器里调试桌面能力"时的后备。 |
| **C. 自定义 protocol 反代 sidecar**（`pi://localhost` → 变成 local origin，免 remote ACL） | 诱人但 wry 的 custom protocol handler 返回完整响应，**不能流式**；pi-web 的 agent 事件是 SSE，会坏。否决。 |
| **D. 只开原生装饰（`decorations: true`）** | 不用写按钮，但 macOS/Windows/Linux 观感不一致，且实验目标就是这个通道。只作为 Linux 边缘 resize 问题的兜底。 |
| **E. 页面里直接 `__TAURI_INTERNALS__.invoke`** | 内部私有 API，跳过 `withGlobalTauri` 的公共层，无收益。否决。 |

## 4. 三个按钮的具体授权

```json
// src-tauri/capabilities/window-controls.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "window-controls",
  "description": "sidecar 页面可控制 own 窗口",
  "local": false,                       // 内置 splash 页不给这份权限
  "remote": { "urls": ["http://127.0.0.1:30153"] },
  "windows": ["main"],
  "permissions": [
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-close",
    "core:window:allow-is-maximized",
    "core:window:allow-start-dragging",          // data-tauri-drag-region
    "core:window:allow-internal-toggle-maximize", // 双击标题栏
    "core:event:allow-listen",                    // tauri://resize → 图标状态
    "core:event:allow-unlisten"
  ]
}
```

注意点：

- `core:window:allow-close` 等于"允许页面退出 app"，remote origin 上这是最小必要集里最"重"的一条。
  生产形态建议改用 **自定义 command `pi_window`**（单条 `allow-pi-window`），在 Rust 侧
  忽略 JS 传来的 label、只作用于调用者窗口，并可按需把 close 改成"隐藏到托盘"。
- URLPattern 里 `pathname` 为空或 `/` 时 Tauri 会规范成 `*`（`tauri-utils/src/acl/mod.rs`
  `RemoteUrlPattern::from_str`），实测 `*` 能跨越 `/` 匹配子路径。实现时以 dev 构建的
  ACL 报错（"allowed on: …"）为准做一次校验。
- **陷阱：不要用 `build.devUrl` 指向 sidecar。**
  `get_app_url()` 在 dev 下返回 `devUrl`，页面会被判成 **local**，于是 `tauri dev` 能跑、
  打包后（`frontendDist`）变 remote 直接失效 —— 权限模型随构建方式漂移。
  本方案固定用 `WebviewUrl::External(sidecar_url)` / `navigate()`，dev 与 prod 行为一致。

## 5. 桥接脚本（唯一 Tauri 感知文件）

`src-tauri/src/bridge.js`，用 `include_str!` + `WebviewWindowBuilder::initialization_script()` 注入。
init script 顺序上排在 Tauri 自己的 invoke/global 脚本之后，可以直接用 `window.__TAURI__`。

命名归 pi-star 自己：`__PI_STAR__`。语义只**参考** PI-Desktop 的 `api.windowControl(action)`
（那边是 Electron preload，`minimize|maximize|close|getState` 四态已跑通并有 e2e），
但 pi-star 不复用它的名字、不把 pi-web 改造成 pi-desktop 的客户端；
同一个 `windowControl(action)` 形状只是为了让三个项目的心智模型一致，方便阅读与比对。

```js
window.__PI_STAR__ = {
  version: 1,
  platform: "linux",
  windowControl(action),        // "minimize" | "maximize" | "close" | "getState"
  onWindowMaximized(cb) → off,  // 监听 tauri://resize 后 is-maximized
};
```

实现要点：

- 顶部 `const tauri = window.__TAURI__; if (!tauri?.window) return;` → 浏览器内静默无副作用。
- 用 `getCurrentWindow()`（label 由 Tauri 在 `__TAURI_INTERNALS__.metadata` 里给，remote 页同样有）。
- 外层 `Object.freeze`，避免页面误改；不暴露 `invoke` 本体，页面拿不到任意 command 的能力面（虽然 ACL 已经兜底）。
- 桥只属于 pi-star shell；pi-web 将来只在 UI 侧写 `window.__PI_STAR__?.…`，不 import 任何 `@pi-star/*` 包。

## 6. 窗口与 sidecar 生命周期

```
app 启动
 ├─ 立即建窗口(visible=true) → 加载内置 splash: tauri://localhost/index.html   ← 内置资源，local origin
 ├─ 后台线程: spawn sidecar(node … --port <port>) → 轮询 TCP 127.0.0.1:<port> 就绪(≤40s)
 └─ 就绪后 run_on_main_thread → webview.navigate("http://127.0.0.1:<port>/")
      ├─ ACL origin 从 local 切到 remote（bridge 已在新文档重跑）
      └─ 失败/超时 → navigate 回 splash 的 error 态（可显示日志、重试按钮）
```

- **端口**：v1 固定（harness 30153 / pi-star 30151 / pi-web 30141），capability 静态可读；
  生产建议"Rust 探测空闲端口 + `Manager::add_capability` 运行时下发 remote URL"（`dynamic-acl` 是默认 feature），
  避免把 `http://127.0.0.1:<任意端口>` 这种宽口径写死。注意宽口径的风险：
  同端口的任何本地页面都会拿到这三个窗口命令 —— 所以生产上要"随机端口 + 精确 origin"，
  必要时再让 sidecar 返回一次性 nonce 做端口归属确认。
- **退出清理**：`CloseRequested` → `prevent_close()` → kill sidecar（SIGTERM，5s 后 SIGKILL）→
  `window.destroy()`；`RunEvent::Exit` 再兜一次。
  Unix 上 `pre_exec` 里 `setsid()`，退出时 `kill(-pid)`，连 Next.js 孙进程一起收；
  Windows 用 `CREATE_NEW_PROCESS_GROUP` + `taskkill /T /F`。
  pi-web 的 `bin/pi-web.js` 本身会把 SIGTERM 转发给 next 并等 5s，所以正常路径 SIGTERM 足够。
- **dev 复用**：`cargo tauri dev` 每次改 Rust 会重启进程。端口已被一个健康实例占用时**复用而不 kill**
  （与 pi-web 文档的约定一致），否则 next 冷启动几秒 × 每次重启，实验没法做。
- **已知限制**：Tauri 被 `kill -9` 时 sidecar 会孤儿化。后续可给 pi-web 加"父进程 watchdog"，
  或复用 `pi-web` 已有的 run-record + `pi-web stop`。

## 7. 目录

```
app/
├── DESIGN.md                 ← 本文
├── src-tauri/
│   ├── Cargo.toml            tauri 2.12 / tauri-build
│   ├── build.rs              AppManifest::commands(&["pi_window"])（阶段 2）
│   ├── tauri.conf.json       withGlobalTauri:true, decorations:false, windows:[]
│   ├── capabilities/window-controls.json
│   ├── icons/
│   └── src/
│       ├── main.rs           窗口/ACL/sidecar 编排
│       ├── sidecar.rs        spawn / wait_ready / kill(进程组) / 端口探测
│       └── bridge.js         注入式桥（上面那段）
├── ui/index.html             内置 splash + 错误页（frontendDist）
└── harness/                  实验用"本地进程提供的 web"
    ├── server.mjs            node http，127.0.0.1 + --port，SIGTERM 干净退出
    └── public/               假标题栏 + 三个按钮 + 拖拽区
```

阶段 2 把 `harness` 换成真实 pi-web（`npm run dev` 或 `next start`），
pi-web 里加一个 `DesktopWindowControls` client component：
`useEffect` 后读 `window.__PI_STAR__` 决定是否渲染（避免 SSR/hydration 不一致），
标题栏条带用 `data-tauri-drag-region`，按钮区 `no-drag`。

## 8. 验收（实验矩阵）

| # | 操作 | 期望 |
| --- | --- | --- |
| 1 | Tauri 内打开 sidecar 页 | `__PI_STAR__` 存在；三个按钮分别最小化/最大化·还原/关闭 |
| 2 | 普通浏览器打开同一 URL | 无 `__PI_STAR__`、无 `__TAURI_INTERNALS__`；按钮不渲染 |
| 3 | 临时删掉 capability 的 `remote` | invoke 被拒，dev 构建错误里列出 "allowed on: …"（这条同时验证 URLPattern 写法） |
| 4 | 从 `tauri://localhost` splash 页 invoke | 被拒（`local:false` 生效）→ 证明边界是 origin 而不是页面内容 |
| 5 | 关闭窗口后 `pgrep -f harness` | 空；`ss -ltnp` 无残留端口 |
| 6 | dev 热重启两次 | 只有 1 个 node 进程（复用分支生效） |
| 7 | 阶段 2：真实 pi-web | 同一份 bridge、同一份 capability 不改动即生效 |
| 8 | `kill -9` Tauri | 记录孤儿（已知限制，不进验收门槛） |

自动化手段：`webview.eval()` 触发点击、读 `is_maximized()` / `is_visible()`；
CI 可用 `xvfb-run`（本机已有 Xvfb，当前也有 `DISPLAY=:1`）。devtools 在 debug 构建显式开 `devtools` feature。

## 9. 风险

- **Linux frameless**：没有原生边框就失去边缘 resize。v1 接受；后续用 8 个透明把手 +
  `start-resize-dragging` 权限补，或 Linux 上保留 `decorations: true`。
- **`data-tauri-drag-region` 在 Wayland**：依赖 WebKitGTK ≥ 2.40 的 startDragging，本机 2.44 可用。
- **pi-web 打包**：Next.js 需要 Node 运行时。实验不解决；生产候选是
  `externalBin` + 固定 Node 二进制，或 `bun build --compile` 出 standalone server。
- **端口归属**：固定端口时，任何本地进程都可能占用它并"继承"这三个窗口权限（危害面=操控本窗口）。
  生产用随机端口 + 运行时 capability 消除。

## 10. 下一步

1. 阶段 1：脚手架 + harness + 内置 window 命令（零 Rust command），跑通矩阵 1–6。
2. 阶段 2：改自定义 `pi_window` + `AppManifest`，对比"最小授权面"，跑通矩阵 4/7。
3. 阶段 3（另开实验）：换真实 pi-web sidecar + 标题栏组件 + 打包方案。
