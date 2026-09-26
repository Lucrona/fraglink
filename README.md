# Fraglink

A lightweight Windows tray companion for PC gamers. It tracks your play time locally and (later) shows what your friends are playing. It uses the classic-client UI from the prototype.

**Status:** milestones **M0 (setup)** and **M1 (local hours tracker + tray tabs)**. There are no accounts, no friends backend and no network calls yet. See [`docs/fraglink-mvp-spec.md`](docs/fraglink-mvp-spec.md) for the full plan.

> Private repository. Licence: **All rights reserved** (placeholder, still to be decided; see `LICENSE`).

## Layout

```
apps/desktop/            Tauri 2 app
  ui/                    webview frontend: plain HTML/CSS/JS from the prototype, no bundler
  src-tauri/             Rust shell: tray, windows, commands, tracker thread
crates/fraglink-core/    detector, Steam library reader, tracker, SQLite store, stats, export, simulation
  src/bin/fraglink-sim.rs  headless runner for testing on any OS
  tests/fixtures/steam/  VDF/ACF fixtures for the Steam parser tests
packages/signatures/     games.json (game signatures) + the script that generates it
docs/                    spec, privacy notes, Windows testing checklist
.github/workflows/ci.yml Linux (fmt/clippy/tests/sim) + Windows (tests + unsigned NSIS/MSI installers)
```

## How detection works (read-only)

Every 5 s Fraglink takes a process snapshot (Windows Toolhelp, plus `QueryFullProcessImageNameW` only for new PIDs). It then checks, in this order:

1. **Your manual tags.** Programs you marked "This is a game".
2. **Signatures.** Exe names from `packages/signatures/games.json`, some with required path fragments.
3. **Steam library.** Any process running from `steamapps/common/<installdir>` of an installed Steam game. The game list comes from `libraryfolders.vdf` and `appmanifest_*.acf`, so no per-game entries are needed. Known helpers (crash reporters, launchers, redistributables) are skipped.
4. **Steam's `HKCU\Software\Valve\Steam\RunningAppID`.** Only trusted while `steam.exe` is running.

Excluded games are ignored. **Fraglink never reads game memory, injects, hooks or overlays anything**, so it is safe with anti-cheat. It only lists process names and paths and reads the Steam files above. `GetLastInputInfo` gives the idle time.

**Idle trimming:** if you are idle longer than the threshold (default 15 min, configurable in Options) while a game runs, that whole idle stretch is removed from the session. Minimised games still count unless you are idle.

**Sessions:** a session ends after 2 missed polls (about 10 s). Sessions shorter than 2 min are dropped (configurable). A relaunch within 10 min continues the previous session. The open session is checkpointed to SQLite every 60 s and recovered after a crash or power loss. Gaps between polls longer than 60 s (sleep/hibernate) add no time.

## Build and run

Prerequisites: Rust stable (1.85+) and Node 20+. On Windows, WebView2 (preinstalled on Windows 10/11) and the MSVC build tools. On Linux, the Tauri deps: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.

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

## Privacy

Everything stays on this PC. M1 has no telemetry, no analytics and no network calls. See [`docs/privacy.md`](docs/privacy.md).

## Window notes

The main window is the 290×381 classic client (scaled 100–200 %, default 150 %). It is **frameless and transparent**, so the SVG chassis shapes the window. The transparent corners are not click-through yet, so clicks there hit the (invisible) window. Closing or minimising sends it to the tray. The webview is destroyed while hidden to keep memory low, and recreated when you open it again.
