#!/usr/bin/env bash
# Build fraglink-server for the production VPS and roll it out.
#
#   scripts/deploy.sh            # build + upload + restart + health check
#   scripts/deploy.sh --configs  # also reinstall deploy/ (systemd units, Caddyfile, backup script)
#
# Env: FRAGLINK_HOST (default 119.42.54.86), FRAGLINK_SSH_USER (default fraglink),
#      FRAGLINK_SSH_KEY (default ~/.ssh/fraglink_binarylane).
# The binary is built in a rust:1-bookworm container (glibc 2.36), so it runs on
# the server's Ubuntu 24.04 whatever the build machine's glibc is. Needs docker, ssh, scp.
set -euo pipefail
HOST=${FRAGLINK_HOST:-119.42.54.86}
USER_=${FRAGLINK_SSH_USER:-fraglink}
KEY=${FRAGLINK_SSH_KEY:-$HOME/.ssh/fraglink_binarylane}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SSH=(ssh -i "$KEY" -o StrictHostKeyChecking=accept-new "$USER_@$HOST")
DOCKER=${DOCKER:-docker}

echo "==> building fraglink-server (release, bookworm)"
OUT=$(mktemp -d)
$DOCKER volume create fl-cargo >/dev/null
$DOCKER run --rm -v "$ROOT":/src:ro -v fl-cargo:/usr/local/cargo/registry -v "$OUT":/out \
  -e CARGO_TARGET_DIR=/tmp/target -w /src rust:1-bookworm \
  sh -c 'cargo build --release --locked -p fraglink-server && cp /tmp/target/release/fraglink-server /out/'

echo "==> uploading to $HOST"
FILES=("$OUT/fraglink-server")
[[ "${1:-}" == "--configs" ]] && FILES+=("$ROOT"/deploy/*)
scp -i "$KEY" -o StrictHostKeyChecking=accept-new "${FILES[@]}" "$USER_@$HOST:/tmp/"

echo "==> installing and restarting"
"${SSH[@]}" CONFIGS="${1:-}" 'bash -s' <<'REMOTE'
set -euo pipefail
sudo install -m 755 /tmp/fraglink-server /opt/fraglink/fraglink-server.new
sudo cp -f /opt/fraglink/fraglink-server /opt/fraglink/fraglink-server.prev 2>/dev/null || true
sudo mv /opt/fraglink/fraglink-server.new /opt/fraglink/fraglink-server
if [[ "$CONFIGS" == "--configs" ]]; then
  sudo install -m 644 /tmp/fraglink-server.service /tmp/fraglink-backup.service /tmp/fraglink-backup.timer \
    /tmp/fraglink-geoip.service /tmp/fraglink-geoip.timer /etc/systemd/system/
  sudo install -m 755 /tmp/fraglink-backup.sh /usr/local/sbin/fraglink-backup
  sudo install -m 755 /tmp/fraglink-geoip.sh /usr/local/sbin/fraglink-geoip
  sudo install -m 644 /tmp/Caddyfile /etc/caddy/Caddyfile
  sudo systemctl daemon-reload && sudo systemctl reload caddy
  sudo systemctl enable --now fraglink-geoip.timer
  [ -f /var/lib/fraglink/geo/country.mmdb ] || sudo /usr/local/sbin/fraglink-geoip || true
fi
rm -f /tmp/fraglink-server /tmp/fraglink-server.service /tmp/fraglink-backup.* /tmp/fraglink-geoip.* /tmp/Caddyfile
sudo systemctl restart fraglink-server   # migrations run on start
for i in $(seq 1 20); do curl -fs -m2 http://127.0.0.1:8787/api/v1/health && echo && exit 0; sleep 1; done
echo "health check failed; rolling back"; sudo journalctl -u fraglink-server -n 20 --no-pager
sudo cp /opt/fraglink/fraglink-server.prev /opt/fraglink/fraglink-server && sudo systemctl restart fraglink-server; exit 1
REMOTE
rm -rf "$OUT"
curl -fsS -m10 "http://$HOST/api/v1/health" && echo " <- public health OK"
