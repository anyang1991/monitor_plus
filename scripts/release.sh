#!/usr/bin/env bash
# Monitor Plus 发布脚本（macOS）
# 用法：./scripts/release.sh 0.2.0 "更新说明"
# 构建并发布 .app.tar.gz（供已安装到 /Applications 的应用一键更新）。
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?用法: release.sh x.y.z [更新说明]}"
NOTES="${2:-}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "版本号需为 x.y.z"; exit 1; }

# 1. 同步版本号
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" src-tauri/tauri.conf.json package.json
sed -i '' "0,/^version = \".*\"/s//version = \"$VERSION\"/" src-tauri/Cargo.toml

# 2. 构建
pnpm install
pnpm tauri build

# 3. 收集产物（mac arm64）
TARGZ=$(ls src-tauri/target/release/bundle/macos/*.app.tar.gz | head -1)
mkdir -p updates
ASSET="MonitorPlus_${VERSION}_aarch64.app.tar.gz"
cp "$TARGZ" "updates/$ASSET"

# 4. 更新清单（保留其他平台条目）
# 更新通道：默认 GitHub Releases（公开）；私有通道用环境变量覆盖
#   RELEASE_BASE=https://your.gitlab/api/v4/projects/ID/repository/files  +  RELEASE_TOKEN=xxx
BASE="${RELEASE_BASE:-https://github.com/YOUR_GITHUB_USERNAME/monitor_plus/releases/download/v$VERSION}"
python3 - "$VERSION" "$NOTES" "$ASSET" "$BASE" <<'EOF'
import json, sys, datetime, os
version, notes, asset, base = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
path = "updates/latest.json"
m = {}
if os.path.exists(path):
    try:
        m = json.load(open(path))
    except Exception:
        pass
plat = m.get("platforms") or {}
plat["darwin-aarch64"] = {"url": f"{base}/updates%2F{asset}/raw?ref=main", "size": os.path.getsize(f"updates/{asset}")}
m.update({
    "version": version,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "notes": notes,
    "platforms": plat,
})
json.dump(m, open(path, "w"), indent=2, ensure_ascii=False)
EOF

# 5. 提交推送
git add updates src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json src-tauri/Cargo.lock
git commit -m "release v$VERSION" || true
git push origin main

echo "✅ v$VERSION 已发布。"
