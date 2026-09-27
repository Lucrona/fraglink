# Roadmap

Fraglink is built in milestones. Durations are rough estimates for a solo developer, and the order can change. The full plan is in [`fraglink-mvp-spec.md`](fraglink-mvp-spec.md) §11.

| | Milestone | What it delivers | Status |
|---|---|---|---|
| **M0** | Setup | Monorepo, Tauri 2 app running the classic UI, tray, single instance, autostart, CI with Windows installers | ✅ Done (unsigned builds) |
| **M1** | Local tracker + tray tabs | Game detection (signatures + Steam, Epic and GOG libraries), idle trimming, local SQLite, tagging/exclusions, Stats and Activity tabs, local toasts, export | ✅ Done (0.1.1), in testing |
| **M2** | Accounts + Steam + trust tiers | Sign-in (email/Discord), Steam link and hours import, offline-safe sync, Tracked / Imported / Estimated hours, export v1, privacy policy | 🚧 Mostly done locally: email accounts, tray pairing + sync, tiers, Steam import (verified link, daily refresh), JSON + zip/CSV export, tray account totals, draft privacy policy. Still to do (need external accounts or a decision): Discord login, email verification/reset, hosting, legal review of the policy |
| **M3** | Friends + presence + feed | Friend requests and blocks, live presence (Online/Away/Invisible), Friends tab and side panel, friend toasts, activity feed, Steam friends presence | 🚧 In progress: requests/accept/remove/block, WebSocket presence with Invisible, real Friends tab + side panel, friend toasts (local server). Still to do: activity feed (friends' events), Steam friends presence, live web friends page, auto-away |
| **M4** | Website core | `fraglink.gg` profiles (drag/hide/reorder, themes), gaming history with trust markers, onboarding with the history estimator, linked accounts, signature images | Planned |
| **M5** | WoW + imported hours | Battle.net link and verified character cards, Raider.IO, Fraglink Played addon for exact `/played` import | Planned |
| **M6** | Website depth | Game pages, play-time achievements, weekly recap, rig card, Twitch live badge and OBS widget, opt-in Discord Rich Presence | Planned |
| **M7** | Beta hardening | Code signing, auto-updater, opt-in crash reports, download page, ToS/privacy policy and shutdown pledge, closed beta | Planned |
| v1.1 | Post-beta | Supporter tier (cosmetics only, stats stay free), yearly recap | Later |

## Never planned

- In-game overlays, DLL injection or reading game memory, because they risk anti-cheat bans.
- Keyboard hooks.
- Chat, voice or clans (Discord does that).
- Ads and gambling mechanics.
