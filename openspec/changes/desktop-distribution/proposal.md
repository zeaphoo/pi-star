# Proposal

## Why

桌面安装包不能要求用户预装 Node、pi CLI 或 Git Bash。当前 `app/` 只携带 `pi-star` 二进制，
`rpc` 后端仍依赖 PATH 上的 `pi`，Windows 的命令执行依赖不稳定的 bash 探测；同时缺少可复现的
Windows/macOS 安装包和 CI/Release 流水线。

## What Changes

- **自包含运行时**：安装包携带官方 Node、固定版本的 `@earendil-works/pi-coding-agent` 生产依赖，
  以及 `agent-extension/`；`pi-star` 从 `pi-runtime/` 自动发现它们。
- **平台打包**：新增 Linux（deb/AppImage）、Windows（NSIS/MSI/portable zip）、macOS（.app/dmg）
  脚本；全部是桌面应用，不再发布 Linux service 包。
- **macOS 签名/公证**：有 secrets 时对内置 Node/原生模块先签名，再签 `.app`，公证 app 与 dmg；
  无 secrets 时产出未签名 dmg。
- **CI / Release**：新增 GitHub Actions，CI 在 Linux/Windows/macOS 编译检查并跑测试；Release
  由 `v*` 触发，汇总三平台产物并创建 GitHub Release。
- **Windows bash**：按 PI-Desktop 的策略探测 Git for Windows（`PI_STAR_BASH` → git.exe 推导 →
  标准安装目录 → `where bash.exe` 排除 WSL），使用 `--noprofile --norc -c`。
- **版本固定**：`packaging/pi-runtime.json` 固定 Node/pi 版本；发布 tag 必须与三个版本清单一致。

## Capabilities

### New Capabilities

- `desktop-distribution`: 自包含运行时、平台安装包、CI/Release 与 Windows Git Bash 的执行契约。

### Modified Capabilities

<!-- 桌面运行/壳行为由已完成的 desktop-shell 变更定义。 -->

## Impact

- 新增 `pi-runtime` 运行时 staging、三个平台打包脚本、macOS entitlements/公证脚本、GitHub Actions。
- `pi-star` 新增 `pi-runtime` 自动发现，并让 RPC/export 通过内置 Node + pi 启动。
- `pi-star-tools` 的 Windows bash 探测对齐 PI-Desktop。
- 安装包体积显著增大（Node + pi 生产依赖）；构建需要网络下载固定版本运行时。
