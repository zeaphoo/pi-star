#!/usr/bin/env bash
# 构建 Windows 桌面应用：NSIS / MSI 安装包 + 便携 zip。
# 内嵌 pi-star.exe、Node、pi 包、agent-extension 和 MinGit；必须在 Git Bash / MSYS 下运行。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$("$ROOT/scripts/check-desktop-version.sh")"
DIST_DIR="${DIST_DIR:-$ROOT/dist/windows}"
RUNTIME_DIR="${PI_RUNTIME_DIR:-$ROOT/app/pi-runtime}"
BUNDLES="${TAURI_BUNDLES:-nsis,msi}"
INJECT_CONFIG="$ROOT/app/.bundle-resources.json"
APP_BIN="$ROOT/app/pi-star.exe"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *) echo "package-windows.sh 仅在 Windows（Git Bash / MSYS）上运行" >&2; exit 1 ;;
esac

cleanup() {
    rm -f "$INJECT_CONFIG" "$APP_BIN"
}
trap cleanup EXIT

for command in cargo node npm curl tar; do
    command -v "$command" >/dev/null 2>&1 || { echo "缺少命令: $command" >&2; exit 1; }
done
if ! cargo tauri --version >/dev/null 2>&1; then
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
fi

if [[ "${SKIP_UI_BUILD:-0}" != "1" ]]; then
    [[ -d "$ROOT/ui/node_modules" ]] || (cd "$ROOT/ui" && npm ci)
    (cd "$ROOT/ui" && PI_STAR_VERSION="$VERSION" npm run build)
fi
[[ -f "$ROOT/ui/dist/index.html" ]] || { echo "缺少 ui/dist/index.html" >&2; exit 1; }

case "$(uname -m)" in
    x86_64 | amd64) WIN_ARCH=x64 ;;
    aarch64 | arm64) WIN_ARCH=arm64 ;;
    *) echo "不支持的 Windows 架构: $(uname -m)" >&2; exit 1 ;;
esac

if [[ "${SKIP_PI_RUNTIME:-0}" != "1" ]]; then
    node "$ROOT/scripts/prepare-pi-runtime.mjs" --out "$RUNTIME_DIR" --platform win32 --arch "$WIN_ARCH"
fi
for path in "$RUNTIME_DIR/node.exe" "$RUNTIME_DIR/pi/package.json" "$RUNTIME_DIR/agent-extension/index.ts"; do
    [[ -e "$path" ]] || { echo "pi 运行时缺少 $path" >&2; exit 1; }
done

