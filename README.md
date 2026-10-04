<p align="center"><img src="docs/images/hero.png" alt="Fraglink" width="100%"></p>

<p align="center"><b>PLAY. CONNECT. TRACK.</b><br>A small Windows tray client that tracks your hours in every launcher and shows what your friends are playing.<br>
<sub>Early development · current pre-release <b>0.2.0-dev.12</b> · <a href="https://github.com/Lucrona/fraglink/releases">Releases</a> · <a href="CHANGELOG.md">Changelog</a></sub></p>

---

## What is Fraglink?

Fraglink is a **lightweight tray client for Windows** in the spirit of the classic gaming clients. It notices which game you're playing on any launcher (Steam, Battle.net, Epic, EA, Ubisoft, GOG, Xbox or standalone), keeps an honest record of your play time, and shows your friends list with who is online and what they're playing. It wears a dark, 2013-style client UI.

The client stays small on purpose. The bigger things (profiles, full stats and history, game pages) are planned for the website, playfraglink.com, which comes later.

**How it fits with Discord:** Fraglink doesn't do chat, voice or overlays. Discord already does that well. Fraglink adds the part Discord doesn't have: your hours across every launcher and a "who's playing what" list. Discord sign-in and "Message on Discord" are on the roadmap.

<p align="center">
<img src="docs/images/client-main.png" alt="Main window with the side panel: Activity Report and installed games, update notice in the title bar" width="400">
&nbsp;
<img src="docs/images/client-friend.png" alt="Main window with a friend selected: playing time and hours this week" width="400">
<br><sub>The main window with the side panel (Activity Report and installed games, green update arrow in the title bar), and with a friend selected. Browser preview of the dev.11 UI with sample data.</sub>
</p>

## What works today (0.2.0-dev.12)

- **Game detection like Discord's:** about 10,000 Windows games from Discord's public detectable list (refreshed daily through the Fraglink server, with a bundled copy for offline use), Fraglink's own signatures and folder rules (every WoW version and Star Citizen channel), installed Steam games, and a "Track <name> as a game?" question for an unlisted program that runs full screen for 5 minutes. Read-only: no injection, no memory reading.
- **WoW realm:** the realm you're on shows next to the game ("World of Warcraft · Frostmourne") for you and your friends.
- **Honest hours:** hours count all the time a game is running, like Steam: no idle trimming and no minimum session length. Sessions survive crashes. Sessions sync to your account when you're signed in and stay local otherwise.
- **Accounts in the client:** log in with username (or email) and password, or **Create Account** right in the main window. Change your email, password or display name, or delete your account, from Options › Account.
- **Friends:** search users and send requests, accept or decline, remove or block. Live status (Online / Away / Invisible) and the game each friend is playing, how long they've been playing and their hours this week. Friend tooltips, native right-click menus, sign-on toasts.
- **Side panel:** an **Activity Report** of what your friends played recently, your installed games with **Launch** links, or details of the friend you click (click empty space or press Esc to go back). Drag its edge to make it wider. Friends can turn off sharing their hours in Options › Privacy.
- **Updates like Discord's:** a small "Checking for updates…" window at startup installs a new build before the client opens, then finds your installed games. While Fraglink runs it checks every hour and downloads quietly; a small green arrow in the title bar (and a dot on the tray icon) restarts into it. **Help › Check for Updates** and the tray menu check on demand. Updates are signature-checked before they install.
- **Status menu:** Online, Away, Invisible, custom status and recent statuses. Automatic **Away** after 10 minutes without input, like Discord ("World of Warcraft (Away)" in a game). **Looking to Play** and **Music Status** are in the menu as early placeholders.
- **Options:** a dark Options window with a two-column Games tab (install folder and the exe Fraglink found), a **Connections** tab (Steam, Battle.net, Spotify and Discord, shown as "coming soon" for now), and a **Text size** setting.
- **Classic tray menu:** Open Fraglink, Launch (installed games, with their icons), Skin, Status, Check for Updates, Log Off, Exit.
- **Export everything** (JSON + CSV) from Options at any time.

Stats and Activity views exist in the code but are hidden for now. They return with the website's stats.

<p align="center">
<img src="docs/images/client-login.png" alt="Login screen" width="260">
&nbsp;
<img src="docs/images/client-create-account.png" alt="Create Account screen" width="260">
<br><sub>Signed out: Login and Create Account, inside the main window.</sub>
</p>

