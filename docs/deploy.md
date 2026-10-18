# Production server

One BinaryLane VPS in Sydney (Ubuntu 24.04, 1 vCPU, 2 GB RAM + 1 GB swap), public IP `119.42.54.86`, domain `playfraglink.com` (Cloudflare Registrar, DNS only, not proxied).
Everything below is already set up; this page is for redeploys and recovery.

| What | Where |
|---|---|
| Server binary | `/opt/fraglink/fraglink-server` (previous one: `fraglink-server.prev`) |
| Service | `fraglink-server.service` (user `fraglink-srv`, listens on `127.0.0.1:8787`; migrations run at start) |
| Secrets / config | `/etc/fraglink/server.env` (root-only, `DATABASE_URL`, `FRAGLINK_BIND`, `FRAGLINK_PUBLIC_URL`, optional `STEAM_API_KEY`) |
| Reverse proxy | Caddy, `/etc/caddy/Caddyfile`: HTTPS on `playfraglink.com` (Let's Encrypt, automatic), `www` redirects to it, plain HTTP on the IP kept for old clients |
| Website | `/var/www/fraglink-beta` (`apps/site-web/`), served on `https://playfraglink.com` (public, since 2026-10-18) and `https://beta.playfraglink.com` (gated); `/var/www/fraglink-site` keeps `/brand/` and the old coming-soon files |
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

**Versions (from 0.3.0):** releases are `0.3.0-beta.1`, `0.3.0-beta.2`, … with tags `v0.3.0-beta.N` and
CHANGELOG headings `## [0.3.0-beta.N] - YYYY-MM-DD`. Set the version with `scripts/bump-version.sh 0.3.0-beta.N`
(Cargo.toml, package.json, tauri.conf.json and the numeric MSI version `0.3.0.N`). The updater compares
plain semver, so `0.3.0-beta.1` is newer than the old `0.2.0-N` test builds (unit test in
`apps/desktop/src-tauri/src/updater.rs`). `latest.json` just carries the version string; nothing else changes.

Release flow: bump the version and commit, push the tag, let CI build (it signs the update files with the `TAURI_SIGNING_PRIVATE_KEY` /
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets and writes `latest.json`), create the GitHub release, then

```sh
scripts/publish-update.sh <ci-run-id>   # copies the setup exe, .sig and latest.json to the server
```

The previous manifest stays next to it (`latest.json.prev`, or `latest.json.devN` when done by hand); to roll
back, copy it over `latest.json`.

The private key's backup is outside the repo (`~/.secrets/fraglink-updater.key` + `.password` on the build box).
Losing it means existing installs can't auto-update to builds signed with a new key.

## Game icons

`GET /icons/games/{16,32,64}/<game id>.png` serves official game icons from our own domain. The
server fetches each icon once (the game list's app icon first, then the Steam community icon by
appid), resizes it to 16/32/64 px and caches it in `/var/lib/fraglink/icon-cache/`
(`<hash>-<px>.png`; a `.miss` file stops retries for 24 h). Icons older than 30 days are refreshed in
the background. Responses carry an ETag and `Cache-Control: public, max-age=604800`.
`GET /api/v1/games/icons?ids=...` returns the `icon_path` for ids that have one. Upstream bases can be
changed with `FRAGLINK_APP_ICON_CDN` and `FRAGLINK_STEAM_ICON_CDN`. To force a refetch, delete the
cache directory. The desktop app never contacts third-party image hosts; without a server icon it
uses the exe icon or a generic glyph.

## Staff and the admin section

- Roles live in `users.role` (`owner`, `admin`, `moderator`, NULL for everyone else). Exactly one owner
  (migration 0011 made `lucrona` the owner); the owner can't be removed, demoted or banned and is the only
  one who adds/removes admins or transfers ownership. Admins can do everything else; moderators get game
  approvals, user search, ban/unban and hours review.
- The website's `/admin` (private preview: `https://beta.playfraglink.com/admin`) uses `/api/v1/admin/*`.
  Every endpoint checks the role on the server; non-staff get 404. Every change goes to `admin_audit`
  (`SELECT actor_handle, action, target, before, after, created_at FROM admin_audit ORDER BY id DESC`).
- Game edits (`game_overrides`: name, hidden, extra exe names, uploaded icon in
  `/var/lib/fraglink/icon-custom/`) sit on top of the daily games list, so refreshes don't undo them.
- Banned accounts (`users.banned_at`) can't sign in, their tokens are revoked and their profile is hidden.
- The Server tab reads the journal; the unit runs with `SupplementaryGroups=systemd-journal`
  (`/etc/systemd/system/fraglink-server.service.d/journal.conf` on the VPS, and in `deploy/`).
- The CLI still works: `fraglink-server suggestions | approve-game <exe> <name> | unapprove-game <exe>`.

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

## Email (account emails)

The server sends: email confirmation (sign-up / email change), password reset, new sign-in alert.
`/etc/fraglink/server.env`: `FRAGLINK_MAIL=on` plus one provider:
- Resend: `RESEND_API_KEY` (domain `playfraglink.com` verified in Resend; DNS records on Cloudflare).
- Cloudflare Email Service (Workers Paid): `FRAGLINK_MAIL_PROVIDER=cloudflare`, `CLOUDFLARE_EMAIL_TOKEN` (Email Sending: Edit), `CLOUDFLARE_ACCOUNT_ID`.
Optional `FRAGLINK_MAIL_FROM` (default `Fraglink <no-reply@playfraglink.com>`), `FRAGLINK_MAIL_REPLY_TO` (default `support@playfraglink.com`).
Without a key, mail is only logged and email-gated rules stay off. Incoming mail: Cloudflare Email Routing forwards hello@, support@ and a catch-all.

## Site settings and test accounts

- Admin > Site settings (`site_settings`, `src/features.rs`): sign-ups, announcement banner, Steam/Battle.net/Discord connect,
  friend requests, public profiles + search, maintenance mode. Public read-only `GET /api/v1/features`; enforced server-side.
- Test accounts: `UPDATE users SET is_test = true WHERE handle = '...'` hides an account from everyone but itself and its friends.
