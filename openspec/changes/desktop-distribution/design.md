# Design

## Context

`desktop-shell` 已经提供 Tauri 壳、进程监督和内置 UI。分发层仍未解决 Node/pi 依赖、Windows
bash、平台安装包和 CI。安装包必须做到“下载即用”，且不能把 Linux service 与桌面应用混在一起。

## Goals / Non-Goals

**Goals:**

- 安装包自包含 Node、pi CLI 生产依赖和 `agent-extension/`。
- 三平台使用同一套版本闸门和资源注入方式。
- CI 能发现 Windows/macOS 编译问题；Release 能一键产出全部安装包。
- Windows `bash` 工具稳定使用 Git for Windows，绝不回退 cmd.exe/WSL。

**Non-Goals:**

- 不把 pi 编译成单文件二进制；仍使用官方 Node + npm 包。
- Linux/macOS 不额外捆绑 Git（系统自带 bash）；Windows 为满足 bash 工具捆绑 MinGit。
- 不签名 Windows 安装包；macOS 签名/公证为可选 secrets。
- 不改变 Linux 服务端的独立运行方式，只是不再单独发布 service 包。

## Decisions

### 1. `pi-runtime/` 布局与固定版本

```text
pi-runtime/
  node(.exe)
  pi/                  npm pack + npm install --omit=dev
  agent-extension/     index.ts + subagent.ts
  git/                 Windows only: MinGit
  pi-runtime.json      { node, pi, git, platform, arch }
  NODE-LICENSE / PI-NOTICE / GIT-LICENSE.txt / GIT-NOTICE.txt
```

`packaging/pi-runtime.json` 固定 `node=22.20.0`、`pi=0.99.1`、`minGit=2.56.0.4`。`scripts/prepare-pi-runtime.mjs`
下载官方 Node（curl），`npm pack` pi 并从 tarball 安装生产依赖，复制扩展；元数据匹配时跳过，
供 GitHub Actions 缓存。 pi 的 `docs/`、`examples/` 保留，供系统提示引用。

### 2. pi-star 的运行时发现

`crates/pi-star/src/pi_runtime.rs` 按以下顺序找 `pi-runtime/`：

1. `PI_STAR_PI_RUNTIME`；
2. `pi-star` 可执行文件同目录（安装包布局）；
3. debug 构建的 `app/pi-runtime`、`dist/pi-runtime`。

找到后 `apply_env` 设置 `PI_STAR_PI_NODE`、`PI_STAR_PI_JS`、`PI_STAR_AGENT_EXTENSION`、
`PI_PACKAGE_DIR`；Windows 上还会设置 `PI_STAR_GIT_HOME`/`PI_STAR_BASH`，并把 MinGit 的
`cmd`、`ucrt64/bin`、`usr/bin`、`bin` 前置到 PATH。`PI_STAR_PI_CLI` 是显式逃生口，存在时完全不使用内置运行时。RPC 与 export
都通过 `pi_runtime::command()` 构造 `node <cli.js> ...`。

### 3. 打包脚本共享资源注入

三个平台脚本都：

1. 校验版本；
2. 准备 `app/pi-runtime`；
3. 构建 UI、release `pi-star`；
4. 把 `pi-star` 复制到 `app/`；
5. 用临时 `--config` 注入 `bundle.resources`（`pi-star`、`pi-runtime/node`、`pi-runtime/pi`、
   `pi-runtime/agent-extension`）；
6. 调用 `cargo tauri build`；
7. 清理临时资源。

Linux 产物 rename 为 `pi-star-app_*` 并重写 deb `Package:`；Windows `PiStarApp.exe` 与
`pi-star.exe` 同目录，另出 portable zip；macOS 资源落在 `Contents/Resources`，签名/公证由
`package-mac.sh` + `notarize-mac.sh` 处理。

### 4. macOS 签名顺序

先签 `Contents` 下除主程序外的所有 Mach-O（Node、pi-star、`.node` 原生模块），Node 使用
`packaging/macos/entitlements.plist` 的 JIT/unsigned-memory/library-validation 例外；再签
`.app` 外层。公证流程：app zip → notarytool → staple app → 重建 dmg → notarytool → staple dmg →
`spctl` 验收。无 secrets 时跳过并明确告警。

### 5. CI / Release

- `ci.yml`：Rust workspace + app 单测/格式、UI 类型/单测/构建、三平台 `cargo check` + app 测试。
- `release.yml`：`v*` 或手动触发；verify 校验版本/tag 与 `CHANGELOG.md` 版本段落；
  Linux/Windows/macOS 矩阵构建；macOS 可选导入证书并公证；`scripts/release-notes.sh` 抽取
  CHANGELOG 作为 Release 正文，`softprops/action-gh-release` 汇总 artifacts。
- 缓存：Rust 按 workspace，`app/pi-runtime` 按平台 + `packaging/pi-runtime.json` 哈希。

### 6. Windows bash

对齐 PI-Desktop（见 `crates/pi-star-tools/src/os_env.rs`）：

1. `with_shell_path`；
2. 安装包内置 `pi-runtime/git`（或 `PI_STAR_GIT_HOME`）；
3. `PI_STAR_BASH`（兼容 `PI_DESKTOP_BASH`）；
4. 从 PATH 上的 `git.exe` 推导 Git 根目录，查 `bin/bash.exe`、`usr/bin/bash.exe`；
5. `ProgramFiles`/`ProgramFiles(x86)`/`ProgramW6432`/`LocalAppData\Programs`；
6. `where bash.exe`，排除 `System32`/`Sysnative` 的 WSL 启动器。

Git Bash 使用 `--noprofile --norc -c`；WSL bash 保留 `-s` stdin 传输；缺失时返回
`ShellUnavailable`，不静默回退 cmd.exe。

## Risks / Trade-offs

- 安装包变大（deb ~90 MB、AppImage ~160 MB）→ 自包含是明确目标；Node/pi 缓存可缩短构建。
- `npm install` 会按构建机安装可选原生依赖 → 三平台各自原生 runner 构建，不做交叉编译。
- Node 在 hardened runtime 下需要 JIT entitlements → 对内置 Node/native 模块签名时带
  entitlements。
- macOS 无 secrets 时未签名 → Release 仍出 dmg 并告警；配置 secrets 后自动公证。
- MinGit 不提供 `bash.exe`，但 `usr/bin/sh.exe` 与 bash 字节相同 → 准备脚本复制为 `bash.exe`，解压约 96 MB（PortableGit 为 416 MB）。
- GPL-2.0 合规 → 安装包附带 Git 许可证与源码地址（`GIT-LICENSE.txt`、`GIT-NOTICE.txt`）。
- 未签名 Windows 安装包触发 SmartScreen → README 说明。

## Migration Plan

1. 更新 `packaging/pi-runtime.json` 并运行 `prepare-pi-runtime`；
2. 本地各平台跑 `package-*` 验证；
3. 配置 macOS secrets（可选）；
4. 打 `v*` 标签触发 Release；
5. 回滚：删除 `pi-runtime` 注入即可恢复“依赖系统 pi/Node”的旧行为。
