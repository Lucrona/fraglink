<p align="center"><img src="docs/images/hero.png" alt="Fraglink" width="100%"></p>

<p align="center">A small Windows tray client that tracks your hours in every launcher and shows what your friends are playing.<br>
<sub>Early development · current pre-release <b>0.2.0-dev.6</b> · <a href="https://github.com/Lucrona/fraglink/releases">Releases</a> · <a href="CHANGELOG.md">Changelog</a></sub></p>

---

## What is Fraglink?

Fraglink is a **lightweight tray client for Windows, inspired by Xfire**. It notices which game you're playing on any launcher (Steam, Battle.net, Epic, EA, Ubisoft, GOG, Xbox or standalone), keeps an honest record of your play time, and shows your friends list with who is online and what they're playing. It wears a dark, 2013-style client UI.

The client stays small on purpose. The bigger things (profiles, full stats and history, game pages) are planned for the website, fraglink.gg, which comes later.

**How it fits with Discord:** Fraglink doesn't do chat, voice or overlays. Discord already does that well. Fraglink adds the part Discord doesn't have: your hours across every launcher and a "who's playing what" list. Discord sign-in and "Message on Discord" are on the roadmap.

<p align="center">
<img src="docs/images/client-friends.png" alt="Main window: friends list" width="300">
&nbsp;
<img src="docs/images/client-info-panel.png" alt="Main window with the info panel open on a friend" width="446">
<br><sub>The main window with the friends list, and with the info panel open on a friend. Browser preview of the dev.6 UI with sample data.</sub>
</p>

## What works today (0.2.0-dev.8)

- **Game detection on every launcher:** installed Steam, Epic, GOG, Battle.net, EA app, Ubisoft Connect and Xbox games, 170 curated signatures, and "This is a game…" for anything else. Read-only: no injection, no memory reading.
- **Honest hours:** idle time is trimmed, very short sessions are dropped, and sessions survive crashes. Sessions sync to your account when you're signed in and stay local otherwise.
- **Accounts in the client:** log in with username (or email) and password, or **Create Account** right in the main window. Browser pairing is still there as an option.
- **Friends:** search users and send requests, accept or decline, remove or block. Live status (Online / Away / Invisible) and the game each friend is playing. Friend tooltips, right-click menu, sign-on toasts.
- **Info panel:** your installed games with **Launch** links, or details of the friend you click.
- **Classic tray menu:** Launch (installed games), Skin, Status, Log Off, Exit.
- **Signed auto-update:** checks at startup and every 6 hours. Updates are signature-checked before they install.
- **Export everything** (JSON + CSV) from Options at any time.

Stats and Activity views exist in the code but are hidden for now. They return with the website's stats.

<p align="center">
<img src="docs/images/client-login.png" alt="Login screen" width="260">
&nbsp;
<img src="docs/images/client-create-account.png" alt="Create Account screen" width="260">
<br><sub>Signed out: Login and Create Account, inside the main window.</sub>
</p>

<p align="center">
<img src="docs/images/tray-menu.png" alt="Tray menu with the Launch submenu open" width="385">
&nbsp;
<img src="docs/images/client-toast.png" alt="Friend started playing toast" width="226">
<br><sub>Tray menu (a native Windows menu; drawn from the dev.6 menu definition) and a friend toast.</sub>
</p>

## Roadmap

| | Planned | Status |
|---|---|---|
| **Hours with trust tiers** | Every number labelled **● Tracked** (seen by the client), **◆ Imported** (e.g. Steam) or **~ Estimated** (self-reported history). Leaderboards count Tracked only. | Server side done; client views return with the website |
| **Friends and presence** | Requests, live status and game, friend toasts | ✅ In dev.6. Still to come: friends' activity feed, Steam friends presence |
| **Discord** | Sign in with Discord; "Message on Discord" from a friend's menu (Fraglink has no chat of its own) | Planned |
| **Website (fraglink.gg)** | Auto-filled profiles, gaming history, stats, signature images, game pages | Planned |
| **WoW character cards** | Verified characters via Battle.net, exact `/played` import | Planned |
| **Beta** | Code-signed installers, download page, closed beta | Planned (auto-update is already in) |

