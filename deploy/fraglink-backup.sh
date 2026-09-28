#!/bin/sh
# Nightly pg_dump of the fraglink database; keeps 14 days.
set -eu
DIR=/var/backups/fraglink
install -d -m 700 -o postgres -g postgres "$DIR"
F="$DIR/fraglink-$(date +%Y%m%d-%H%M%S).dump"
runuser -u postgres -- pg_dump -Fc -d fraglink -f "$F.part"
mv "$F.part" "$F"
find "$DIR" -name 'fraglink-*.dump' -mtime +13 -delete
