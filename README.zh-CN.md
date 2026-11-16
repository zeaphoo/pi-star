# Pi Star（pi-star）

[English](README.md) · **简体中文**

**基于 code agent 的一体化编码桌面应用。** 指挥 Agent、读代码、审 diff、跑终端，全程不用离开窗口。

pi-star 把一次编码回合需要的几件事放进同一个跨平台桌面应用：一边让 Agent 改代码，一边在
同一窗口里打开文件、查看 Git diff、运行和调试。它沿用 [pi coding agent](https://github.com/earendil-works/pi)
的会话与工具生态，并继续使用 `~/.pi/agent` 下的数据——和 pi CLI、pi-web 可以读取同一批会话。

## 特性

### 1. 一体化：Agent、代码、diff、终端都在同一个窗口

pi-star 不是把几个工具并排放在一起，而是把一次完整的编码回合收进一个应用：

| 能力 | 在 pi-star 里 |
| --- | --- |
| 指挥 Agent | 会话与分支树、队列/打断/重试、子代理、工具调用与思考过程、会话压缩；输入区直接切换模型、思考等级、工具与后端 |
| 代码浏览器 | 项目文件树、多标签文件视图、语法高亮、@ 文件索引、搜索；Agent 刚改过的文件可以当场打开对照 |
| Git diff 查看器 | 工作区与暂存改动分开查看、逐文件 diff、行内高亮与增删统计；review Agent 产出和提交前自查都在这里 |
| 终端 | 内置 PTY 终端，运行、测试、调试都在应用内；主题/字体/字号/回滚可调，与 Agent 共享 cwd 和进程环境 |
| 轨迹视图 | 把整条会话压成可筛选的事件时间线，长会话也能快速看清 Agent 做过什么 |

所以不需要在编辑器、终端、浏览器和 git 客户端之间来回切换，也不需要把 Agent 的输出复制到别处
再看——这是 pi-star 与「agent + 外部编辑器」组合最本质的区别。

### 2. 两个 Agent 引擎：原生 PI 与 Rust PI Agent

| 引擎 | 实现 | 说明 |
| --- | --- | --- |
| **原生 PI（Node.js）** | `pi --mode rpc` 子进程，与 pi CLI 同源 | 默认；兼容 TypeScript 扩展、MCP、subagent 等完整生态 |
| **Rust PI Agent（QuickPI）** | pi 的 Rust 实现（源码融合自 [pi-rust](https://github.com/bigfish1913/pi-rust)），在进程内运行 | 无需 Node，启动快、占用低；内置工具、MCP、subagent 与 skill 加载 |

后端按会话记录在会话文件的 custom entry 中，空闲时可切换、运行中锁定；两个引擎共享同一批会话
文件与配置。

### 3. 界面：向 pi-web 学习，为桌面重新设计

UI 参考并学习了 [pi-web](https://github.com/agegr/pi-web)（MIT），但按桌面应用的使用场景重新
设计：更简洁的层级、更少的视觉干扰，把注意力留给代码和对话。

## 下载

安装包发布在 [GitHub Releases](https://github.com/zeaphoo/pi-star/releases)。只发布桌面应用，
每个安装包都自带 Node、pi CLI 和（Windows）MinGit，无需预装 Node、pi 或 Git。

| 平台 | 安装包 |
| --- | --- |
| Linux | `pi-star-app_<版本>_linux_<arch>.deb`（Debian/Ubuntu）、`....AppImage` |
| Windows | `pi-star-app_<版本>_windows_x64-setup.exe`（NSIS，推荐）、`....msi`、`..._portable.zip` |
| macOS | `pi-star-app_<版本>_macos_<arch>.dmg`（Intel `x86_64` / Apple Silicon `arm64`） |

系统要求：

- **Linux**：x86_64 或 arm64，需要 WebKitGTK 4.1（Ubuntu 22.04+、Debian 12+ 等提供；deb 会
  自动拉取依赖）。
- **Windows**：Windows 10 / 11（x64），需要 WebView2 运行时（通常随 Edge 已安装；缺失时安装程序会自动下载，也可以使用便携版）。
- **macOS**：macOS 11+，Intel 或 Apple Silicon，使用系统自带的 WebKit。

## 安装

### Linux

deb（推荐，自动处理依赖和桌面图标）：

```bash
sudo apt install ./pi-star-app_<版本>_linux_amd64.deb
```

AppImage（免安装）：

```bash
chmod +x pi-star-app_<版本>_linux_amd64.AppImage
./pi-star-app_<版本>_linux_amd64.AppImage
```

安装后从应用菜单启动 **PiStar**；AppImage 双击运行即可。

### Windows

- **安装版**：运行 `pi-star-app_<版本>_windows_x64-setup.exe`，按向导完成安装（默认安装到
  Program Files，需要管理员权限）。也可以使用 `.msi`（适合企业分发/静默安装）。
- **便携版**：解压 `..._portable.zip`，保持 `PiStarApp.exe`、`pi-star.exe` 与 `pi-runtime/`
  在同一目录，双击 `PiStarApp.exe`；无需安装、无需管理员权限。
- 内置 MinGit 已提供 Git 和 bash，不需要另外安装 Git for Windows。
- 安装包目前未做代码签名，SmartScreen 可能提示「未知发布者」：点「更多信息」→「仍要运行」。

### macOS

打开 `pi-star-app_<版本>_macos_<arch>.dmg`，把 **PiStar** 拖进「应用程序」。

官方安装包已用 Apple 开发者证书签名并完成公证，首次启动无需任何额外设置。

## 首次启动

- 桌面壳会启动内置的本地服务并加载界面，端口固定为 `127.0.0.1:30151`（只监听本机）。
- 刚从命令行/其他工具迁移时，选择已有项目目录即可看到原来的 `~/.pi/agent` 会话。
- 关闭窗口默认收进托盘；托盘菜单可以退出、开机自启动和「关闭时最小化到托盘」。
- 启动后会在后台检查 GitHub Releases，有新版本时在界面里提示并链接到发布页，不会自动安装。
  升级时直接覆盖安装新版本即可。
- Linux 桌面数据（偏好、日志）在 `~/.local/share/net.quadhub.pi-star/`，macOS 在
  `~/Library/Application Support/net.quadhub.pi-star/`，Windows 在
  `%APPDATA%\net.quadhub.pi-star`；会话与凭据仍在 `~/.pi/agent`（`PI_CODING_AGENT_DIR`
  可覆盖）。

卸载应用不会删除 `~/.pi/agent` 和桌面偏好；需要彻底清理时手动删除上述目录。

## 更多文档

| 文档 | 内容 |
| --- | --- |
| [`docs/development.zh-CN.md`](docs/development.zh-CN.md) | 架构、从源码构建与运行、开发/测试、桌面打包、安全模型 |
| [`docs/status.zh-CN.md`](docs/status.zh-CN.md) | 实现状态与运行时边界 |
| [`CHANGELOG.md`](CHANGELOG.md) | 版本变更 |
| [`packaging/README.zh-CN.md`](packaging/README.zh-CN.md) | 安装包内容、运行时版本与打包细节 |

## 许可

pi-star 以 **Apache License 2.0** 发布，全文见 [`LICENSE`](LICENSE)。从 pi-web 与 pi-rust
复制的代码继续按 MIT 分发，上游署名、MIT 全文与字体许可见 [`NOTICE`](NOTICE)。
