#!/usr/bin/env bash
# 构建 macOS 桌面应用：PiStar.app + pi-star-app_<version>_macos_<arch>.dmg。
# 内嵌 pi-star、Node、pi 包和 agent-extension；有 Developer ID 身份时签名。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$("$ROOT/scripts/check-desktop-version.sh")"
DIST_DIR="${DIST_DIR:-$ROOT/dist/mac}"
RUNTIME_DIR="${PI_RUNTIME_DIR:-$ROOT/app/pi-runtime}"
INJECT_CONFIG="$ROOT/app/.bundle-resources.json"
APP_BIN="$ROOT/app/pi-star"
APP_NAME="PiStar"
ENTITLEMENTS="$ROOT/packaging/macos/entitlements.plist"

case "$(uname -s)" in
    Darwin*) ;;
    *) echo "package-mac.sh 仅支持 macOS" >&2; exit 1 ;;
esac

cleanup() {
    rm -f "$INJECT_CONFIG" "$APP_BIN"
}
trap cleanup EXIT

for command in cargo node npm curl tar codesign hdiutil plutil file ditto security; do
    command -v "$command" >/dev/null 2>&1 || { echo "缺少命令: $command" >&2; exit 1; }
done
if ! cargo tauri --version >/dev/null 2>&1; then
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64) MAC_ARCH=x86_64; NODE_ARCH=x64 ;;
    arm64) MAC_ARCH=arm64; NODE_ARCH=arm64 ;;
    *) echo "不支持的 macOS 架构: $(uname -m)" >&2; exit 1 ;;
esac

if [[ "${SKIP_UI_BUILD:-0}" != "1" ]]; then
    [[ -d "$ROOT/ui/node_modules" ]] || (cd "$ROOT/ui" && npm ci)
    (cd "$ROOT/ui" && PI_STAR_VERSION="$VERSION" npm run build)
fi
[[ -f "$ROOT/ui/dist/index.html" ]] || { echo "缺少 ui/dist/index.html" >&2; exit 1; }

if [[ "${SKIP_PI_RUNTIME:-0}" != "1" ]]; then
    node "$ROOT/scripts/prepare-pi-runtime.mjs" --out "$RUNTIME_DIR" --platform darwin --arch "$NODE_ARCH"
fi
for path in "$RUNTIME_DIR/node" "$RUNTIME_DIR/pi/package.json" "$RUNTIME_DIR/agent-extension/index.ts"; do
    [[ -e "$path" ]] || { echo "pi 运行时缺少 $path" >&2; exit 1; }
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

(cd "$ROOT/app" && cargo tauri build --config "$INJECT_CONFIG" --bundles app)
rm -f "$INJECT_CONFIG"

APP_TARGET="$(target_dir "$ROOT/app")"
APP_SRC="$APP_TARGET/release/bundle/macos/$APP_NAME.app"
[[ -d "$APP_SRC" ]] || { echo "Tauri bundler 未产出 $APP_SRC" >&2; exit 1; }

for path in \
    "Contents/Resources/pi-star" \
    "Contents/Resources/pi-runtime/node" \
    "Contents/Resources/pi-runtime/pi/package.json" \
    "Contents/Resources/pi-runtime/agent-extension/index.ts"; do
    [[ -e "$APP_SRC/$path" ]] || { echo "app 缺少 $path" >&2; exit 1; }
done

resolve_identity() {
    if [[ -n "${MAC_SIGN_IDENTITY:-}" ]]; then
        if ! security find-identity -v -p codesigning 2>/dev/null | grep -qF "\"${MAC_SIGN_IDENTITY}\""; then
            echo "找不到 MAC_SIGN_IDENTITY：${MAC_SIGN_IDENTITY}" >&2
            exit 1
        fi
        IDENTITY="$MAC_SIGN_IDENTITY"
        return 0
    fi
    local candidates=()
    local candidate
    while IFS= read -r candidate; do
        candidates+=("$candidate")
    done < <(security find-identity -v -p codesigning 2>/dev/null |
        sed -n 's/.*"\(Developer ID Application[^"]*\)".*/\1/p')
    if [[ ${#candidates[@]} -eq 1 ]]; then
        IDENTITY="${candidates[0]}"
        return 0
    fi
    return 1
}

IDENTITY=""
if ! resolve_identity; then
    echo "WARN: 未找到唯一的 Developer ID Application 身份，产物将不签名。" >&2
    echo "      设置 MAC_SIGN_IDENTITY 或导入证书后可签名/公证。" >&2
fi

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"
APP="$DIST_DIR/$APP_NAME.app"
ditto "$APP_SRC" "$APP"

main_exec="$(plutil -extract CFBundleExecutable raw -o - "$APP/Contents/Info.plist")"
if [[ -n "$IDENTITY" ]]; then
    echo "签名身份：$IDENTITY"
    signed=0
    # 先签内层 Mach-O（Node、pi-star、原生 .node）。跳过主程序，留给外层 bundle 签名。
    while IFS= read -r -d '' candidate; do
        [[ "$candidate" == "$APP/Contents/MacOS/$main_exec" ]] && continue
        if file -b "$candidate" 2>/dev/null | grep -q 'Mach-O'; then
            codesign --force --options runtime --timestamp \
                --entitlements "$ENTITLEMENTS" -s "$IDENTITY" "$candidate" </dev/null
            signed=$((signed + 1))
        fi
    done < <(find "$APP/Contents" -type f -print0)
    echo "内层签名完成：$signed 个 Mach-O"

    codesign --force --options runtime --timestamp -s "$IDENTITY" "$APP" </dev/null
    codesign --verify --deep --strict "$APP"
    echo "签名校验通过"
else
    echo "WARN: 产物未签名；macOS 首次打开可能需要在“隐私与安全性”中允许。" >&2
fi

# DMG：包含 .app 与 /Applications 链接。
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pistar-dmg.XXXXXX")"
trap 'rm -rf "$STAGE"; cleanup' EXIT
ditto "$APP" "$STAGE/$APP_NAME.app"
ln -s /Applications "$STAGE/Applications"
DMG="$DIST_DIR/pi-star-app_${VERSION}_macos_${MAC_ARCH}.dmg"
hdiutil create -volname "Pi Star" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
trap cleanup EXIT

echo "生成完成："
echo "  $APP"
echo "  $DMG"
