#!/usr/bin/env bash
# 按当前平台构建 Pi Star 桌面应用。所有产物都内嵌 pi-star + pi 运行时。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

case "$(uname -s)" in
    Linux*) exec "$ROOT/scripts/package-linux-app.sh" "$@" ;;
    Darwin*) exec "$ROOT/scripts/package-mac.sh" "$@" ;;
    MINGW* | MSYS* | CYGWIN*) exec "$ROOT/scripts/package-windows.sh" "$@" ;;
    *) echo "不支持的平台: $(uname -s)" >&2; exit 1 ;;
esac
