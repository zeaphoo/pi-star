#!/usr/bin/env bash
# 构建 Pi Star 桌面安装包（当前平台）。
#
# 顺序：UI → 内嵌 UI 的 pi-star release → Tauri 安装包。
# app/tauri.conf.json 不声明资源；这里把 pi-star 复制到 app/ 并用临时 --config 注入，
# 避免普通 cargo build/test 依赖打包产物。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$("$ROOT/scripts/check-desktop-version.sh")"
INJECT_CONFIG="$ROOT/app/.bundle-resources.json"
APP_BIN="$ROOT/app/pi-star"
TMP_BINARY=""

cleanup() {
    rm -f "$INJECT_CONFIG" "$APP_BIN" "$APP_BIN.exe" "$TMP_BINARY"
}
trap cleanup EXIT

for command in cargo node npm; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "缺少命令: $command" >&2
        exit 1
    fi
done
if ! cargo tauri --version >/dev/null 2>&1; then
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
fi

case "$(uname -s)" in
    Linux*)                       OS=linux;   BUNDLES="${TAURI_BUNDLES:-deb,appimage}"; BIN_NAME=pi-star ;;
    Darwin*)                      OS=macos;   BUNDLES="${TAURI_BUNDLES:-app}";        BIN_NAME=pi-star ;;
    MINGW* | MSYS* | CYGWIN*)     OS=windows; BUNDLES="${TAURI_BUNDLES:-nsis,msi}";   BIN_NAME=pi-star.exe ;;
    *) echo "不支持的平台: $(uname -s)" >&2; exit 1 ;;
esac

if [[ "${SKIP_UI_BUILD:-0}" != "1" ]]; then
    if [[ ! -d "$ROOT/ui/node_modules" ]]; then
        (cd "$ROOT/ui" && npm ci)
    fi
    (cd "$ROOT/ui" && PI_STAR_VERSION="$VERSION" npm run build)
fi
if [[ ! -f "$ROOT/ui/dist/index.html" ]]; then
    echo "缺少 ui/dist/index.html；先执行 cd ui && npm run build" >&2
    exit 1
fi

(cd "$ROOT" && cargo build --release -p pi-star)

target_dir() {
    (cd "$1" && cargo metadata --no-deps --format-version 1 |
        node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')
}

ROOT_TARGET="$(target_dir "$ROOT")"
RELEASE_BIN="$ROOT_TARGET/release/$BIN_NAME"
if [[ ! -f "$RELEASE_BIN" ]]; then
    echo "未找到 $RELEASE_BIN" >&2
    exit 1
fi

cp -f "$RELEASE_BIN" "$APP_BIN"

# Tauri bundler 对资源源路径使用绝对路径；Windows 下转成 Windows 形式。
RESOURCE_SRC="$APP_BIN"
if [[ "$OS" == windows ]]; then
    RESOURCE_SRC="$(cygpath -w "$APP_BIN")"
fi

node -e '
const fs = require("fs")
const [src, dest, out] = process.argv.slice(1)
fs.writeFileSync(out, JSON.stringify({ bundle: { resources: { [src]: dest } } }))
' "$RESOURCE_SRC" "$BIN_NAME" "$INJECT_CONFIG"

if [[ "$OS" == linux ]]; then
    # linuxdeploy 是 AppImage；无 FUSE 的环境需要自解压运行。
    export APPIMAGE_EXTRACT_AND_RUN=1
    rm -rf "$(target_dir "$ROOT/app")/release/bundle/appimage" \
           "$(target_dir "$ROOT/app")/release/bundle/appimage_deb"
fi

(cd "$ROOT/app" && cargo tauri build --config "$INJECT_CONFIG" --bundles "$BUNDLES")

BUNDLE_DIR="$(target_dir "$ROOT/app")/release/bundle"
echo "构建完成：$VERSION ($OS)"
echo "安装包目录：$BUNDLE_DIR"
