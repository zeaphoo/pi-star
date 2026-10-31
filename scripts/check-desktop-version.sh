#!/usr/bin/env bash
# 校验桌面壳与 workspace 的版本一致。发布/打包前调用。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

workspace_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)"
app_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/app/Cargo.toml" | head -n1)"
conf_version="$(node -e 'const fs=require("fs");const c=JSON.parse(fs.readFileSync(process.argv[1],"utf8"));process.stdout.write(c.version||"")' "$ROOT/app/tauri.conf.json")"

if [[ -z "$workspace_version" || -z "$app_version" || -z "$conf_version" ]]; then
    echo "无法读取版本号：workspace='$workspace_version' app='$app_version' tauri.conf='$conf_version'" >&2
    exit 1
fi

if [[ "$workspace_version" != "$app_version" || "$workspace_version" != "$conf_version" ]]; then
    echo "版本不一致：workspace=$workspace_version app/Cargo.toml=$app_version app/tauri.conf.json=$conf_version" >&2
    exit 1
fi

echo "$workspace_version"