<p align="center">
<img src="docs/images/splash.png" alt="Startup window: Checking for updates" width="240">
<br><sub>The startup window checks for updates before the client opens (like Discord's).</sub>
</p>

<p align="center">
<img src="docs/images/client-status-menu.png" alt="Status menu with Looking to Play and Music Status" width="400">
<br><sub>The status menu (a native Windows menu in the app; drawn here by the browser preview, sample data).</sub>
</p>

<p align="center">
<img src="docs/images/options-games.png" alt="Options, Games tab" width="390">
&nbsp;
<img src="docs/images/options-connections.png" alt="Options, Connections tab with coming-soon services" width="390">
<br><sub>Options: the Games tab and the Connections tab (the Connect buttons are placeholders for now). Browser preview, sample data.</sub>
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
| **Friends and presence** | Requests, live status and game, friend toasts, playing time and hours this week | ✅ Done |
| **Activity Report** | What your friends played recently, in the side panel ("View More" for the full list) | ✅ First version in dev.9; full history comes with the website |
| **Looking to Play** | Tell friends you're up for a game | In the status menu since dev.9 (local only); sharing it with friends is next |
| **Steam hour import** | Connect Steam to import your existing hours as **◆ Imported** | Server side done; Connect button in Options › Connections coming soon |
| **Battle.net / WoW characters** | Verified WoW characters via Battle.net, exact `/played` import | Planned (placeholder in Connections) |
| **Spotify (Music Status)** | Show what you're listening to as your status | Planned (placeholder in Connections and the status menu) |
| **Discord** | Sign in with Discord; "Message on Discord" from a friend's menu (Fraglink has no chat of its own) | Planned |
| **Website (playfraglink.com)** | Auto-filled profiles, gaming history, stats, signature images, game pages | Planned |
| **Web installer** | Small download from the website that fetches the latest version | Once the domain is live |
| **Beta** | Code-signed installers, download page, closed beta | Planned (auto-update is already in) |

**Not planned:** chat, voice, clans or in-game overlays. Discord and Steam already cover those, and overlays and injection risk anti-cheat bans. Details in the [roadmap](docs/ROADMAP.md).

<p align="center"><img src="docs/images/website-concept.png" alt="Concept mock-up of a playfraglink.com profile" width="640"><br><sub>Concept mock-up of a playfraglink.com profile (not built yet; all data fictional).</sub></p>

## Look and feel

<p align="center">
<img src="docs/images/app-icon.png" alt="Fraglink app icon" width="128">
&nbsp;&nbsp;
<img src="docs/images/installer.png" alt="Installer sidebar and header art" width="420">
<br><sub>The silver app icon and the dark installer art (sidebar with the "PLAY. CONNECT. TRACK." slogan, and the header). The logo is "Split Orbit": an original italic F with a split orbit ring (sources in <a href="assets/brand/">assets/brand/</a>).</sub>
</p>

## Privacy stance

- **No game memory reading, no injection, no overlays, no packet sniffing.** Fraglink only lists running processes and reads documented launcher files, so it is safe alongside anti-cheat (VAC, EAC, BattlEye, Vanguard, Ricochet).
- **No keyboard or mouse hooks.** For the automatic Away status (10 minutes without input, like Discord) Fraglink asks Windows how long ago your last input was (`GetLastInputInfo`), never what you typed. Hours always count while a game is running.
- **No telemetry.** Signed out, the only network request is the update check. Signed in, the client uploads game sessions (game, start, end, active time) and your presence, never program names or paths. Friends see your recent games and weekly hours unless you turn that off in Options › Privacy.
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

Excluded games are ignored. **Fraglink never reads game memory, injects, hooks or overlays anything**, so it is safe with anti-cheat. It only lists process names and paths and reads the launcher files above.

**Hours:** since dev.9 a session counts all the time the game is running, like Steam, including minimised or idle time. (Earlier builds trimmed idle stretches; that was removed because it under-counted.)

**Sessions:** a session ends after 2 missed polls (about 10 s). Every session counts, however short (like Steam). A relaunch within 10 min continues the previous session. The open session is checkpointed to SQLite every 60 s and recovered after a crash or power loss. Gaps between polls longer than 60 s (sleep/hibernate) add no time.

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

The default Dark skin is a frameless, resizable window (345×555 at minimum). The side panel joins flush on the right and widens the window when it opens; drag its edge to resize it. Options › Appearance › Text size makes the Dark skin text larger. The Classic Silver skin keeps the fixed 290×381 chassis (scaled 100–200 %). Closing or minimising sends Fraglink to the tray. The webview is destroyed while hidden to keep memory low.

## Documentation

- [Roadmap](docs/ROADMAP.md) · [Changelog](CHANGELOG.md) · [Product spec](docs/fraglink-mvp-spec.md)
- [Windows testing checklist](docs/testing-windows.md) · [Privacy](docs/privacy.md) · [Signatures](packages/signatures/README.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## License

**All rights reserved; license to be decided before public release.** See [`LICENSE`](LICENSE). Bundled fonts keep their own licences (Russo One: SIL OFL 1.1; Wine Tahoma-metric fonts: LGPL 2.1+). The wordmark is Saira Expanded Bold (SIL OFL 1.1), converted to outlines.

## Independent project

Fraglink is an independent project, not affiliated with or endorsed by any game company, launcher or chat service. The Fraglink name, mark and UI artwork are original. Game names are used only to identify games and belong to their owners.
