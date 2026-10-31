# Spec Delta

## Purpose

定义 Pi Star 桌面安装包的自包含运行时、三平台产物、CI/Release 流程，以及 Windows 上使用
Git for Windows 执行 bash 命令的行为。

## ADDED Requirements

### Requirement: 自包含 pi 运行时

桌面安装包 SHALL 携带官方 Node 运行时、固定版本的 pi CLI 生产依赖和 pi-star 的
`agent-extension/`，安装后无需用户预装 Node 或 pi。运行时版本 MUST 由仓库内固定清单声明。

#### Scenario: 干净机器启动

- **WHEN** 用户安装桌面包且机器没有 Node/pi
- **THEN** pi-star 使用内置 `pi-runtime/` 启动 RPC 后端，Agent 页面可用

#### Scenario: 版本固定

- **WHEN** 打包脚本运行
- **THEN** 下载/安装清单中的 Node 与 pi 版本，并写入 `pi-runtime.json`

### Requirement: 运行时发现与覆盖

pi-star SHALL 从 `PI_STAR_PI_RUNTIME`、可执行文件同目录的 `pi-runtime/` 或 debug 构建目录发现
内置运行时；发现后设置 Node、CLI、扩展和包目录环境变量。`PI_STAR_PI_CLI` 存在时 MUST 优先使用
用户指定的 pi。

#### Scenario: 安装包布局

- **WHEN** `pi-star` 与 `pi-runtime/` 位于同一资源目录
- **THEN** RPC 和 export 使用内置 Node 执行 `pi/dist/bundle/cli.js`

#### Scenario: 用户覆盖

- **WHEN** 设置 `PI_STAR_PI_CLI=/opt/pi`
- **THEN** 不注入内置 Node/CLI，直接执行该命令

### Requirement: 三平台桌面产物

打包 SHALL 产出 Linux deb/AppImage、Windows NSIS/MSI/portable zip、macOS dmg；产物 MUST 包含
桌面壳、`pi-star` 和 `pi-runtime/`。Linux MUST NOT 再发布独立的 service 包。

#### Scenario: Linux 安装包

- **WHEN** 安装 deb
- **THEN** `/usr/lib/PiStar/` 同时存在 `pi-star` 与 `pi-runtime/`，桌面应用可直接启动

#### Scenario: Windows 便携包

- **WHEN** 解压 portable zip
- **THEN** `PiStarApp.exe`、`pi-star.exe`、`pi-runtime/` 同目录

#### Scenario: macOS 安装镜像

- **WHEN** 打开 dmg
- **THEN** `PiStar.app` 内含 `Contents/Resources/pi-star` 与 `pi-runtime/`

### Requirement: CI 与 Release

CI SHALL 在 Linux/Windows/macOS 上校验 Rust、桌面壳与 UI；Release SHALL 由 `v*` 标签或手动触发，
构建三平台产物并上传 GitHub Release。发布 tag MUST 与版本清单一致。

#### Scenario: 标签发布

- **WHEN** 推送 tag `v0.1.0`
- **THEN** 三个平台构建成功并把安装包挂到同名 Release

#### Scenario: 版本不一致

- **WHEN** tag 与 `app/tauri.conf.json` 版本不同
- **THEN** verify job 失败，不构建任何产物

### Requirement: macOS 签名与公证

配置签名/公证 secrets 时，macOS 构建 SHALL 先签内置 Mach-O 再签 `.app`，并公证/钉票 app 与
dmg；未配置时 SHOULD 产出未签名 dmg 并明确告警。

#### Scenario: 有 secrets

- **WHEN** 证书与 App Store Connect key 可用
- **THEN** dmg 通过 `codesign --verify`、`stapler validate` 与 `spctl`

#### Scenario: 无 secrets

- **WHEN** 仓库未配置 macOS secrets
- **THEN** 仍产出 dmg，日志提示未签名

### Requirement: Windows Git Bash 探测

Windows 的 bash 工具 SHALL 使用 Git for Windows 的 `bash.exe`，支持 `PI_STAR_BASH` 覆盖；探测
MUST 排除 `System32`/`Sysnative` 的 WSL 启动器，且 MUST NOT 回退到 cmd.exe。

#### Scenario: 标准 Git 安装

- **WHEN** Git 安装在 `Program Files\Git`
- **THEN** bash 工具使用 `Git\bin\bash.exe` 执行命令

#### Scenario: 只有 WSL bash

- **WHEN** PATH 上只有 `C:\Windows\System32\bash.exe`
- **THEN** 返回 `ShellUnavailable`，提示安装 Git for Windows
