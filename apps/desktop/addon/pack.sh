#!/bin/sh
# CurseForge zip: the addon folder at the root, one shared toc, no flavour copies.
set -eu
cd "$(dirname "$0")"
ver=$(sed -n 's/^## Version: //p' FraglinkPlayed/FraglinkPlayed.toc)
test "$ver" = "1.1.1"
if ls FraglinkPlayed/*_*.toc >/dev/null 2>&1; then
  echo "flavour toc files must not ship" >&2
  exit 1
fi
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/FraglinkPlayed"
cp FraglinkPlayed/FraglinkPlayed.lua FraglinkPlayed/FraglinkPlayed.toc \
  FraglinkPlayed/CHANGELOG.md FraglinkPlayed/icon.tga "$stage/FraglinkPlayed/"
out="$PWD/FraglinkPlayed-$ver.zip"
rm -f "$out"
(cd "$stage" && zip -r -X "$out" FraglinkPlayed >/dev/null)
python3 - "$out" <<'PY'
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
names = set(z.namelist())
need = {
    "FraglinkPlayed/FraglinkPlayed.lua",
    "FraglinkPlayed/FraglinkPlayed.toc",
    "FraglinkPlayed/CHANGELOG.md",
    "FraglinkPlayed/icon.tga",
}
missing = need - names
extra = [n for n in names if n.endswith(".toc") and n != "FraglinkPlayed/FraglinkPlayed.toc"]
if missing or extra:
    raise SystemExit(f"bad zip missing={missing} extra_tocs={extra} names={sorted(names)}")
toc = z.read("FraglinkPlayed/FraglinkPlayed.toc").decode()
for needle in ("## Version: 1.1.1", "## Interface: 120105, 120107, 16001"):
    if needle not in toc:
        raise SystemExit("toc missing " + needle)
print("zip ok", sys.argv[1])
PY
if [ -d /opt/cursor/artifacts ]; then
  cp "$out" /opt/cursor/artifacts/
fi
