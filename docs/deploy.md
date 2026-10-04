# Production server

One BinaryLane VPS in Sydney (Ubuntu 24.04, 1 vCPU, 2 GB RAM + 1 GB swap), public IP `119.42.54.86`, domain `playfraglink.com` (Cloudflare Registrar, DNS only, not proxied).
Everything below is already set up; this page is for redeploys and recovery.

| What | Where |
|---|---|
| Server binary | `/opt/fraglink/fraglink-server` (previous one: `fraglink-server.prev`) |
| Service | `fraglink-server.service` (user `fraglink-srv`, listens on `127.0.0.1:8787`; migrations run at start) |
| Secrets / config | `/etc/fraglink/server.env` (root-only, `DATABASE_URL`, `FRAGLINK_BIND`, `FRAGLINK_PUBLIC_URL`, optional `STEAM_API_KEY`) |
| Reverse proxy | Caddy, `/etc/caddy/Caddyfile`: HTTPS on `playfraglink.com` (Let's Encrypt, automatic), `www` redirects to it, plain HTTP on the IP kept for old clients |
| Coming-soon page | `/var/www/fraglink-site` (`apps/site-coming-soon/`), served for `https://playfraglink.com/` only |
| Database | PostgreSQL 16, database and role `fraglink`, localhost only |
| Backups | `/var/backups/fraglink/fraglink-YYYYMMDD-HHMMSS.dump` nightly at 03:30 (`fraglink-backup.timer`), 14 days kept |
| Copies of the unit files, Caddyfile, backup script | `deploy/` in this repo |

Access: SSH key only (`~/.ssh/fraglink_binarylane`) as `fraglink` (passwordless sudo). Root and password logins are off.
Firewall (ufw) allows 22, 80 and 443 only; fail2ban watches sshd; unattended-upgrades installs security updates.

## Redeploy

```sh
scripts/deploy.sh             # build in a rust:1-bookworm container, upload, restart, health check
scripts/deploy.sh --configs   # the same, and reinstall everything in deploy/
```

If the new binary fails its health check the script puts the previous one back. It can't undo migrations, so
a release with a new migration should stay backward compatible or be rolled forward.

Check it: `curl https://playfraglink.com/api/v1/health` (and `http://119.42.54.86/api/v1/health`) → `{"ok":true,"version":"…"}`.
Logs: `sudo journalctl -u fraglink-server -f`.

## Game detection lists (dev.12)

- The server downloads Discord's public detectable list (`FRAGLINK_DETECTABLE_URL`, default
  `https://discord.com/api/v10/applications/detectable`, browser User-Agent) when its copy is a day old, into
  `/var/lib/fraglink/detectable/discord.json` (`FRAGLINK_DATA_DIR`, default `/var/lib/fraglink`). A failed or
  short download keeps the last good copy; with no copy at all it serves the snapshot built into the binary.
  `slugs.json` next to it keeps the Fraglink game id of every Discord id stable.
- `GET /api/v1/games/detectable`: compact Windows-only list, gzip + ETag. `GET /api/v1/games/overrides`: the
  folder rules; put a valid `/var/lib/fraglink/detect-overrides.json` there (same format as
  `packages/signatures/overrides.json`) to change them without an app release (clients pick it up within a day
  or on restart).
- "Track as a game" answers land in the `game_suggestions` table for review:
  `sudo -u postgres psql fraglink -c "SELECT exe, name, reports, last_at FROM game_suggestions WHERE status = 'new' ORDER BY reports DESC"`.

## Desktop auto-updates

Installed clients check `https://playfraglink.com/updates/latest.json` first and `http://119.42.54.86/updates/latest.json`
second (dev.11+; older builds only the IP) at startup and every hour. Caddy serves `/updates/` from
`/var/www/fraglink-updates` (owned by `fraglink`) on both. `latest.json` points at the HTTPS download. Access log:
`/var/log/caddy/updates-https.log` and `updates-ip.log` (only `/updates/*` requests). The repo is private, so GitHub release URLs
aren't reachable by clients; that's why the files live here.

