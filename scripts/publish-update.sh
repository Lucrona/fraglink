#!/usr/bin/env bash
# Publish a CI build to the auto-updater: copies the signed NSIS installer, its .sig and
# latest.json from a GitHub Actions run to http://119.42.54.86/updates/ (Caddy serves
# /var/www/fraglink-updates). Installed clients check that latest.json every 6 hours.
#
#   scripts/publish-update.sh <run-id>      # the CI run of the release tag
#
# Needs gh (GH_CONFIG_DIR as usual), unzip, ssh/scp. Env: FRAGLINK_HOST, FRAGLINK_SSH_USER, FRAGLINK_SSH_KEY.
set -euo pipefail
RUN=${1:?usage: publish-update.sh <run-id>}
HOST=${FRAGLINK_HOST:-119.42.54.86}
USER_=${FRAGLINK_SSH_USER:-fraglink}
KEY=${FRAGLINK_SSH_KEY:-$HOME/.ssh/fraglink_binarylane}
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
ID=$(gh api "repos/Lucrona/fraglink/actions/runs/$RUN/artifacts" --jq '.artifacts[] | select(.name|startswith("fraglink-windows-installers")) | .id')
gh api "repos/Lucrona/fraglink/actions/artifacts/$ID/zip" > "$TMP/a.zip"
unzip -q "$TMP/a.zip" -d "$TMP"
EXE=$(ls "$TMP"/nsis/*-setup.exe)
test -f "$EXE.sig" && test -f "$TMP/nsis/latest.json"
scp -i "$KEY" "$EXE" "$EXE.sig" "$USER_@$HOST:/var/www/fraglink-updates/"
# latest.json last, so clients never see a manifest before its installer is there
scp -i "$KEY" "$TMP/nsis/latest.json" "$USER_@$HOST:/var/www/fraglink-updates/latest.json.new"
ssh -i "$KEY" "$USER_@$HOST" 'mv /var/www/fraglink-updates/latest.json.new /var/www/fraglink-updates/latest.json'
curl -fsS "http://$HOST/updates/latest.json"
