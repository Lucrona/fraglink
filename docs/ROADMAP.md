# Roadmap

Fraglink is built in milestones. Durations are rough estimates for a solo developer, and the order can change. The full plan is in [`fraglink-mvp-spec.md`](fraglink-mvp-spec.md) §11.

| | Milestone | What it delivers | Status |
|---|---|---|---|
| **M0** | Setup | Monorepo, Tauri 2 app running the classic UI, tray, single instance, autostart, CI with Windows installers | ✅ Done (unsigned builds) |
| **M1** | Local tracker + tray tabs | Game detection (signatures + Steam, Epic and GOG libraries), idle trimming, local SQLite, tagging/exclusions, Stats and Activity tabs, local toasts, export | ✅ Done (0.1.1). The Stats and Activity tabs are hidden in the 0.2 client for now and return with the website's stats |
| **M2** | Accounts + Steam + trust tiers | Sign-in (email/Discord), Steam link and hours import, offline-safe sync, Tracked / Imported / Estimated hours, export v1, privacy policy | 🚧 Mostly done: accounts (username/password in the client from dev.6, email optional, rate-limited), browser pairing, sync, tiers, Steam import (verified link, daily refresh), JSON + zip/CSV export, draft privacy policy, hosted server (dev.6 default). Still to do: Discord login, email verification/reset, HTTPS domain, legal review of the policy |
| **M3** | Friends + presence + feed | Friend requests and blocks, live presence (Online/Away/Invisible), Friends tab and side panel, friend toasts, activity feed, Steam friends presence | 🚧 In progress: user search and requests (dev.6), accept/remove/block, WebSocket presence with Invisible, friends list with groups, tooltip, right-click menu and info panel, sign-on toasts. Still to do: activity feed (friends' events), Steam friends presence, live web friends page, auto-away, "Message on Discord" |
| **M4** | Website core | `fraglink.gg` profiles (drag/hide/reorder, themes), gaming history with trust markers, onboarding with the history estimator, linked accounts, signature images | Planned |
| **M5** | WoW + imported hours | Battle.net link and verified character cards, Raider.IO, Fraglink Played addon for exact `/played` import | Planned |
| **M6** | Website depth | Game pages, play-time achievements, weekly recap, rig card, Twitch live badge and OBS widget, opt-in Discord Rich Presence | Planned |
| **M7** | Beta hardening | Code signing, auto-updater, opt-in crash reports, download page, ToS/privacy policy and shutdown pledge, closed beta | 🚧 Signed auto-update is in (dev.6). The rest is planned |
| v1.1 | Post-beta | Supporter tier (cosmetics only, stats stay free), yearly recap | Later |

## Never planned

- In-game overlays, DLL injection or reading game memory, because they risk anti-cheat bans.
- Keyboard hooks.
- Chat, voice or clans (Discord does that).
- Ads and gambling mechanics.
