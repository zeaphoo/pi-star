#!/usr/bin/env bash
# 对 scripts/package-mac.sh 的产物做 Apple 公证、钉票与 Gatekeeper 验收。
# 需要 App Store Connect API key：
#   MAC_NOTARY_API_KEY_ID / MAC_NOTARY_API_ISSUER_ID / MAC_NOTARY_API_KEY_P8_BASE64
# 或 MAC_NOTARY_API_KEY_P8（已解码的 .p8 路径）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/mac}"
APP_NAME="PiStar"
APP="$DIST_DIR/$APP_NAME.app"
TIMEOUT="${NOTARY_TIMEOUT:-30m}"
STAPLER_RETRIES="${STAPLER_RETRIES:-4}"
STAPLER_RETRY_DELAY="${STAPLER_RETRY_DELAY:-20}"

fail() {
    echo "$*" >&2
    exit 1
}

[[ "$(uname -s)" == "Darwin" ]] || fail "notarize-mac.sh 仅支持 macOS"
for command in xcrun ditto hdiutil spctl codesign plutil; do
    command -v "$command" >/dev/null 2>&1 || fail "缺少命令: $command"
done

missing=()
[[ -n "${MAC_NOTARY_API_KEY_ID:-}" ]] || missing+=("MAC_NOTARY_API_KEY_ID")
[[ -n "${MAC_NOTARY_API_ISSUER_ID:-}" ]] || missing+=("MAC_NOTARY_API_ISSUER_ID")
if [[ -z "${MAC_NOTARY_API_KEY_P8:-}" && -z "${MAC_NOTARY_API_KEY_P8_BASE64:-}" ]]; then
    missing+=("MAC_NOTARY_API_KEY_P8_BASE64")
fi
if (( ${#missing[@]} > 0 )); then
    fail "缺少公证凭据：${missing[*]}"
fi
[[ -d "$APP" ]] || fail "缺少已签名 app：$APP（先执行 scripts/package-mac.sh）"

version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/\1/p' "$ROOT/app/tauri.conf.json" | head -n1)"
arch="$(uname -m)"
case "$arch" in
    x86_64) MAC_ARCH=x86_64 ;;
    arm64) MAC_ARCH=arm64 ;;
    *) fail "不支持的 macOS 架构: $arch" ;;
esac

codesign --verify --deep --strict "$APP"
info="$(codesign -dv --verbose=2 "$APP" 2>&1)"
grep -q 'Authority=Developer ID Application' <<<"$info" || {
    echo "$info" >&2
    fail "公证要求 Developer ID Application 签名"
}
grep -q 'flags=.*runtime' <<<"$info" || fail "公证要求 hardened runtime"
echo "$info" | grep -E 'Authority=|flags='

WORK="$(mktemp -d "${TMPDIR:-/tmp}/pistar-notarize.XXXXXX")"
MOUNT="$WORK/mnt"
mkdir -p "$MOUNT"
cleanup() {
    hdiutil detach "$MOUNT" >/dev/null 2>&1 || true
    rmdir "$MOUNT" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

KEY_FILE="${MAC_NOTARY_API_KEY_P8:-}"
if [[ -z "$KEY_FILE" ]]; then
    KEY_FILE="$WORK/notary-key.p8"
    (umask 077; printf '%s' "$MAC_NOTARY_API_KEY_P8_BASE64" | base64 -D > "$KEY_FILE")
fi
[[ -s "$KEY_FILE" ]] || fail "公证密钥不存在或为空：$KEY_FILE"
grep -q 'BEGIN PRIVATE KEY' "$KEY_FILE" || fail "公证密钥不是 .p8 PEM"

notary_args=(--key "$KEY_FILE" --key-id "$MAC_NOTARY_API_KEY_ID" --issuer "$MAC_NOTARY_API_ISSUER_ID")

# stapler talks to Apple CloudKit to fetch the ticket. A successful notarization
# can still fail here with Error 65/68 or NSURLError -1005 ("network connection
# was lost") when CloudKit/network is flaky. Retry only those transient cases.
stapler_retryable() {
    local rc="$1"
    local output="$2"
    if [[ "$rc" -eq 65 || "$rc" -eq 68 ]]; then
        return 0
    fi
    grep -qiE 'NSURLError|connection (was )?lost|-1005|timed out|network|CloudKit' <<<"$output"
}

stapler_retry() {
    local action="$1"
    local target="$2"
    local attempt=1
    local rc=0
    local output=""
    while (( attempt <= STAPLER_RETRIES )); do
        rc=0
        output="$(xcrun stapler "$action" "$target" 2>&1)" || rc=$?
        if (( rc == 0 )); then
            [[ -z "$output" ]] || printf '%s\n' "$output"
            return 0
        fi
        printf 'stapler %s 失败（第 %d/%d 次，rc=%d）：%s\n' \
            "$action" "$attempt" "$STAPLER_RETRIES" "$rc" "$output" >&2
        if ! stapler_retryable "$rc" "$output"; then
            return "$rc"
        fi
        if (( attempt == STAPLER_RETRIES )); then
            return "$rc"
        fi
        printf 'CloudKit/网络瞬时错误，%s 秒后重试…\n' "$STAPLER_RETRY_DELAY" >&2
        sleep "$STAPLER_RETRY_DELAY"
        attempt=$((attempt + 1))
    done
    return "$rc"
}

notarize() {
    local artifact="$1"
    local label="$2"
    echo "提交公证：$label"
    xcrun notarytool submit "$artifact" "${notary_args[@]}" --wait --timeout "$TIMEOUT" || {
        fail "公证失败或超时：$label"
    }
    echo "公证通过：$label"
}

ZIP="$WORK/$APP_NAME.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
notarize "$ZIP" "app zip"

stapler_retry staple "$APP"
stapler_retry validate "$APP"

# 由已钉票的 .app 重建 DMG；dmg 有独立校验和，必须单独公证并钉票。
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/$APP_NAME.app"
ln -s /Applications "$STAGE/Applications"
DMG="$DIST_DIR/pi-star-app_${version}_macos_${MAC_ARCH}.dmg"
rm -f "$DMG"
hdiutil create -volname "Pi Star" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
notarize "$DMG" "dmg"
stapler_retry staple "$DMG"
stapler_retry validate "$DMG"

hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT" "$DMG" >/dev/null
codesign --verify --deep --strict "$MOUNT/$APP_NAME.app"
stapler_retry validate "$MOUNT/$APP_NAME.app"
output="$(spctl -a -vv "$MOUNT/$APP_NAME.app" 2>&1 || true)"
grep -q 'accepted' <<<"$output" && grep -q 'Notarized Developer ID' <<<"$output" || {
    echo "$output" >&2
    fail "spctl 验收失败"
}
hdiutil detach "$MOUNT" >/dev/null
rmdir "$MOUNT" 2>/dev/null || true
trap - EXIT
rm -rf "$WORK"

echo "公证与验收完成："
echo "  $APP"
echo "  $DMG"