Release flow: push the tag, let CI build (it signs the update files with the `TAURI_SIGNING_PRIVATE_KEY` /
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets and writes `latest.json`), create the GitHub release, then

```sh
scripts/publish-update.sh <ci-run-id>   # copies the setup exe, .sig and latest.json to the server
```

The previous manifest stays next to it (`latest.json.prev`, or `latest.json.devN` when done by hand); to roll
back, copy it over `latest.json`.

The private key's backup is outside the repo (`~/.secrets/fraglink-updater.key` + `.password` on the build box).
Losing it means existing installs can't auto-update to builds signed with a new key.

## Xfire game icons (optional, private testing)

A removable fallback source of game icons, extracted from the 2013 Xfire client. The images are
**not** in the git repo or the desktop installer; only `scripts/xfire_icons.py` is.

- Generate: `python scripts/xfire_icons.py icons.dll xfire_games.ini out/` (needs `pefile` and
  `Pillow`). It writes `<game>--<xfire short>-32.png` (plus `-16.png` when a native 16 px exists)
  and `manifest.json`.
- Install: copy `out/*` to `/var/lib/fraglink/icons/xfire/` (world readable). Caddy serves it at
  `/icons/xfire/` (see `deploy/Caddyfile`).
- Enable: `FRAGLINK_XFIRE_ICONS=1` in `/etc/fraglink/server.env` (the directory can be changed with
  `FRAGLINK_XFIRE_ICONS_DIR`), then `sudo systemctl restart fraglink-server`. The log shows
  `xfire icons: N games`.
- Effect: `GET /api/v1/games/icons` adds `"icon_path": "/icons/xfire/<file>"` whenever an Xfire
  icon matches (even if there is a Steam icon) and `"preferred": "xfire"`. Clients (0.2.0-dev.7+)
  use it first, then Steam, then the local exe icon. It matches the signature's exe names first (only exes unique to one Xfire game),
  then the normalised game name, then a bare series name ("Halo" gives the oldest Xfire "Halo: ..." entry).
- Remove: delete the flag line and restart, then optionally delete the directory and the
  `/icons/xfire/*` block in the Caddyfile. Nothing else depends on it.

## Domain and HTTPS

- DNS (Cloudflare, DNS only): `A playfraglink.com` and `A www.playfraglink.com` → `119.42.54.86`.
- `deploy/Caddyfile` is the live config: `playfraglink.com` gets a Let's Encrypt certificate automatically
  (`http://` and `www` redirect to `https://playfraglink.com`); the `:80` block keeps the IP working over plain
  HTTP for dev.10 and older clients (API, websocket, updates). Both use the same snippet.
- `FRAGLINK_PUBLIC_URL=https://playfraglink.com` in `/etc/fraglink/server.env`: pairing links, Steam sign-in and
  secure web cookies use it. Web sign-in works on the domain; on the IP the web forms are refused (other origin).
- Clients (dev.11+) use `https://playfraglink.com` and `wss://playfraglink.com/api/v1/ws`; if HTTPS or DNS fails
  they switch to `http://119.42.54.86` for the rest of the session.
- Change: edit `deploy/Caddyfile`, then `sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile`
  and `sudo systemctl reload caddy` (a reload drops open websockets for a few seconds; clients reconnect).

## Restore a backup

```sh
sudo systemctl stop fraglink-server
sudo -u postgres dropdb fraglink
sudo -u postgres createdb -O fraglink fraglink
sudo -u postgres pg_restore --no-owner --role=fraglink -d fraglink /var/backups/fraglink/fraglink-YYYYMMDD-HHMMSS.dump
sudo systemctl start fraglink-server
```

Take a backup by hand: `sudo systemctl start fraglink-backup`. Backups only live on the VPS, so copy
one off now and then (`scp fraglink@119.42.54.86:/var/backups/fraglink/…`; the directory is root-only, so copy it
to `/tmp` with sudo first) or turn on BinaryLane backups.
