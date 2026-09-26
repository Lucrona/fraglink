# Changelog

All notable changes to Fraglink are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to use [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once public releases begin.

## [Unreleased]

## [0.1.1] - 2026-09-26

M1 polish.

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
