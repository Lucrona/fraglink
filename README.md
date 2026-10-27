<p align="center"><img src="docs/images/hero.png" alt="Fraglink" width="100%"></p>

<p align="center"><b>PLAY. CONNECT. TRACK.</b><br>A small Windows tray client that tracks your hours in every launcher and shows what your friends are playing.<br>
<sub>Public beta · current release <b>0.3.0-beta.9.1</b> · <a href="https://playfraglink.com/download">Download</a> · <a href="https://github.com/Lucrona/fraglink/releases">Releases</a> · <a href="CHANGELOG.md">Changelog</a></sub></p>

---

## What is Fraglink?

Fraglink is a **lightweight tray client for Windows** in the spirit of the classic gaming clients. It notices which game you're playing on any launcher (Steam, Battle.net, Epic, EA, Ubisoft, GOG, Xbox or standalone), keeps an honest record of your play time, and shows your friends list with who is online and what they're playing.

The website at [playfraglink.com](https://playfraglink.com) hosts public profiles, hours, friends, leaderboards and account settings. You can link Steam, Battle.net and Discord, import your Steam hours, and bring in exact World of Warcraft `/played` hours with the [Fraglink addon](https://www.curseforge.com/projects/1766775). Fraglink doesn't do chat, voice or overlays — Discord already covers those.

**Privacy in short:** no game memory reading, no injection, no overlays, no keylogging, no ads or selling data. Hours count while you're signed in and connected. Hours are either **Tracked** (by Fraglink or Steam) or **Imported** (a game's own counter, like WoW `/played`). Full policy: [playfraglink.com/privacy](https://playfraglink.com/privacy). Details of what the app reads: [`docs/privacy.md`](docs/privacy.md).

## Layout

```
apps/desktop/            Tauri 2 Windows app (tray client)
apps/server/             axum + Postgres API and website backend
apps/site-web/           Static website pages (built into dist/)
crates/fraglink-core/    Game detection, tracker, local SQLite, export
crates/fraglink-sync/    Desktop ↔ server sync client
packages/signatures/     Game signatures and folder rules
deploy/                  systemd unit, Caddyfile, backup/geo helpers
docs/                    Roadmap, decisions, original spec, privacy notes, legal texts, short deploy note
```

## Build from source

Prerequisites: Rust stable (1.94+) and Node 20+. On Windows, WebView2 and the MSVC build tools. On Linux (for CI / server), the usual Tauri system libs if you build the desktop app there.

**Desktop**

```bash
cd apps/desktop
npm ci
npm run dev          # run against real processes
npm run dev:sim      # fake process source
npm run build        # release build + Windows installers
```

**Server** (needs Postgres, e.g. `docker compose up -d db`):

```bash
export DATABASE_URL=postgres://fraglink:fraglink@127.0.0.1:5432/fraglink
cargo run -p fraglink-server      # http://localhost:8787
cargo test -p fraglink-server
```

**Core only** (any OS):

```bash
cargo test -p fraglink-core
```

More server env vars and API notes: [`apps/server/README.md`](apps/server/README.md). Redeploy helpers: [`docs/deploy.md`](docs/deploy.md) and `scripts/deploy.sh`.

## Documentation

- [Roadmap](docs/ROADMAP.md) · [Changelog](CHANGELOG.md) · [Product spec](docs/fraglink-mvp-spec.md)
- [Design decisions](docs/decisions.md) · [Windows testing checklist](docs/testing-windows.md) · [Privacy notes](docs/privacy.md)
- [Privacy Policy](https://playfraglink.com/privacy) · [Terms of Use](https://playfraglink.com/terms) · [FAQ](https://playfraglink.com/faq)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## License

**All rights reserved; license to be decided before a stable public release.** See [`LICENSE`](LICENSE). Bundled fonts keep their own licences (Russo One: SIL OFL 1.1; Wine Tahoma-metric fonts: LGPL 2.1+). The wordmark is Saira Expanded Bold (SIL OFL 1.1), converted to outlines. Website country flags come from [flag-icons](https://github.com/lipis/flag-icons) (MIT, see `apps/site-web/static/flags/LICENSE.txt`). IP geolocation data by [DB-IP](https://db-ip.com) (CC BY 4.0).

## Independent project

Fraglink is an independent project, not affiliated with or endorsed by any game company, launcher or chat service. The Fraglink name, mark and UI artwork are original. Game names are used only to identify games and belong to their owners.
