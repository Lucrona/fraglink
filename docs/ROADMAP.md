# Roadmap

Fraglink is built in milestones, and the order can change. The original plan is in [`fraglink-mvp-spec.md`](fraglink-mvp-spec.md) §11; this page shows where things stand now. What changed in each release is in the [changelog](../CHANGELOG.md) and on [playfraglink.com/changelog](https://playfraglink.com/changelog).

**Now:** public beta. The app is at **0.3.0-beta.9.1** (27 October 2026) and the website is live at [playfraglink.com](https://playfraglink.com).

| | Milestone | What it delivers | Status |
|---|---|---|---|
| **M0** | Setup | Monorepo, Tauri 2 app, tray, single instance, autostart, CI with Windows installers | ✅ Done |
| **M1** | Local tracker | Game detection (Steam, Battle.net, Epic, GOG, EA, Ubisoft, Xbox, RSI and standalone games, plus a folder scan), local SQLite, tagging and exclusions, toasts, export | ✅ Done. The app's Stats and Activity tabs are switched off for now; your hours and activity are on the website instead |
| **M2** | Accounts and hours | Accounts, email verification and password reset, two-step sign-in, sign-in from the app, HTTPS at playfraglink.com, Steam link and hours import, **Tracked** and **Imported** hours, data export, privacy policy and terms | ✅ Done. Steam hours are imported when you press Import hours (no automatic refresh). The Estimated tier was dropped |
| **M3** | Friends and presence | Friend requests and blocks, live presence (Online / Away / Invisible / Looking to Play), automatic Away, groups, side panel, friend toasts, friends' activity, live friends page on the website, Message on Discord | ✅ Done. Still to do: Steam friends' presence |
| **M4** | Website core | Public profiles at `playfraglink.com/u/<name>`, games and hours, linked accounts (Steam, Battle.net, Discord) with a verified tick, privacy switches shared with the app, Miniprofile signature image, achievements, gaming rig, leaderboards, news | ✅ Live. Still to do: rearranging profile sections and profile themes |
| **M5** | World of Warcraft | Battle.net link, verified WoW characters with gear, raids and PvP, the [Fraglink addon](https://www.curseforge.com/projects/1766775) for exact `/played`, uploaded on the website | ✅ Live for Retail. WoW: Forever uploads open after it launches. Classic is tracked by the app; Classic uploads come later. Still to do: Raider.IO |
| **M6** | Website depth | Game pages, weekly recap, Twitch live badge and OBS widget, opt-in Discord Rich Presence | Planned. Per-game leaderboards are already live |
| **M7** | Beta | Auto-updater, download page, privacy policy, terms and 90-day shutdown notice, public beta, code signing, opt-in crash reports | 🚧 Public beta since 0.3.0-beta.1 (12 October 2026), with auto-update and the download page. Still to do: code-signed installers and opt-in crash reports (crash logs are kept on your PC for now) |
| v1.1 | After beta | Supporter tier (cosmetics only, stats stay free), yearly recap | Later |

## Never planned

- In-game overlays, DLL injection or reading game memory, because they risk anti-cheat bans.
- Keyboard hooks.
- Chat, voice or clans (Discord does that).
- Ads and gambling mechanics.
