#!/usr/bin/env bash
# 从 CHANGELOG.md 抽取某个版本的条目，作为 GitHub Release 正文。
#
# 用法：
#   ./scripts/release-notes.sh 0.1.0     # 打印该版本条目（不含 `## [...]` 标题）
#   ./scripts/release-notes.sh           # 打印当前清单版本对应的条目
#
# 条目格式见 Keep a Changelog：从 `## [<version>]` 起，到下一个 `## [` 或链接定义止。
# 找不到条目时以非零码退出，发布流水线据此拦截。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
    VERSION="$("$ROOT/scripts/check-desktop-version.sh")"
fi

CHANGELOG="$ROOT/CHANGELOG.md"
[[ -f "$CHANGELOG" ]] || { echo "缺少 CHANGELOG.md" >&2; exit 1; }

notes="$(awk -v ver="## [$VERSION]" '
    index($0, ver) == 1 { inside = 1; next }
    inside && index($0, "## [") == 1 { exit }
    inside && /^\[[^]]+\]: / { exit }
    inside { print }
' "$CHANGELOG" | sed -e '/./,$!d' -e :a -e '/^\n*$/{$d;N;ba' -e '}')"

if ! grep -q '[^[:space:]]' <<<"$notes"; then
    echo "CHANGELOG.md 缺少版本 $VERSION 的条目（应为 '## [$VERSION] - <date>'）" >&2
    exit 1
fi

printf '%s\n' "$notes"
