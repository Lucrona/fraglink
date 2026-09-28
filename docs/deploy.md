# Production server

One BinaryLane VPS in Sydney (Ubuntu 24.04, 1 vCPU, 2 GB RAM + 1 GB swap), public IP `119.42.54.86`.
Everything below is already set up; this page is for redeploys and recovery.

| What | Where |
|---|---|
| Server binary | `/opt/fraglink/fraglink-server` (previous one: `fraglink-server.prev`) |
| Service | `fraglink-server.service` (user `fraglink-srv`, listens on `127.0.0.1:8787`; migrations run at start) |
| Secrets / config | `/etc/fraglink/server.env` (root-only, `DATABASE_URL`, `FRAGLINK_BIND`, `FRAGLINK_PUBLIC_URL`, optional `STEAM_API_KEY`) |
| Reverse proxy | Caddy, `/etc/caddy/Caddyfile` (HTTP on the IP for now) |
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

Check it: `curl http://119.42.54.86/api/v1/health` → `{"ok":true,"version":"…"}`.
Logs: `sudo journalctl -u fraglink-server -f`.

## Switch to the domain (once DNS is set)

1. DNS: `A` records for `fraglink.gg` and `app.fraglink.gg` → `119.42.54.86`.
2. In `/etc/caddy/Caddyfile` delete the `:80` block and uncomment the `app.fraglink.gg, fraglink.gg` block
   (also in `deploy/Caddyfile`). Caddy then gets Let's Encrypt certificates by itself.
3. Set `FRAGLINK_PUBLIC_URL=https://app.fraglink.gg` in `/etc/fraglink/server.env` (secure cookies follow).
4. `sudo systemctl reload caddy && sudo systemctl restart fraglink-server`.

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