**Not planned:** chat, voice, clans or in-game overlays. Discord and Steam already cover those, and overlays and injection risk anti-cheat bans. Details in the [roadmap](docs/ROADMAP.md).

<p align="center"><img src="docs/images/website-concept.png" alt="Concept mock-up of a fraglink.gg profile" width="640"><br><sub>Concept mock-up of a fraglink.gg profile (not built yet; all data fictional).</sub></p>

## Look and feel

<p align="center">
<img src="docs/images/app-icon.png" alt="Fraglink app icon" width="128">
&nbsp;&nbsp;
<img src="docs/images/installer.png" alt="Installer sidebar and header art" width="420">
<br><sub>The silver app icon and the dark installer art. The logo is "Split Orbit": an original italic F with a split orbit ring (sources in <a href="assets/brand/">assets/brand/</a>).</sub>
</p>

## Privacy stance

- **No game memory reading, no injection, no overlays, no packet sniffing.** Fraglink only lists running processes and reads documented launcher files, so it is safe alongside anti-cheat (VAC, EAC, BattlEye, Vanguard, Ricochet).
- **No keyboard or mouse hooks.** Idle detection uses Windows' `GetLastInputInfo`, which only gives the time since your last input, never what you typed.
- **No telemetry.** Signed out, the only network request is the update check. Signed in, the client uploads game sessions (game, start, end, active time) and your presence, never program names or paths.
- **Export-everything pledge.** All your data is exportable, free, at any time. If Fraglink ever shuts down, you'll get plenty of notice and a full export.

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
5. **Other launchers' install folders.** Battle.net, EA app, Ubisoft Connect and Xbox (`XboxGames`) installs, plus any Windows Uninstall entry that matches a known game.
6. **Steam's `HKCU\Software\Valve\Steam\RunningAppID`.** Only trusted while `steam.exe` is running.

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

## Releases and updates

Pre-releases are on the [releases page](https://github.com/Lucrona/fraglink/releases) (an NSIS `.exe`, which installs per user with no admin prompt, and an `.msi`). From dev.6 the client updates itself: updates are signed and checked before they install. The installers themselves are **not code-signed yet**, so Windows SmartScreen will warn ("Windows protected your PC"). Click **More info → Run anyway**. CI also builds installers on every push to `main` (Actions artifacts `fraglink-windows-installers-<sha>`).

## Window notes

The default Dark skin is a frameless, resizable window (345×555 at minimum). The info panel joins flush on the right and widens the window when it opens. The Classic Silver skin keeps the fixed 290×381 chassis (scaled 100–200 %). Closing or minimising sends Fraglink to the tray. The webview is destroyed while hidden to keep memory low.

## Documentation

- [Roadmap](docs/ROADMAP.md) · [Changelog](CHANGELOG.md) · [Product spec](docs/fraglink-mvp-spec.md)
- [Windows testing checklist](docs/testing-windows.md) · [Privacy](docs/privacy.md) · [Signatures](packages/signatures/README.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## License

**All rights reserved; license to be decided before public release.** See [`LICENSE`](LICENSE). Bundled fonts keep their own licences (Russo One: SIL OFL 1.1; Wine Tahoma-metric fonts: LGPL 2.1+). The wordmark is Saira Expanded Bold (SIL OFL 1.1), converted to outlines.

## Not affiliated with Xfire

Fraglink is an independent project inspired by the classic Xfire client. It is **not affiliated with, endorsed by or connected to Xfire** or its past or present owners. "Xfire" is a trademark of its respective owner. The Fraglink name, mark and UI artwork are original. Game names are used only to identify games and belong to their owners.
