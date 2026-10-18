#!/bin/sh
# Nightly pg_dump of the fraglink database. Everything in the backup folder (nightly
# dumps and the manual pre-* dumps/folders, *.www.tgz site copies and announcements CSVs
# taken before deploys) is deleted once it is 14 days old.
set -eu
DIR=/var/backups/fraglink
install -d -m 700 -o postgres -g postgres "$DIR"
F="$DIR/fraglink-$(date +%Y%m%d-%H%M%S).dump"
runuser -u postgres -- pg_dump -Fc -d fraglink -f "$F.part"
mv "$F.part" "$F"
find "$DIR" -mindepth 1 -maxdepth 1 -mtime +13 \
  \( -name 'fraglink-*.dump' -o -name 'pre-*' -o -name '*.www.tgz' -o -name 'announcements-*.csv' \) \
  -exec rm -rf -- {} +
