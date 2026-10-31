# Tasks

## 1. 自包含运行时

- [x] 1.1 新增 `packaging/pi-runtime.json` 固定 Node/pi/MinGit 版本，并写 `scripts/prepare-pi-runtime.mjs` 下载 Node、npm pack/install pi、复制 `agent-extension`、解压 Windows MinGit
- [x] 1.2 新增 `crates/pi-star/src/pi_runtime.rs`：运行时发现、环境注入、RPC/export 命令构造；补布局解析单测
- [x] 1.3 在 `main` 启动时调用 `apply_env`，并验证 `PI_STAR_PI_RUNTIME` 能驱动内置 pi（Linux 打包产物实测）
- [x] 1.4 Windows 安装包内置 MinGit：准备脚本解压/校验，`apply_env` 设置 `PI_STAR_GIT_HOME`/`PI_STAR_BASH` 并前置 PATH

## 2. 平台打包

- [x] 2.1 重写 `package-linux-app.sh`：注入 pi-star + 运行时、重写 deb 包名、输出 `dist/linux-app`；实测 deb/AppImage
- [x] 2.2 新增 `package-windows.sh`：NSIS/MSI、MSI 预发布版本映射、portable zip 与内容断言
- [x] 2.3 新增 `package-mac.sh`：资源布局、Mach-O 内层签名（Node entitlements）、dmg
- [x] 2.4 新增 `notarize-mac.sh`：app/dmg 双公证、钉票、spctl 验收
- [x] 2.5 `package-desktop.sh` 改为平台分发器；移除 Linux service 打包路径

## 3. CI / Release

- [x] 3.1 新增 `.github/workflows/ci.yml`：Rust workspace/app、UI、三平台编译检查
- [x] 3.2 新增 `.github/workflows/release.yml`：版本/tag 闸门、三平台矩阵、artifact 汇总与 Release
- [x] 3.3 `check-desktop-version.sh` 支持 tag 校验；Release verify 使用该脚本
- [x] 3.4 新增标准 `CHANGELOG.md` 与 `scripts/release-notes.sh`；Release verify 校验版本段落，正文从 CHANGELOG 抽取

## 4. Windows bash

- [x] 4.1 `os_env.rs` 增加 `PI_STAR_BASH`/`PI_DESKTOP_BASH` 覆盖
- [x] 4.2 优先使用内置 MinGit，其次按 PI-Desktop 顺序探测系统 Git，排除 WSL，使用 `--noprofile --norc -c`
- [x] 4.3 补 WSL 路径识别单测

## 5. 文档与验收

- [x] 5.1 更新 README、app/README、docs/status 与 `packaging/README.md`
- [ ] 5.2 在 Windows/macOS 实机各打一次安装包并安装启动
- [ ] 5.3 配置 macOS secrets 后跑一次签名/公证 Release 演练
- [ ] 5.4 跑一次完整 Release workflow（可 publish=false）

## Workflow follow-up

- 归档 `desktop-distribution` 并把 delta 合入主 specs。
