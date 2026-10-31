# 桌面打包

Pi Star 只发布**桌面应用**，不再提供单独的 Linux 服务包。每个安装包都包含：

| 内容 | 位置 |
| --- | --- |
| 桌面壳 | `PiStarApp` / `PiStarApp.exe` / `PiStar.app` |
| pi-star 服务 | 资源目录下的 `pi-star`（Windows 与壳同目录） |
| Node 运行时 | `pi-runtime/node`（Windows `node.exe`） |
| pi CLI | `pi-runtime/pi/`（`@earendil-works/pi-coding-agent` + 生产依赖） |
| pi-star 扩展 | `pi-runtime/agent-extension/`（`index.ts`、`subagent.ts`） |
| MinGit（仅 Windows） | `pi-runtime/git/`（Git Bash、git、coreutils） |

Linux 安装位置为 `/usr/lib/PiStar/`，macOS 为 `Contents/Resources/`，Windows 为安装目录。
`pi-star` 启动时会在自身同目录查找 `pi-runtime/`，因此安装后无需单独安装 Node 或 pi。

## 版本

`packaging/pi-runtime.json` 固定内置运行时：

```json
{
  "node": "22.20.0",
  "pi": "0.99.1",
  "minGit": "2.56.0.4",
  "minGitTag": "v2.56.0.windows.4"
}
```

升级 pi/Node/MinGit 时同时更新此文件并重新打包。Windows 目标机器不需要预装 Git for
Windows；`pi-star` 会使用 `pi-runtime/git/bin/bash.exe`，并把 `git`/`cmd`/`usr/bin` 等目录
加入子进程 PATH。`scripts/check-desktop-version.sh` 校验
`app/Cargo.toml`、`app/tauri.conf.json` 与 workspace 版本；发布 tag 必须是 `v<version>`。

## 本机构建

```bash
./scripts/check-desktop-version.sh
./scripts/package-desktop.sh              # 当前平台
# 或分别执行：
./scripts/package-linux-app.sh            # deb + AppImage
./scripts/package-windows.sh              # NSIS + MSI + portable zip（Git Bash）
./scripts/package-mac.sh                  # .app + dmg
```

常用环境变量：

- `SKIP_UI_BUILD=1`：复用已有 `ui/dist`。
- `SKIP_PI_RUNTIME=1`：复用 `app/pi-runtime`（必须与当前平台/架构匹配）。
- `PI_RUNTIME_DIR=...`：自定义运行时 staging 目录。
- `TAURI_BUNDLES=deb` / `nsis`：只出部分安装包。
- `DIST_DIR=...`：自定义输出目录。
- `MAC_SIGN_IDENTITY=...`：macOS 签名身份。

运行时准备脚本也可以单独执行：

```bash
node scripts/prepare-pi-runtime.mjs --out app/pi-runtime --platform linux --arch x64
node scripts/prepare-pi-runtime.mjs --out app/pi-runtime --platform win32 --arch x64
node scripts/prepare-pi-runtime.mjs --out app/pi-runtime --platform darwin --arch arm64
```

它从 nodejs.org 下载固定版本 Node，`npm pack` 固定版本 pi，`npm install --omit=dev`
安装生产依赖，复制 `agent-extension/`；Windows 上还会解压 MinGit。已准备且元数据一致时
自动跳过。MinGit 采用 GPL-2.0，安装包内附带 `GIT-LICENSE.txt`/`GIT-NOTICE.txt`，源码地址
为 <https://github.com/git-for-windows/git>。

## 产物命名

| 平台 | 产物 |
| --- | --- |
| Linux | `pi-star-app_<ver>_linux_<arch>.deb`、`.AppImage` |
| Windows | `pi-star-app_<ver>_windows_<arch>-setup.exe`、`..._<locale>.msi`、`..._portable.zip` |
| macOS | `pi-star-app_<ver>_macos_<arch>.dmg` |

Windows 主程序是 `PiStarApp.exe`，与 `pi-star.exe` 在忽略大小写后仍不同。便携 zip 内两个 exe
与 `pi-runtime/` 同目录。`pi/node_modules` 路径较深，Windows 构建机建议启用 long paths
（`git config --global core.longpaths true` 与注册表 `LongPathsEnabled=1`）；CI 已自动开启。

## CI / Release

`.github/workflows/ci.yml` 在 Linux 跑 Rust workspace + 桌面壳测试、UI 测试/构建，并在
Linux/Windows/macOS 做桌面壳编译检查。

`.github/workflows/release.yml` 由 `v*` 标签触发（也可手动触发）：

1. 校验 tag 与版本清单；
2. Linux 产出 deb + AppImage；
3. Windows 产出 NSIS、MSI 与 portable zip；
4. macOS 分别在 Intel/Apple Silicon runner 产出 dmg；
5. 汇总并创建 GitHub Release（手动触发时 `publish=false` 只上传 artifacts）。

### macOS 签名与公证（可选）

未配置 secrets 时构建**未签名** dmg。配置以下 secrets 后自动签名、公证并钉票：

| secret | 说明 |
| --- | --- |
| `MAC_CERT_P12_BASE64` | Developer ID Application 证书 + 完整链 |
| `MAC_CERT_PASSWORD` | p12 口令 |
| `MAC_SIGN_IDENTITY` | 签名身份全名 |
| `MAC_NOTARY_API_KEY_ID` | App Store Connect API key id |
| `MAC_NOTARY_API_ISSUER_ID` | issuer id |
| `MAC_NOTARY_API_KEY_P8_BASE64` | .p8 私钥（base64） |

签名时会给内置 Node 加 JIT/unsigned-memory entitlements（见
`packaging/macos/entitlements.plist`），并对 `pi-runtime` 下所有 Mach-O 先签名，再签外层
`.app`；公证脚本会公证 app、钉票、重建 dmg、再公证 dmg。

## Windows 的 bash

Agent 的 `bash` 工具在 Windows 上优先使用安装包内置的 MinGit，不会回退到 `cmd.exe`，
也不会使用 WSL 的 `System32\bash.exe`。探测顺序：

1. 内置 `pi-runtime/git/bin/bash.exe`（或 `PI_STAR_GIT_HOME`）；
2. `PI_STAR_BASH`，兼容 `PI_DESKTOP_BASH`；
3. 从 PATH 上的 `git.exe` 推导 `Git\bin\bash.exe` / `Git\usr\bin\bash.exe`；
4. `ProgramFiles`、`ProgramFiles(x86)`、`ProgramW6432`、`LocalAppData\Programs` 下的
   `Git\bin\bash.exe`；
5. `where bash.exe`，排除 `System32` 的 WSL 启动器。

找到后使用 `bash --noprofile --norc -c`，并以 `CREATE_NO_WINDOW` 启动。终端面板仍默认使用
PowerShell（可用 `SHELL` 覆盖）。
