<p align="center"><img src="docs/images/fraglink-wordmark.png" alt="Fraglink" width="420"></p>

<p align="center"><b>Track your gaming hours across every launcher and see what your friends are playing, from anywhere.</b></p>

<p align="center"><i>Early development. Current build: 0.1.1 (M1, a local hours tracker); M2 (accounts + Steam + trust tiers) in progress. Private testing only.</i></p>

---

## What is Fraglink?

Fraglink is a **lightweight Windows tray client, inspired by Xfire**, for PC gamers who care about their hours. It sits quietly in the tray, notices which game you are playing on any launcher (Steam, Battle.net, Riot, Epic, EA, standalone), and keeps an honest record of your play time. Later, it will also show which friends are online and what they're playing.

The tray app is deliberately tiny: a sensor, a friends list and a quick glance at your stats. It wears a faithful 2000s-style chrome UI. **The website (fraglink.gg, coming later) does the heavy lifting:** your auto-filled profile, full stats and gaming history, friends from any device including phones, game pages, recaps, achievements, WoW character cards and forum signature images.

Fraglink sits alongside Discord, Steam and Twitch rather than replacing them.

<p align="center"><img src="docs/images/app-tabs.png" alt="Fraglink tray client: Friends (sample), Stats and Activity tabs" width="900"><br><sub>The current M1 build (simulation mode): Friends (sample data), Stats and Activity on real local data.</sub></p>

## Status

**Early development.**

- **Done:** M0 (setup) and M1 (local tracker + tray tabs), released as 0.1.1.
- **In progress:** M2 (done locally except Discord login, email and hosting) and M3. A local development server (`apps/server`) has accounts, tray pairing, opt-in session sync, Tracked / Imported / Estimated hours, Steam import, zip export, friends and live presence. Nothing is hosted yet.
- **Not yet:** the activity feed and the real website.
- **Builds:** CI produces unsigned Windows installers for private testing.

See the [changelog](CHANGELOG.md) and [roadmap](docs/ROADMAP.md).

## Features

**In the current build (M1)**