(cd "$ROOT" && cargo build --release -p pi-star)
target_dir() {
    (cd "$1" && cargo metadata --no-deps --format-version 1 |
        node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')
}
ROOT_TARGET="$(target_dir "$ROOT")"
cp -f "$ROOT_TARGET/release/pi-star.exe" "$APP_BIN"

MSI_VERSION=""
if [[ "$VERSION" == *-* ]]; then
    core="${VERSION%%-*}"
    pre="${VERSION#*-}"
    build=1
    if [[ "$pre" =~ [0-9]+ ]]; then
        build="$((10#${BASH_REMATCH[0]}))"
    fi
    (( build > 65535 )) && build=65535
    MSI_VERSION="${core}.${build}"
fi

node -e '
const fs = require("fs")
const [root, runtime, out, msi] = process.argv.slice(1)
const p = (value) => value.replace(/\\/g, "/")
const fsExists = (p) => fs.existsSync(p)
const resources = {
  [p(`${root}/app/pi-star.exe`)]: "pi-star.exe",
  [p(`${runtime}/node.exe`)]: "pi-runtime/node.exe",
  [p(`${runtime}/pi`)]: "pi-runtime/pi",
  [p(`${runtime}/agent-extension`)]: "pi-runtime/agent-extension",
  [p(`${runtime}/pi-runtime.json`)]: "pi-runtime/pi-runtime.json",
  [p(`${runtime}/NODE-LICENSE`)]: "pi-runtime/NODE-LICENSE",
  [p(`${runtime}/PI-NOTICE`)]: "pi-runtime/PI-NOTICE",
}
for (const extra of ["git", "GIT-LICENSE.txt", "GIT-NOTICE.txt"]) {
  if (fsExists(`${runtime}/${extra}`)) resources[p(`${runtime}/${extra}`)] = `pi-runtime/${extra}`
}
const config = { bundle: { resources } }
if (msi) config.bundle.windows = { wix: { version: msi } }
fs.writeFileSync(out, JSON.stringify(config))
' "$(cygpath -m "$ROOT")" "$(cygpath -m "$RUNTIME_DIR")" "$(cygpath -w "$INJECT_CONFIG")" "$MSI_VERSION"

(cd "$ROOT/app" && cargo tauri build --config "$(cygpath -w "$INJECT_CONFIG")" --bundles "$BUNDLES")
rm -f "$INJECT_CONFIG"

APP_TARGET="$(target_dir "$ROOT/app")"
APP_EXE="$APP_TARGET/release/PiStarApp.exe"
BUNDLE_DIR="$APP_TARGET/release/bundle"
[[ -f "$APP_EXE" ]] || { echo "未找到 $APP_EXE" >&2; exit 1; }

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/portable"

shopt -s nullglob
installers=("$BUNDLE_DIR"/nsis/*.exe "$BUNDLE_DIR"/msi/*.msi)
shopt -u nullglob
[[ ${#installers[@]} -gt 0 ]] || { echo "Tauri bundler 未产出安装包（$BUNDLE_DIR）" >&2; exit 1; }

for file in "${installers[@]}"; do
    case "$file" in
        *.exe) out="pi-star-app_${VERSION}_windows_${WIN_ARCH}-setup.exe" ;;
        *.msi)
            locale="${file##*_}"
            out="pi-star-app_${VERSION}_windows_${WIN_ARCH}_${locale}"
            ;;
        *) out="$(basename "$file")" ;;
    esac
    cp -f "$file" "$DIST_DIR/$out"
done

# 便携版：两个 exe + pi-runtime 必须在同一目录（Windows 大小写不敏感，
# PiStarApp.exe 与 pi-star.exe 仍不同）。
cp -f "$APP_EXE" "$DIST_DIR/portable/PiStarApp.exe"
cp -f "$ROOT_TARGET/release/pi-star.exe" "$DIST_DIR/portable/pi-star.exe"
cp -R "$RUNTIME_DIR" "$DIST_DIR/portable/pi-runtime"
cat > "$DIST_DIR/portable/README.txt" <<TXT
Pi Star ${VERSION} portable (${WIN_ARCH})
=========================================

运行 / Run
  双击 PiStarApp.exe。无需安装、无需管理员权限。
  桌面壳会启动同目录的 pi-star.exe，并在托盘常驻。
  Double-click PiStarApp.exe. No installer or admin rights required.

目录 / Layout
  PiStarApp.exe        桌面应用
  pi-star.exe          本地服务（必须与 PiStarApp.exe 同目录）
  pi-runtime/          内置 Node、pi CLI 与 pi-star 扩展
  README.txt

数据 / Data
  桌面偏好：%APPDATA%\\xyz.pi-star.desktop
  会话与设置：%USERPROFILE%\\.pi\\agent
  Back up %USERPROFILE%\\.pi\\agent to keep sessions and credentials.

命令执行 / Bash
  安装包已内置 MinGit（pi-runtime\git），无需另装 Git for Windows。
  The package bundles MinGit under pi-runtime\git; no separate Git install is needed.
  仍可用 PI_STAR_BASH 指向其他 bash.exe。


WebView2
  Windows 10 1803+ 通常已随 Edge 安装；缺失时请安装 Evergreen Bootstrapper。

未签名 / Unsigned
  安装包未做代码签名，SmartScreen 可能提示“未知发布者”。

退出 / Quit
  托盘菜单“退出”会同时停止 pi-star；只关窗口则留在托盘。
TXT

PORTABLE_WIN="$(cygpath -w "$DIST_DIR/portable")"
ZIP_WIN="$(cygpath -w "$DIST_DIR/pi-star-app_${VERSION}_windows_${WIN_ARCH}_portable.zip")"
powershell.exe -NoProfile -NonInteractive -Command \
    "Compress-Archive -Path '$PORTABLE_WIN\\*' -DestinationPath '$ZIP_WIN' -Force" >/dev/null

for name in PiStarApp.exe pi-star.exe pi-runtime/node.exe pi-runtime/pi/package.json pi-runtime/agent-extension/index.ts; do
    [[ -e "$DIST_DIR/portable/$name" ]] || { echo "便携包缺少 $name" >&2; exit 1; }
done

SEVEN_ZIP=""
if command -v 7z >/dev/null 2>&1; then
    SEVEN_ZIP=7z
elif [[ -x "/c/Program Files/7-Zip/7z.exe" ]]; then
    SEVEN_ZIP="/c/Program Files/7-Zip/7z.exe"
fi
for file in "$DIST_DIR"/*-setup.exe; do
    [[ -f "$file" ]] || continue
    if [[ -z "$SEVEN_ZIP" ]]; then
        echo "WARN: 未找到 7-Zip，跳过 $file 内容断言" >&2
        continue
    fi
    listing="$("$SEVEN_ZIP" l "$file" 2>/dev/null || true)"
    for name in PiStarApp.exe pi-star.exe pi-runtime/node.exe; do
        grep -qi "$name" <<<"$listing" || { echo "安装包 ${file##*/} 缺少 $name" >&2; exit 1; }
    done
done

echo "生成完成："
ls -lh "$DIST_DIR"
