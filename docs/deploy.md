# Deploy

Production runs on a small Linux VPS behind Caddy (HTTPS). The server binary, unit
files, Caddyfile, backup timer and related helpers live under `deploy/` in this
repo. Day-to-day redeploys:

```sh
scripts/deploy.sh             # build, upload, restart, health check
scripts/deploy.sh --configs   # same, and reinstall files from deploy/
```

If the new binary fails its health check, the script restores the previous one.
Migrations run when the server starts; keep them backward compatible or plan a
forward fix.

Health check: `GET /api/v1/health` → `{"ok":true,"version":"…"}`.

Desktop auto-updates are served from the same host (`/updates/latest.json`). After
CI builds a tagged release:

```sh
scripts/publish-update.sh <ci-run-id>
```

Host-specific paths, SSH, secrets, backup restore and mail provider setup are
kept out of this public repo (operator runbook).
