#!/usr/bin/env bash
# Set the release version everywhere (beta channel: 0.3.0-beta.N, tags v0.3.0-beta.N).
#
#   scripts/bump-version.sh 0.3.0-beta.2
#
# Writes Cargo.toml (workspace version, also the server's /api/v1/health version),
# apps/desktop/package.json, apps/desktop/src-tauri/tauri.conf.json (+ the MSI version
# 0.3.0.N, because Windows Installer only takes numbers) and refreshes Cargo.lock.
# The updater compares versions as semver: 0.3.0-beta.2 > 0.3.0-beta.1 > 0.2.0-21.
set -euo pipefail
V=${1:?usage: bump-version.sh <major.minor.patch-beta.N | major.minor.patch>}
[[ "$V" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-beta\.([0-9]+)(\.([0-9]+))?)?$ ]] || { echo "version must look like 0.3.0-beta.1, 0.3.0-beta.9.1 or 0.3.0" >&2; exit 1; }
# MSI 4th field: beta.1-9 -> N; point releases and beta.10+ -> N*10+M (beta.9.1 -> 91, beta.10 -> 100), so it keeps rising.
N=${BASH_REMATCH[5]:-0}; M=${BASH_REMATCH[7]:-}
B=$N; { [[ -n "$M" ]] || (( N >= 10 )); } && B=$(( N * 10 + ${M:-0} ))
MSI="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}.$B"
# final releases need a higher MSI build than any beta of the same version
[[ -z "${BASH_REMATCH[4]}" ]] && MSI="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}.65535"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
sed -i -E "0,/^version = \"[^\"]+\"/s//version = \"$V\"/" "$ROOT/Cargo.toml"
python3 - "$ROOT" "$V" "$MSI" <<'PY'
import json, sys
root, v, msi = sys.argv[1:]
p = f"{root}/apps/desktop/package.json"; d = json.load(open(p)); d["version"] = v
open(p, "w").write(json.dumps(d, indent=2, ensure_ascii=False) + "\n")
p = f"{root}/apps/desktop/src-tauri/tauri.conf.json"; d = json.load(open(p)); d["version"] = v
d["bundle"].setdefault("windows", {}).setdefault("wix", {})["version"] = msi
open(p, "w").write(json.dumps(d, indent=2, ensure_ascii=False) + "\n")
PY
(cd "$ROOT" && cargo update -w -q)
echo "version $V (MSI $MSI)"
