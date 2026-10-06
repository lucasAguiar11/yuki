#!/usr/bin/env bash
# Usage: fixture.sh <dest-dir> [workspace-id ...]
# Copies Voleeo workspaces (+ local envs, key files) into an isolated Yuki data
# dir and adds a personal env "Local" (BASE → http://127.0.0.1:8765, TOKEN
# secret) to each. Never point the app at the user's real data dir for tests.
set -euo pipefail
dest=${1:?dest dir}; shift
src="$HOME/Library/Application Support/com.voleeo.desktop"
rm -rf "$dest"; mkdir -p "$dest/workspaces" "$dest/envs-local" "$dest/keys"
ids=("$@"); [ ${#ids[@]} -eq 0 ] && ids=(yEce6y81B9) # "demo": tiny, safe URLs
for id in "${ids[@]}"; do
  cp -R "$src/workspaces/$id" "$dest/workspaces/"
  [ -d "$src/envs-local/$id" ] && cp -R "$src/envs-local/$id" "$dest/envs-local/" || mkdir -p "$dest/envs-local/$id"
  [ -f "$src/keys/$id.key" ] && cp "$src/keys/$id.key" "$dest/keys/"
  cat > "$dest/envs-local/$id/env_smoke.yaml" <<EOF
id: smoke
workspaceId: $id
kind: personal
name: Local
color: ''
shared: false
variables:
- key: BASE
  value: http://127.0.0.1:8765
  encrypted: false
- key: TOKEN
  value: s3cr3t-token
  encrypted: true
createdAt: 2026-01-01T00:00:00
updatedAt: 2026-01-01T00:00:00
EOF
done
mkdir -p "$dest/srv" && echo '{"ok":true,"items":[1,2,3]}' > "$dest/srv/data.json"
echo "fixture ready: $dest (serve $dest/srv on :8765 for sends)"