- Automatic game detection:
  - **any installed Steam, Epic Games Store or GOG Galaxy game** (read from the launchers' local install lists, no list needed)
  - 170 curated games from other launchers and classics (all verified against public sources)
  - "This is a game…" for anything else
- Honest hours: **idle trimming** (no "left it at the menu overnight = 12 h"), short-session filter, crash-safe sessions.
- Stats tab:
  - Now Playing with a live timer
  - Today, This week vs last week, Streak
  - top games with 7-day and total hours
- Activity tab with your own events and hour milestones.
- A "started playing" pop-up.
- Classic Silver, Phoenix and StreamEngine skins, scalable 100–200 %.
- Pause tracking, per-game exclusions, start with Windows (off by default).
- **Export everything** (JSON + CSV) from the tray at any time.

**M2 preview (local server, opt-in)**

- Sign in to sync from Options: pair the tray with a code, approved in your browser. Sessions upload in the background, and tracking works the same offline.
- Server: email accounts, Steam link (verified via Steam sign-in) and playtime import, history estimator and self-reported counters, each number labelled **● Tracked / ◆ Imported / ~ Estimated**. Leaderboards count Tracked only.
- Stats tab shows your account totals from all devices with tier markers (falls back to this PC offline).
- See [`apps/server/README.md`](apps/server/README.md), [`docs/decisions.md`](docs/decisions.md) and the draft [privacy policy](docs/privacy-policy.md).

**M3 preview (friends, local server)**

- Add friends by username, accept/decline, remove, block (silent).
- Live presence: Online / Away / Invisible (friends see you offline) and the game you're playing, over a WebSocket.
- Friends tab on real data with a side panel, plus "*friend* has started playing" pop-ups (not while you're playing, by default; mute per friend).

<p align="center"><img src="docs/images/friends-live.png" alt="Friends tab with live presence" width="290"> <img src="docs/images/stats-tiers.png" alt="Stats tab with account totals by tier" width="290"><br><img src="docs/images/friend-toast.png" alt="Friend started playing toast" width="320"><br><sub>Real tray window (simulation mode) against a local server.</sub></p>

**Coming** (see the [roadmap](docs/ROADMAP.md))

- **M2 (rest):** Discord login, email verification/password reset, hosting, legal review of the privacy policy.
- **M3 (rest):** friends' activity feed, Steam friends presence.
- **M4:** fraglink.gg profiles, gaming history, onboarding estimator and signature images.
- **M5:** verified WoW character cards and exact `/played` import.
- **M6:** game pages, achievements, weekly recaps, Twitch live badge.
- **M7:** signed builds, auto-update, closed beta.

<p align="center"><img src="docs/images/website-concept.png" alt="Concept mock-up of a fraglink.gg profile" width="700"><br><sub>Concept mock-up of a fraglink.gg profile (from the prototype; all data fictional).</sub></p>

<p align="center"><img src="docs/images/skins-concept.png" alt="Phoenix skin concept" width="520"><br><sub>Phoenix skin (prototype).</sub></p>

## Privacy stance

- **No game memory reading, no injection, no overlays, no packet sniffing.** Fraglink only lists running processes and reads documented files (Steam library manifests), so it is safe alongside anti-cheat (VAC, EAC, BattlEye, Vanguard, Ricochet).
- **No keyboard or mouse hooks.** Idle detection uses Windows' `GetLastInputInfo`, which only gives the time since your last input, never what you typed.
- **Local-first, no telemetry.** The tray makes no network connections unless you sign in to sync (Options). Even then it only uploads game sessions (game, start, end, active time), never program names or paths.
- **Export-everything pledge.** All your data is exportable, free, at any time. That is already in the tray menu today. If Fraglink ever shuts down, you'll get plenty of notice and a full export.

Details: [`docs/privacy.md`](docs/privacy.md).

## Layout

```
apps/desktop/            Tauri 2 app
  ui/                    webview frontend: plain HTML/CSS/JS from the prototype, no bundler
  src-tauri/             Rust shell: tray, windows, commands, tracker thread
apps/server/             M2 server: axum + Postgres, accounts, pairing, sync, trust tiers, Steam import
crates/fraglink-core/    detector, Steam/Epic/GOG library readers, tracker, SQLite store + upload outbox, stats, export, simulation
crates/fraglink-sync/    HTTP client the tray uses for opt-in sync (DPAPI-protected token)
  src/bin/fraglink-sim.rs  headless runner for testing on any OS
  tests/fixtures/steam/  VDF/ACF fixtures for the Steam parser tests
packages/signatures/     games.json (game signatures) + the script that generates it
docs/                    spec, privacy notes, Windows testing checklist
.github/workflows/ci.yml Linux (fmt/clippy/tests incl. server on Postgres/sim) + Windows (tests + unsigned NSIS/MSI installers)
docker-compose.yml       local Postgres (+ optional server container) for development
```

## How detection works (read-only)

Every 5 s Fraglink takes a process snapshot (Windows Toolhelp, plus `QueryFullProcessImageNameW` only for new PIDs). It then checks, in this order:

1. **Your manual tags.** Programs you marked "This is a game".
2. **Signatures.** Exe names from `packages/signatures/games.json`, some with required path fragments.
3. **Steam library.** Any process running from `steamapps/common/<installdir>` of an installed Steam game. The game list comes from `libraryfolders.vdf` and `appmanifest_*.acf`, so no per-game entries are needed. Known helpers (crash reporters, launchers, redistributables) are skipped.
4. **Epic Games Store and GOG Galaxy installs.** Any process running from the install folder of a game listed in the Epic launcher's manifests (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item`) or GOG's registry list (`HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`). DLC, engines and overly broad folders are skipped.
5. **Steam's `HKCU\Software\Valve\Steam\RunningAppID`.** Only trusted while `steam.exe` is running.

Excluded games are ignored. **Fraglink never reads game memory, injects, hooks or overlays anything**, so it is safe with anti-cheat. It only lists process names and paths and reads the launcher files above. `GetLastInputInfo` gives the idle time.

**Idle trimming:** if you are idle longer than the threshold (default 15 min, configurable in Options) while a game runs, that whole idle stretch is removed from the session. Minimised games still count unless you are idle.

**Sessions:** a session ends after 2 missed polls (about 10 s). Sessions shorter than 2 min are dropped (configurable). A relaunch within 10 min continues the previous session. The open session is checkpointed to SQLite every 60 s and recovered after a crash or power loss. Gaps between polls longer than 60 s (sleep/hibernate) add no time.

## Build from source

Prerequisites: Rust stable (1.94+) and Node 20+. On Windows, WebView2 (preinstalled on Windows 10/11) and the MSVC build tools. On Linux, the Tauri deps: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.

```bash
cd apps/desktop
npm ci
npm run dev          # run the app against real processes
npm run dev:sim      # run the app with a fake process source (simulated games start/stop)
npm run build        # release build + installers (Windows: NSIS .exe and .msi in target/release/bundle)
```

The simulation can also be switched on with `FRAGLINK_SIM=1` or `fraglink --sim [--sim-script "<script>"]`. It uses a separate database (`<data dir>/sim/`).

Core crate only (works on any OS, no GUI deps):

```bash
cargo test -p fraglink-core
cargo run --release -p fraglink-core --bin fraglink-sim -- --fast            # simulated day, prints events + stats
cargo run --release -p fraglink-core --bin fraglink-sim -- --system --duration 60   # watch real processes for 60 s
cargo run --release -p fraglink-core --bin fraglink-sim -- --help
```

Server (M2, local development; needs Postgres, e.g. `docker compose up -d db`):

```bash
export DATABASE_URL=postgres://fraglink:fraglink@127.0.0.1:5432/fraglink
cargo run -p fraglink-server      # http://localhost:8787
cargo test -p fraglink-server     # fresh database per test, Steam mocked
```

Details, env vars and the API: [`apps/server/README.md`](apps/server/README.md).

Simulation script format: `exe[:seconds[:idle]]` or `steam:<appid>:<seconds>`, comma-separated. `-:<seconds>` means nothing is running. Example: `cs2.exe:150,-:20,steam:570:150,C:/Games/X/X.exe:120,Wow.exe:180:idle`.

## Where data lives (Windows)

| What | Path |
|---|---|
| Database (SQLite) | `%APPDATA%\gg.fraglink.desktop\fraglink.db` |
| Logs | `%LOCALAPPDATA%\gg.fraglink.desktop\logs\fraglink.log` |
| Exports ("Export my data") | `Documents\Fraglink\fraglink-export-<timestamp>.json` / `-sessions.csv` |

The tray menu has **Open data folder**, **Open logs folder** and **Export my data**.

## Releases and code signing

CI builds **unsigned** installers on every push to `main` and on `v*` tags. They are uploaded as Actions artifacts named `fraglink-windows-installers-<sha>`. Code signing is **not set up yet**, so Windows SmartScreen will warn ("Windows protected your PC"). Click **More info → Run anyway**. Signing (e.g. Azure Trusted Signing or an OV certificate) and an auto-updater are planned before any public release.

## Window notes

The main window is the 290×381 classic client (scaled 100–200 %, default 150 %). It is **frameless and transparent**, so the SVG chassis shapes the window. The transparent corners are not click-through yet, so clicks there hit the (invisible) window. Closing or minimising sends it to the tray. The webview is destroyed while hidden to keep memory low, and recreated when you open it again.

## Documentation

- [Roadmap](docs/ROADMAP.md) · [Changelog](CHANGELOG.md) · [Product spec](docs/fraglink-mvp-spec.md)
- [Windows testing checklist](docs/testing-windows.md) · [Privacy](docs/privacy.md) · [Signatures](packages/signatures/README.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## License

**All rights reserved; license to be decided before public release.** See [`LICENSE`](LICENSE). Bundled fonts keep their own licences (Russo One: SIL OFL 1.1; Wine Tahoma-metric fonts: LGPL 2.1+).

## Not affiliated with Xfire

Fraglink is an independent project inspired by the classic Xfire client. It is **not affiliated with, endorsed by or connected to Xfire** or its past or present owners. "Xfire" is a trademark of its respective owner. The Fraglink name, mark and UI artwork are original. Game names are used only to identify games and belong to their owners.
