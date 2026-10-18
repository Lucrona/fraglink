#!/bin/sh
# Monthly refresh of the offline IP-to-country database used for sign-in history
# (DB-IP "IP to Country Lite", CC BY 4.0, https://db-ip.com). The server reloads the
# file on its own when it changes. Nothing about users is sent anywhere.
set -eu
DIR=/var/lib/fraglink/geo
DEST="$DIR/country.mmdb"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
install -d -m 755 -o fraglink-srv -g fraglink-srv "$DIR"
# this month's file; early in the month it may not be out yet, so fall back to last month's
for M in "$(date +%Y-%m)" "$(date -d "$(date +%Y-%m-01) -1 day" +%Y-%m)"; do
  if curl -fsS --max-time 120 -o "$TMP/db.gz" "https://download.db-ip.com/free/dbip-country-lite-$M.mmdb.gz"; then
    gunzip -c "$TMP/db.gz" > "$TMP/country.mmdb"
    # sanity: a real database is several MB and ends with the MaxMind metadata marker
    if [ "$(stat -c %s "$TMP/country.mmdb")" -gt 1000000 ] && grep -qa 'MaxMind.com' "$TMP/country.mmdb"; then
      install -m 644 -o fraglink-srv -g fraglink-srv "$TMP/country.mmdb" "$DEST.new"
      mv -f "$DEST.new" "$DEST"
      echo "country database updated from DB-IP $M"
      exit 0
    fi
  fi
done
echo "country database not updated (download failed); keeping the current file" >&2
exit 1
