# Changelog

All notable changes to Fraglink are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to use [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once public releases begin.

## [Unreleased]

## [0.2.0-dev] - 2026-09-28

Pre-release for private testing. Builds report version `0.2.0-1` because the MSI installer requires a numeric pre-release identifier.

M2 checkpoint (accounts, Steam import, trust tiers) and the first M3 checkpoint (friends and live presence). Local development only; nothing is hosted.

### Added (M3: friends and presence)
- **Friends on the server:** send a request by exact username (no directory), accept, decline, cancel, remove, block and unblock. Mutual requests are accepted automatically. Blocks and declines are silent, and requests are limited to 30 an hour. JSON API, plus a Friends card on the web dashboard.
- **Live presence over a WebSocket** (`GET /api/v1/ws`):
  - statuses: Online, Away, Invisible (shown to friends as offline)
  - "now playing" with a start time
  - a ping every 60 s, and you show as offline after 150 s of silence or when your last device disconnects
  - only friends get updates. The server pushes friend requests, accepts and removals live.
- **Tray Friends tab on real data** when you're signed in to sync:
  - sections for Playing, Online, Friend Requests (Accept / Decline) and Offline
  - side panel with Remove / Block / Mute toasts
  - an Add Friend dialog
  - Signed out, it keeps the sample friends, with a banner pointing to Sign in to sync.
- **Tray presence thread:** publishes your status (the status drop-down) and the game being tracked right now, reconnects with backoff, and stops when you sign out. Excluded games never appear.
- **Friend "started playing" toasts.** Options control friend pop-ups and "not while I'm playing" (on by default). You get at most one per friend per 30 min, and you can mute a friend.
- **Stats tab → My Account (all devices):** server totals per game with ● Tracked / ◆ Imported / ~ Estimated markers and a legend. It never shows less than this PC knows. When the server can't be reached, it falls back to local data.
- `fraglink-sync`: friends API, a blocking `PresenceSocket`, and `examples/presence.rs` (acts as a friend's tray for local testing).
- Tests: a multi-client WebSocket presence suite (fan-out, invisible, away, pings, second device, disconnects, idle timeout, pushes of requests/blocks) and a test driving the tray's own client against a real server.

### Added (game icons, Xfire style)
- **Real 16x16 game icons** next to game names in Friends (now playing and side panel), Stats (Now Playing, top games, account totals), Activity and the "started playing" toasts. Games with no icon get a neutral generic icon. The old lettered tiles are only used for the signed-out sample data.
- Icons are found on your own PC: the detected game's exe (Windows `ExtractIconExW`, read at runtime), then Steam's library cache (`appcache\librarycache\<appid>_icon.jpg` or the per-app folder). They're cached as 32x32 PNGs in `<data dir>\icons\`. Nothing is bundled.
- For a friend's game that isn't on your PC, only the game id travels over presence. Your tray asks the server for Steam's public icon (the hash recorded from Steam imports, `GET /api/v1/games/icons`) and downloads it from Steam's CDN (only that host, only while signed in), otherwise it shows the generic icon.
- In simulation mode, a generated placeholder stands in for the exe icon.

### Added (M2 leftovers)
- **Zip export** on both sides:
  - Tray: *Export my data* writes `fraglink-export-<time>.zip` (JSON + `sessions.csv` + `games.csv` + README). The sync token is never included.
  - Server: `GET /api/v1/export.zip` (profile, sessions, games per tier, hours entries, Steam playtime, friends), limited to once per hour. The JSON export also includes friends.
- **Delete account** form on the web dashboard (password + confirmation).
- `docs/privacy-policy.md`: **DRAFT v1**, plain English, matching what the code collects. Not legal advice.
- `.dockerignore`. The server image and `docker compose` were checked locally (see decisions D27).

### Fixed
- Toast window stays fully on screen when the desktop enforces a larger minimum window size.

### Added (M2)
- **`apps/server`**, a Rust server (axum + Postgres via sqlx) with migrations, `docker-compose.yml` for local Postgres, and a `.env.example` with no secrets.
  - Accounts: email + password (Argon2id), opaque tokens stored hashed, login rate limit, logout, immediate account deletion, JSON export.
  - Desktop pairing with a device code: the tray shows a code, you approve it in the browser, and the tray gets a revocable device token.
  - Session sync API: idempotent uploads, and it rejects sessions over 24 h, future or overlapping ones.
  - Trust tiers: **Tracked** (Fraglink sessions, or Steam's own counter if that is higher), **Imported** (a game's own counter you type in, marked self-reported) and **Estimated** (history estimator, one per game, never added to totals). Leaderboards use Tracked only.
  - Steam: link by profile URL/custom URL/SteamID64, or verified with "Sign in through Steam" (OpenID). Import via `GetOwnedGames` with the `STEAM_API_KEY` env var, a 10-min cooldown and a daily refresh. Private game details get clear instructions.
  - Minimal no-JavaScript pages: sign up/in, approve pairing, dashboard with tier markers (● ◆ ~), Steam, add hours, paired devices.
- **`crates/fraglink-sync`**, the blocking client the tray uses. The device token is encrypted with Windows DPAPI.
- Tray: **Options → Sync (optional)**. Sign in to sync, see sessions waiting and last sync, Sync now, Sign out. Off by default: no network activity until you sign in. It uploads every 5 min and after each session, and retries quietly when offline. Program names and paths are never uploaded.
- Local database schema v2: per-session sync state (outbox).
- `docs/decisions.md`: the product/technical defaults picked for M2 and the open questions.
- CI: Linux runs the server tests against a Postgres 17 service. Windows also tests the sync client (including DPAPI).


## [0.1.1] - 2026-09-26

M1 polish. Unsigned Windows installers (NSIS + MSI) were built by CI for tag `v0.1.1`: https://github.com/Lucrona/fraglink/actions/runs/36232852969 (artifacts kept until 25 Dec 2026).

### Added
- **Epic Games Store detection.** Games installed through the Epic launcher are recognised by install folder, read from its local manifests (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item`). DLC, Unreal Engine installs and incomplete installs are skipped.
- **GOG Galaxy detection.** Games listed under `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games` are recognised by install folder. DLC entries (`dependsOn`) are skipped. If GOG isn't installed, nothing happens.
- Epic and GOG games use the matching signature id when their main exe is known, so the same game isn't counted twice.
- Options → data info shows how many Steam, Epic and GOG games were found.
- `packages/signatures/SOURCES.md` documents where each previously-uncertain signature was verified.

### Changed
- All 20 `uncertain` signatures were checked against public sources: Steam app info (launch config), PCGamingWiki, official FAQs and support pages. None are uncertain now.
  - **Civilization VII** fixed: it runs `Civ7_Win64_DX12_FinalRelease.exe` (the old entry was the macOS name).
  - **THE FINALS** (`Discovery.exe`) and the **Black Myth: Wukong** launcher (`b1.exe`) now only count inside their own folders.
  - Added launcher/alternate exes for Marvel Rivals, Tekken 8 and S.T.A.L.K.E.R. 2.
  - Minecraft: Java is now detected when started from the Microsoft Store launcher (bundled Java runtime) and the Modrinth App.
- Play time that crosses midnight is now split at the exact second (it used to go to the day of the 5-second poll).


## [0.1.0] - 2026-09-26

First internal build: milestones **M0 (setup)** and **M1 (local hours tracker + tray tabs)**. Unsigned, for private testing only.

### Added

#### M0: setup
- Monorepo with `apps/desktop` (Tauri 2 + the classic-client prototype UI), `crates/fraglink-core`, `packages/signatures` and `docs/`.
- Tray icon and menu: Open, Status (Online/Away/Invisible, placeholder until friends arrive), Pause tracking, Export my data, Open data folder, Open logs folder, Quit.
- Single instance, close-to-tray, and optional start with Windows (off by default, starts hidden).
- Frameless, transparent 290×381 main window shaped by the SVG chassis. It scales 100–200 %, remembers its position, and is destroyed while hidden to save memory.
- GitHub Actions CI:
  - Linux: fmt, clippy, tests and a simulation smoke run.
  - Windows: clippy, tests, and unsigned NSIS + MSI installers uploaded as artifacts.

#### M1: local tracker
- Process detector polling every 5 s. It matches, in order:
  - manual tags
  - the curated signature list
  - installed Steam games (any process under `steamapps/common/<installdir>`, from `libraryfolders.vdf` + `appmanifest_*.acf`)
  - Steam's `RunningAppID`
- Idle trimming via `GetLastInputInfo`: idle stretches longer than the threshold (default 15 min) are removed. Minimised games still count.
- Session rules:
  - sessions under 2 min are dropped
  - a relaunch within 10 min continues the session
  - sleep/hibernate gaps add no time
- Local SQLite store (games, sessions, daily totals, activity, settings). The open session is checkpointed every 60 s and recovered after a crash.
- Seed signatures for 170 PC games (current and classic). Entries still to be verified are flagged `uncertain`.
- "This is a game…" manual tagging and per-game exclusions (Tools → Manage games).
- Stats tab on real data: Now Playing with a live timer, Today, This week vs last week, Streak, top games (7 days + total, "Tracked" marker).
- Activity tab showing your own local events: started/stopped playing, first time played, hour milestones (10/25/50/100/…).
- Bottom-right "You started playing…" toast (a stand-in for friend toasts).
- Options: idle threshold, minimum session length, pop-ups on/off, autostart.
- Export my data: JSON (everything) + sessions CSV to `Documents\Fraglink`.
- Simulation mode (`--sim`, `FRAGLINK_SIM=1`) and a headless `fraglink-sim` runner for testing without Windows or games.
- 42 unit tests covering signature matching, VDF/ACF parsing, idle trimming, session recovery, merging and aggregation.

### Not yet
- Friends tab shows sample data (friends arrive in M3). Website links show "coming later".
- No code signing, auto-updater or crash reporting (M7).
