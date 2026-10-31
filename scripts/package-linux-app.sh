#!/usr/bin/env bash
# 构建 Linux 桌面应用（deb + AppImage），并内嵌 pi-star、pi 与 agent-extension。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$("$ROOT/scripts/check-desktop-version.sh")"
DIST_DIR="${DIST_DIR:-$ROOT/dist/linux-app}"
RUNTIME_DIR="${PI_RUNTIME_DIR:-$ROOT/app/pi-runtime}"
INJECT_CONFIG="$ROOT/app/.bundle-resources.json"
APP_BIN="$ROOT/app/pi-star"

cleanup() {
    rm -f "$INJECT_CONFIG" "$APP_BIN"
}
trap cleanup EXIT

for command in cargo node npm curl tar dpkg-deb; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "缺少命令: $command" >&2
        exit 1
    fi
done
if ! cargo tauri --version >/dev/null 2>&1; then
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
fi

if [[ "${SKIP_UI_BUILD:-0}" != "1" ]]; then
    if [[ ! -d "$ROOT/ui/node_modules" ]]; then
        (cd "$ROOT/ui" && npm ci)
    fi
    (cd "$ROOT/ui" && PI_STAR_VERSION="$VERSION" npm run build)
fi
[[ -f "$ROOT/ui/dist/index.html" ]] || { echo "缺少 ui/dist/index.html" >&2; exit 1; }

if [[ "${SKIP_PI_RUNTIME:-0}" != "1" ]]; then
    node "$ROOT/scripts/prepare-pi-runtime.mjs" --out "$RUNTIME_DIR" --platform linux --arch "$(uname -m | sed 's/x86_64/x64/;s/aarch64/arm64/')"
fi
for path in "$RUNTIME_DIR/node" "$RUNTIME_DIR/pi/package.json" "$RUNTIME_DIR/agent-extension/index.ts"; do
    [[ -e "$path" ]] || { echo "pi 运行时缺少 $path；先执行 scripts/prepare-pi-runtime.mjs" >&2; exit 1; }
done

(cd "$ROOT" && cargo build --release -p pi-star)
target_dir() {
    (cd "$1" && cargo metadata --no-deps --format-version 1 |
        node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')
}
ROOT_TARGET="$(target_dir "$ROOT")"
cp -f "$ROOT_TARGET/release/pi-star" "$APP_BIN"

node -e '
const fs = require("fs")
const [root, runtime, out] = process.argv.slice(1)
const resources = {
  [`${root}/app/pi-star`]: "pi-star",
  [`${runtime}/node`]: "pi-runtime/node",
  [`${runtime}/pi`]: "pi-runtime/pi",
  [`${runtime}/agent-extension`]: "pi-runtime/agent-extension",
  [`${runtime}/pi-runtime.json`]: "pi-runtime/pi-runtime.json",
  [`${runtime}/NODE-LICENSE`]: "pi-runtime/NODE-LICENSE",
  [`${runtime}/PI-NOTICE`]: "pi-runtime/PI-NOTICE",
}
fs.writeFileSync(out, JSON.stringify({ bundle: { resources } }))
' "$ROOT" "$RUNTIME_DIR" "$INJECT_CONFIG"

export APPIMAGE_EXTRACT_AND_RUN=1
APP_TARGET="$(target_dir "$ROOT/app")"
BUNDLE_DIR="$APP_TARGET/release/bundle"
rm -rf "$BUNDLE_DIR/appimage" "$BUNDLE_DIR/appimage_deb" "$DIST_DIR"
mkdir -p "$DIST_DIR"

(cd "$ROOT/app" && cargo tauri build --config "$INJECT_CONFIG" --bundles deb,appimage)
rm -f "$INJECT_CONFIG"

case "$(uname -m)" in
    x86_64) LINUX_ARCH=amd64 ;;
    aarch64 | arm64) LINUX_ARCH=arm64 ;;
    *) LINUX_ARCH="$(uname -m)" ;;
esac

shopt -s nullglob
debs=("$BUNDLE_DIR"/deb/*.deb)
appimages=("$BUNDLE_DIR"/appimage/*.AppImage)
shopt -u nullglob
[[ ${#debs[@]} -gt 0 && ${#appimages[@]} -gt 0 ]] || {
    echo "Tauri bundler 未产出 deb/AppImage（$BUNDLE_DIR）" >&2
    exit 1
}

for file in "${appimages[@]}"; do
    cp -f "$file" "$DIST_DIR/pi-star-app_${VERSION}_linux_${LINUX_ARCH}.AppImage"
done

# Tauri 的包名来自 productName（PiStar → pi-star）。统一改成 pi-star-app，
# 避免和独立服务包同名；本仓库不发布服务包。
for file in "${debs[@]}"; do
    version="$(dpkg-deb -f "$file" Version)"
    arch="$(dpkg-deb -f "$file" Architecture)"
    work="$(mktemp -d)"
    dpkg-deb -R "$file" "$work"
    sed -i 's/^Package: .*/Package: pi-star-app/' "$work/DEBIAN/control"
    out="$DIST_DIR/pi-star-app_${version}_linux_${arch}.deb"
    dpkg-deb --root-owner-group -b "$work" "$out" >/dev/null
    rm -rf "$work"
    pkg="$(dpkg-deb -f "$out" Package)"
    [[ "$pkg" == "pi-star-app" ]] || { echo "deb 包名错误：$pkg" >&2; exit 1; }
    listing="$(dpkg-deb -c "$out")"
    for path in usr/lib/PiStar/pi-star usr/lib/PiStar/pi-runtime/node usr/lib/PiStar/pi-runtime/pi/package.json usr/lib/PiStar/pi-runtime/agent-extension/index.ts; do
        grep -q "$path$" <<<"$listing" || { echo "deb 缺少 $path" >&2; exit 1; }
    done
    rm -f "$file"
done

echo "生成完成："
ls -lh "$DIST_DIR"
