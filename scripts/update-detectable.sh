#!/usr/bin/env bash
# Refresh the detectable-games snapshot bundled into the app and server
# (packages/signatures/detectable.json.gz) from Discord's public list. The live server
# re-downloads the list daily by itself; this only updates the offline fallback.
# Game ids already in the snapshot are kept.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
curl -fsS --compressed -o "$TMP/discord.json" \
  -H 'User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36' \
  -H 'Accept: application/json' https://discord.com/api/v10/applications/detectable
cd "$ROOT"
cargo run -q --release -p fraglink-core --bin fraglink-detectable -- "$TMP/discord.json" "$TMP/out.json.gz"
mv "$TMP/out.json.gz" packages/signatures/detectable.json.gz
echo "updated packages/signatures/detectable.json.gz"
