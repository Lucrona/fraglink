# Changelog

All notable changes to Fraglink are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to use [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once public releases begin.

## [Unreleased]

## [0.2.0-dev.5] - 2026-09-28

Pre-release for private testing (builds report `0.2.0-5`).

### Added
- **Installed games from more launchers** (Options > Games, Tools > Launch, the info panel), each tagged with its launcher:
  - **Battle.net:** the Blizzard entries in the Windows Uninstall list (HKLM and HKCU, 64- and 32-bit views; the product comes from `--uid=` in the uninstall command) and `%ProgramData%\Battle.net\Agent\product.db`. Products map to WoW (retail / Classic / Classic Era), Diablo II: Resurrected, III and IV, Overwatch 2, Hearthstone, StarCraft Remastered and II, Warcraft III, Heroes of the Storm and the Call of Duty titles. Launch runs `Battle.net.exe --exec="launch <code>"` (e.g. `WoW`, `Pro`, `Fen`), or opens `battlenet://<code>` if Battle.net.exe isn't found.
  - **EA app:** Uninstall entries from Electronic Arts / EAInstaller; launched through the game's exe.
  - **Ubisoft Connect:** `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs` and `Uplay Install <id>` entries; launched with `uplay://launch/<id>/0`.
  - **Xbox / Microsoft Store:** folders under `<drive>:\XboxGames\<game>\Content` (the Xbox app's default); launched through a known game exe if one is found.
  - **Any other installer:** an Uninstall entry counts when its name matches a game in the signature list, or one of that game's exes sits in its install folder (up to three folders deep).
  - Epic and GOG were already covered; they now share the same launch rules.
- A running game inside any of these folders is detected even if its exe isn't in the signature list (like Steam, Epic and GOG before).
- Folded / unfolded groups in the friends list are remembered across restarts.

### Changed
- The info panel changes only when you click a row, the same for every group. A clicked friend of a friend shows their details plus a **Mutual Friends** section; hovering no longer changes the panel.
- Friends list tooltips only appear when the text is cut off, and the "Right-click for options" tooltip on group headings is gone.
- Options > Games shows the launcher name next to each game (Steam, Battle.net, EA app, Ubisoft Connect, Xbox, Epic, GOG).

## [0.2.0-dev.4] - 2026-09-28

Pre-release for private testing (builds report `0.2.0-4`).

### Added
- **New logo, "Split Orbit"**: app icon in every Tauri size plus `icon.ico` (16/32 px hand-checked, flat white mark on a dark tile; chrome mark at larger sizes), tray icon, NSIS installer header/sidebar, the header lockup in the dark client (flat white, Saira Expanded Bold wordmark), the banner, toasts and the README. Brand files and the Saira OFL 1.1 note are in `assets/brand/`.
- Menus laid out like the classic client. **Fraglink**: Always on Top, My Stats (opens the website address, a placeholder for now), Sign Out, Close (to the tray), Exit. **Tools**: Add Friend, Options, Launch (installed games), Skin, Theme (Cobalt, Crimson, Emerald, Graphite accents), Friends (show offline friends, show friends of friends, show hidden categories, info panel). **Help**: Contents, About.
- Friends list has three columns (Name, Status, Game Info) and the groups **Friends Online**, **Friends of Friends Playing**, **Friends Offline** (plus Friend Requests when there are any), with counts. Right-click a group heading for **Add Custom Group** and **Hide Category**. Clicking a heading selects it; clicking its arrow folds it.
- Status dropdown: shows **Playing <game>** automatically while you're in a game, then Online, Away, Invisible, your custom status line and **Set Custom Status…**.
- Context-aware info panel. With nothing selected it lists the games installed on this PC with **Launch** links and a **Detect Games** button. A selected group heading shows a line of help, a selected friend shows their details, and hovering a friend of a friend shows your mutual friends. Recent activity stays hidden until the website hosts it.
- Game launching: Steam games via `steam://rungameid`, Epic via the Epic launcher, GOG and other games via their .exe (the path you set, or the one Fraglink saw running), with optional extra arguments.
- 15 original fallback icons by genre (shooter, RPG, MMO, strategy, survival, sandbox, action/platformer, battle royale, puzzle, MOBA, racing, horror, card, fighting, sports) for games without an icon of their own. Every game in the signature list now has a genre (list `2026.09.28.1`).

### Changed
- **Options is now a compact tabbed dialog** (470x560, Windows-style group boxes, OK / Cancel) replacing the window with a category list. Tabs: General, Privacy, Sounds, Games, Appearance, Tracking, Account.
  - General: nickname, launch at startup, start minimised, minimise to tray, show offline friends, show friends of friends, notification alerts, language, Advanced (install info, data folder).
  - Privacy: show my game to friends / on my profile / to friends of friends, Away after N idle minutes, blocked users with Add and Remove.
  - Sounds: friend sign-in/out sounds, sound pack.
  - Games: a detection progress bar, then a tree of installed and not-installed games. Selecting a game shows its game path (with Browse…), detect path, extra launch arguments and "Don't track this game". **Add game manually…** tags a running program as a game (replaces Fraglink > This is a game… / Manage games).
  - Appearance: skin, theme accent, Classic Silver scale, always on top. Tracking: idle cut-off, minimum session, pause, export. Account: sign in / out.
- The Chat button is gone from the bottom bar and the info panel (chat is out of scope).

### Settings that are saved but not used yet
- General: **Nickname**, **Language**.
- Privacy: **Show the games I play on my profile**, **Show my game to friends of my friends**, **Show me as Away after N minutes** (the tracker still stops counting idle time using the Tracking setting; your status doesn't change automatically yet).
- Sounds: **friend sign-in/out sounds** and **sound pack** (no sounds play yet).
- **Custom status** shows only in your own window; friends don't see it yet (the presence protocol only carries Online/Away/Invisible and the game).
- **Custom groups** are empty headings; moving friends into them comes later.
- **Friends of Friends Playing** only has entries in the sample list; the server doesn't send friends of friends yet, so mutual friends on hover only work with sample data.
- **My Stats** opens `https://fraglink.gg/u/<you>`, a placeholder address.

Wired up: launch at startup, start minimised, minimise to tray, show offline friends / friends of friends, notification alerts, show my game to friends (off = friends see you online without a game), always on top, theme, skin, per-game path / launch arguments / don't track, blocked users, sign out, and the Tracking settings.

## [0.2.0-dev.3] - 2026-09-28

Pre-release for private testing (builds report `0.2.0-3`).

### Fixed
- Windows: opening Options showed a blank white window that couldn't be closed and hung the app. Cause: the window was created inside a synchronous Tauri command. Those run on the main thread, and building a WebView2 window there deadlocks. Windows (main, Options, toast) are now always created from a worker thread, and the commands that can open windows (`open_options`, `toast_clicked`) are async. This also covers reopening the main window from the tray, a toast click or a second launch.

## [0.2.0-dev.2] - 2026-09-28

Pre-release for private testing (builds report `0.2.0-2`; MSI needs a numeric pre-release).

### Changed
- **New default look: a dark, resizable main window** laid out like the 2013-era gaming IM clients: black header with the Fraglink logo, minimise and close; Fraglink/Tools/Help menu; banner; a status row with your name and an Online/Away/Invisible dropdown; vertical icon tabs down the left; a blue section header over the list; a bottom icon action bar (Add Friend, Chat, Profile, Join, Options); and the info panel docked on the right. All Fraglink's own CSS and art.
- The main window can be resized from its edges and the bottom-right grip. The list stretches with it. The minimum (and default) size is 333x555 (+12 px for the info-panel nub), and the size and position are remembered. Opening the info panel widens the window by 180 px.
- The Classic Silver skin stays available (Options > Appearance or Tools > Skins) at its fixed size, with the Scale setting. Existing installs switch to Dark once.
- **Options is now its own window** (resizable, 640x480 by default) with a category list on the left: General, Account, Tracking, Notifications, Privacy, Appearance, Advanced, and OK / Cancel / Apply buttons. Changes are applied on OK or Apply, and the main window follows them straight away. The old overlay inside the main window is gone.
- Game icons taken from a game's exe are extracted again when the exe changes (its modified time or size), so a game update that changes its icon is picked up.
- Tray icon: a single or double left click now always shows the main window (it is recreated if it was closed to the tray, restored if minimised, and focused). Before, a double-click opened and closed it again. Starting Fraglink a second time focuses the running copy (single-instance).
- Windows installer: Fraglink-branded NSIS header and sidebar images, installer icon and a "Fraglink" Start menu folder. It still installs per user without admin rights and offers "Run Fraglink" on the last page.
- Tray client: the Stats and Activity tabs are hidden for now, so only Friends shows. Their buttons are removed, so no empty tab slots are left. The code is kept behind a feature flag (`FEATURES` in `apps/desktop/ui/app.js`). To turn them back on, set the flag to `true`, or for testing use `?features=stats,activity` or localStorage `fl-features`. Friend toasts ("has started playing") are unchanged.

## [0.2.0-dev] - 2026-09-28

Pre-release for private testing. Builds report version `0.2.0-1` because the MSI installer requires a numeric pre-release identifier. Unsigned Windows installers (NSIS + MSI) from the tag's CI run: https://github.com/Lucrona/fraglink/actions/runs/36334311018 (artifact `fraglink-windows-installers-7b9330a83854c9ff2a03b0f1017298df9b073557`).

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
