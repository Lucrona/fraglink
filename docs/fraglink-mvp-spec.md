# Fraglink: MVP Product Spec & Technical Plan

*Product name: Fraglink (decided 25 Sep 2026; replaces the placeholder "Gamewire"). Author: Lucrona (solo/indie, Sydney). **Draft v0.2, 26 Sep 2026 (AEST).** The previous draft (v0.1, 25 Sep 2026) is kept as `fraglink-mvp-spec-v1.md`.*
*All effort figures, timings and thresholds here are **estimates or proposed targets**, not measured data. External facts were checked against the linked sources on 25–26 Sep 2026. Terms change, so re-check them before launch.*

> **What changed in v0.2 (product decisions made with Lucrona, 26 Sep 2026)**
> - **Direction:** Fraglink is for people who want to **track their gaming stats and see friends from anywhere**. The tray client becomes *tiny* (sensor + friends list + quick glance). The **website carries the bulk**: stats dashboard, profiles, history, friends, game pages, recaps, account linking and settings.
> - **Client tabs are now Friends | Stats | Activity.** Profile moves to the website. The Servers tab is deferred.
> - **Friends from anywhere:** Fraglink friends on any launcher (core), plus Steam friends' presence for public profiles (bonus).
> - **New v1 features:** WoW character cards (Battle.net verified), a three-tier **historic hours trust system** (Tracked / Imported / Estimated), miniprofile signature image, play-time achievements, game pages, activity feed, weekly recaps, drag/hide/reorder profile sections with 3–5 themes, Verified linked accounts, gaming rig card (v1 or soon after), and **export everything** plus a public shutdown data pledge.
> - **Onboarding** must never leave an empty profile: Steam import + historic-hours estimator in signup.
> - **Monetisation direction decided:** free core, optional cosmetic-only Supporter tier; never paywall stats; no ads-first, no gambling/coins.
> - **Explicit skip list** with reasons (§2.5). Milestones and estimates revised (§11): roughly **22–30 weeks FTE (estimate)**, up from 15–20.

---

## 1. Summary and target user

**Fraglink** gives PC gamers one place to **track their gaming stats across every launcher and see what their friends are playing, from anywhere.** It has two halves:

- **A tiny Windows tray client**: a sensor that detects which game is running (no injection, no overlay), a live friends list, a toast when a friend starts playing, and a quick glance at today's and this week's stats. It wears a faithful 2000s chrome UI (Classic Silver, Phoenix and StreamEngine skins) that already exists as a working HTML prototype.
- **The website (`playfraglink.com`)**, which does the heavy lifting: an auto-filled public profile, a full stats dashboard and gaming history, friends (viewable from any device, including phones), game pages, recaps, achievements, WoW character cards, linked accounts, a signature image for forums and stream panels, settings and data export.

It sits next to Discord, Steam and Twitch and doesn't replace them. Discord keeps chat, voice and communities. Steam keeps the store and its own friends list. Twitch keeps streaming. Fraglink adds what none of them does well: a single, launcher-agnostic record of your hours (with an honest trust label on every number), a cross-platform "who's playing what" list, and a profile you're proud to link.

**Why now / why this shape.** Xfire's most-loved feature was an accident: automatic hours on an auto-generated profile ("incredibly popular… essentially an afterthought", co-founder Chris Kirmse). It died partly from a heavy client and feature sprawl, and its users lost years of logged hours with 2 days' notice. The xf1re.com fan revival proves the nostalgia is real (~9,000 profiles) but also shows the failure mode: **41% of its profiles have no games at all**, and its WoW-character feature is dead (legacy Armory endpoints return 404). Fraglink fixes both: back-filled history on day one, and a working, verified WoW character card. See `/workspace/xfire-refs-hd/xfire-feature-history.md` and `/workspace/xf1re-refs/deep/xf1re-deep-dive.md`.

**Target user (v1):** PC gamers aged about 18 to 35 who play across several launchers, belong to one or more small Discord friend groups, and care about their hours as a trophy ("30k hours in WoW"). Segments:
- **Core:** friend groups of roughly 5 to 30 people who want to see who's on and compare hours.
- **Long-time players / MMO veterans:** want their historic hours (WoW /played, Steam totals, "raided every night through TBC") shown honestly on one profile.
- **Small streamers:** want a "now playing / session hours" widget, a signature image for Twitch panels and a live badge.

The beta seeds from Lucrona's own friend circle and Discord communities. Growth works through invites and shared profile links/signature images.

---

## 2. Product shape and v1 scope

### 2.1 Tray vs website: who does what

| Surface | Job | Contents in v1 |
|---|---|---|
| **Tray client** (Windows, Tauri 2) | Sensor + friends + quick glance. Must stay tiny. | Game detection and session logging (local-first), WoW character name/realm discovery from local folders, reading Fraglink addon SavedVariables for /played, gaming rig auto-detect (opt-in), friends list with live presence, "started playing" toasts, tabs **Friends / Stats / Activity**, status (Online/Away/Invisible), Pause tracking, links out to the website. |
| **Website** (Astro SSR on Cloudflare) | Everything else | Signup/onboarding (Steam import + estimator), profile (auto-filled, drag/hide/reorder sections, themes), stats dashboard, gaming history with trust tiers, friends page (web presence, works on mobile), game pages, activity feed, recaps, achievements, WoW characters, linked accounts, signature image, rig card, privacy settings, export, Supporter settings (post-beta). |

**Rule of thumb:** if a feature needs a lot of UI, it goes on the website and the client links to it.

### 2.2 Tray client features (v1) with acceptance criteria

| # | Feature | Acceptance criteria |
|---|---|---|
| T1 | **Tray app + classic UI shell** | Per-user signed NSIS installer, starts minimised to tray, optional launch at login, single instance. Main window is the prototype's 290×381 chassis (1×/1.5×/2× scale), frameless, draggable, remembers position. Tray menu: Open, Status (Online/Away/Invisible), Pause tracking, Open playfraglink.com, Quit. Idle footprint target (estimate to validate): under 1% CPU averaged over 10 min and under ~80 MB RAM with the window hidden (the WebView is destroyed when hidden). |
| T2 | **Game detection & session tracking** | Detects a game within 15 s of launch (target) and ends the session within 15 s of exit. Sessions under 2 min dropped (configurable). A 10-minute crash/relaunch gap merges sessions. **Idle trimming:** if there's no user input for N minutes (default 15, via `GetLastInputInfo`), the session pauses; this fixes Xfire's "left at the menu for 3 days = 72 h" bug. Sessions are written to local SQLite before any upload (offline-safe). Works for Steam, Epic, GOG, Battle.net, EA, Riot and standalone games when the exe is in the signature DB or manually tagged. |
| T3 | **Signature DB + manual tagging** | Ships with and auto-updates a curated exe→game DB (§7.2). Steam library scan (Epic/GOG manifests as a stretch). "This is a game…" tagging, per-exe exclusions, suggestions to the global DB. |
| T4 | **Friends tab** | Sections: *Playing* (game + session time), *Online*, *Steam friends playing* (bonus, W4), *Requests*, *Offline* (collapsible, sortable, as in the prototype). Presence change reaches online friends in under 5 s at p95 (target). Friend side panel shows game, session time, hours in that game, and (v1.1) a one-line WoW character ("Playing WoW as Fragnar – Frostmourne, 80 Hunter"). Chat button hands off to Discord (`discord.com/users/<id>`) if linked. |
| T5 | **Stats tab (quick glance)** | Shows **Now playing** (live timer), **Today** (total + per game), **This week** (total, vs last week), **Top games** (top 5: 7-day hours, total with trust marker, small bar), and a **"Full stats on playfraglink.com"** link. No sorting UI, no history: the website does that. Loads from local SQLite in under 200 ms (target). |
| T6 | **Activity tab (friends feed)** | Short, day-grouped feed ("Today", "Yesterday", weekday, "Earlier"): *started playing X*, *played X for the first time*, *hit 500 hours in X*, *unlocked an achievement tier*, *went live on Twitch*, *weekly recap ready*. Max ~50 items; each entry links to the relevant website page. Aggregated server-side (one "played 3 sessions of X" line rather than three). |
| T7 | **"Friend started playing" toast** | Skinned toast, bottom-right, 6 s. Click opens the friend in the side panel. Rate limit: at most one per friend per 30 min (configurable). Global mute, per-friend mute, Do-Not-Disturb while *I* am in a game (default on). |
| T8 | **Local sensors for the website** (all opt-in, each with its own toggle) | (a) **WoW characters:** reads *folder names only* under `World of Warcraft\_retail_|_classic_|_classic_era_\WTF\Account\<ACCOUNT>\<Realm>\<Character>\` and region from `WTF\Config.wtf`; uploads `{flavor, region, realm, character}` as *suggested characters*, never the account folder name or file contents. (b) **/played import:** reads the Fraglink addon's SavedVariables file (§5.3) from disk when WoW isn't running. (c) **Gaming rig:** CPU, GPU, RAM, OS, main monitor resolution via WMI/DXGI, uploaded once and on change, editable on the website. **Never touches game memory or game processes.** |
| T9 | **Skins** | Classic Silver (default), Phoenix, StreamEngine; runtime switch; synced so the web profile can match. |
| T10 | **Updater + crash reporting** | Signed auto-update (Tauri updater) with a beta channel; opt-in PII-free crash reports. |
| T11 | **Discord Rich Presence (opt-in)** | Unchanged from v0.1: labelled as *Fraglink*, `details` = game, `state` = "312 h total · via Fraglink", button "Fraglink profile"; default off. |

### 2.3 Website features (v1) with acceptance criteria

| # | Feature | Acceptance criteria |
|---|---|---|
| W1 | **Accounts + onboarding without empty profiles** | Sign up with email (magic link/password) or Discord OAuth; unique handle (3–20 chars). The signup wizard has 4 steps: (1) account, (2) **Link Steam** (imports owned games + playtime, target under 60 s), (3) **"Estimate your history"** (the calculator, §5.4, prefilled with the top Steam games plus a picker for non-Steam classics such as WoW), (4) download the tray client. Every step except (1) can be skipped, but a live profile preview shows it filling up. Target: under 15% of activated beta profiles with zero games (xf1re: 41%). |
| W2 | **Profile (auto-filled, xf1re-style)** | `/u/<handle>`. Two columns. Sections: Miniprofile/signature, Details (incl. linked accounts), Gaming History, WoW Characters, Achievements, Recent Activity, Friends, Gaming Rig, Data. Owner can **drag between columns, reorder, hide** (model: `{section_id, column, order, hidden}[]`, as xf1re does) and "preview as visitor". **3–5 themes** in v1 (Cyan Grid, Gold 567, plus 1–3 more, e.g. Phoenix Dark, StreamEngine Red, Classic Blue). Server-rendered, loads without JS, OG tags so links unfurl on Discord. |
| W3 | **Stats dashboard + gaming history** | Table of games with Past 7 Days, Total and a **trust tier marker** on every total (Tracked ● / Imported ◆ / Estimated ~, §5). Sortable; "Show entire gaming history"; Overall Hours footer shows *verified* (Tracked + Imported) and *estimated* separately. Charts: last 30 days by day, per-game history. Supporters get longer chart ranges (§10) but **all underlying stats and exports stay free**. Optional story note per game. |
| W4 | **Friends from anywhere** | Web friends page (responsive, works on phones) with live presence. **Core:** Fraglink friends on any launcher. **Bonus:** Steam friends' presence via `GetFriendList` + `GetPlayerSummaries` (`personastate`, `gameextrainfo`) for users who link Steam and friends whose profiles are public; separate "Steam friends" section with "Invite to Fraglink". Polled server-side, batched 100 SteamIDs per call, only while the viewer is online (budget §8.3). |
| W5 | **WoW character cards** | Battle.net OAuth (`openid wow.profile`) → list of characters → user ticks which to show. Card: portrait/render, name, realm, level, race, class, active spec, guild, equipped item level, achievement points, M+ score and raid progress (Raider.IO), last login, **"Verified via Battle.net"** badge. Classic flavours supported. Private-server characters as manual **Unverified** entries. Details in §6. |
| W6 | **Miniprofile signature image** | `/sig/<handle>.png` (default 440×110; plus a 350×20 "userbar" and a 320×100 Twitch-panel size), themed to match the profile: avatar, handle, status, This Week top game + hours and All Time top game + hours (with trust marker). Server-rendered, CDN-cached ≤10 min. Copy buttons for URL, BBCode, Markdown and HTML. **Where it works (honestly):** forums (BBCode) and GitHub READMEs hot-link it and it auto-updates; Twitch panels need an uploaded image (download button, refresh manually); Steam profiles and Discord bios don't embed external images, so users paste the profile link and Discord unfurls the OG card (same design). |
| W7 | **Play-time achievements** | Site-generic tracks, 5 tiers each (Bronze → Diamond), computed from Tracked (+ addon-captured Imported where noted) data only: *Clocked In* (total hours), *One More Round* (hours in one game), *No Days Off* (play-day streak), *Regular Circuit* (distinct active days), *Game Hopper* (games tried ≥30 min), *Old Guard* (account age), *Party List* (friends). Estimated hours never count. Site-wide tier stats ("3% of players have Diamond No Days Off"). |
| W8 | **Game pages** | `/g/<slug>`: header, 7-day community hours chart, **Fraglink rank** (#N by hours this week), total hours, **friends who play it**, top players (Tracked only, opt-in public profiles), **live Twitch streams** for the category (Helix Get Streams), JSON-LD `VideoGame` for SEO. |
| W9 | **Activity feed** | Same events as T6, full history with filters, on the website home and the profile "Recent Activity". |
| W10 | **Recaps** | Weekly recap (Mondays, user's timezone): hours, top games, streak, new games, friend comparisons; web page + tray notification + optional email. **Yearly recap** ("Your 2026 in games") shareable card: v1 if time allows, otherwise v1.1 (must ship before Dec 2026, its moment). |
| W11 | **Linked accounts with Verified badges** | Steam (OpenID), Discord (OAuth `identify`), Twitch (OAuth), Battle.net (OAuth). Each row: green **✓ Verified** badge, "show on profile" toggle, unlink. Above the fold on the profile (as xf1re does). Typed-in handles (Riot ID, Xbox gamertag, PSN ID) can be shown but are labelled **Unverified**. |
| W12 | **Gaming rig card** (v1 or soon after) | Auto-filled by the tray (T8c); user-editable free-text fields for peripherals. Hidden until the user reviews it. |
| W13 | **Privacy controls** | Profile visibility Public / Friends-only / Private (**default Friends-only**, open decision §12.3), per-section hide, **per-game hide** (hidden games never appear to others, including feeds and signature), **Invisible** mode (records hours, publishes no presence), block (hides from search, feed, profile), per-linked-account visibility. |
| W14 | **Export everything** | One click in Settings → Data: a zip with `sessions.csv`, `games.csv` (per-game totals by tier), `hours_entries.json` (all tiers with evidence metadata and estimator inputs), `characters.json`, `friends.json`, `activity.json`, `profile.json`, plus a documented JSON schema. **Any time, free**, at most 1 per hour. A headline feature on the landing page. |

### 2.4 Platform features (shared)

| # | Feature | Notes |
|---|---|---|
| P1 | **Steam link & hours import** | Steam OpenID 2.0; `GetOwnedGames` (`include_appinfo`, `include_played_free_games`), `GetRecentlyPlayedGames`, `GetPlayerSummaries`. Re-sync daily + on demand (≤1 per 10 min). Privacy guidance if game details are private. |
| P2 | **Twitch link + live badge + OBS widget** | EventSub `stream.online/offline`; LIVE badge for friends; `/w/<token>` OBS browser source. Unchanged from v0.1. |
| P3 | **Battle.net link** | OAuth 2.0 Authorization Code flow; scopes `openid wow.profile`. Characters refreshed on link, on the tray's "stopped playing WoW" event, and daily (§6). |
| P4 | **Deep links** | `fraglink://` for add friend, open profile, linked-account callbacks. |

### 2.5 Explicit skip list (with reasons)

| Skipped | Reason |
|---|---|
| **In-game overlay** (Xfire's XIG) | Needs DLL injection/graphics hooks. Kernel anti-cheat (Vanguard, EAC, BattlEye, Ricochet) treats unknown injected overlays as cheats, which means bans. Discord and Steam overlays already exist. Toasts instead. |
| **Keyboard hooks for auto-away** | Global keyboard hooks look like keyloggers (AV tools flagged Xfire's `xfire_toucan.dll`). Use the `GetLastInputInfo` idle-time API instead. |
| **Chat, voice, clans/communities, forums, file sharing** | Discord owns these; moderation, retention and age-safety burden is too high for a solo dev. The chat button hands off to Discord. |
| **Video capture / replay / streaming** | Needs graphics hooks; NVIDIA ShadowPlay/Instant Replay, Steam Game Recording and Xbox Game Bar already do it; Twitch does streaming. *Later:* a screenshot-folder watcher (Steam `userdata/*/760/remote`, Game Bar `Videos\Captures`, NVIDIA folders) that offers to attach shots to a session. |
| **Ads (ads-first model)** | Xfire's 2013 CEO: gamers "rarely click through the ads". Ads also clash with Twitch and Blizzard data terms (§10). |
| **Wagering, coins, loot boxes, sweepstakes** | Australian gambling rules; Xfire's Arcade/X-Coins were part of the strategy whiplash that preceded its decline. |
| **Servers tab** (deferred, not skipped) | Only community-server games can be supported, via Steam master server + A2S queries (Rust, DayZ, ARK, CS2/TF2 community servers, Squad; Minecraft via SLP). Matchmade games (Valorant, LoL, CS2 Premier, most modern shooters) can't be. Revisit after beta. |
| **Xbox / PSN integrations** | No open public API for third parties (Xbox is GDK-partner only; PSN has none); unofficial APIs carry ToS risk. Xbox yearly-recap hours can be entered as Imported with a screenshot. |
| **Reading game memory / packet sniffing** | Anti-cheat and privacy risk. Fraglink only enumerates processes with limited-query handles and reads documented files. |

### 2.6 Roadmap (indicative)

| Release | Candidates |
|---|---|
| **v1.1** (about 1–2 months after beta) | **Supporter tier** (§10). Yearly recap (if not in v1). WoW character line in the tray friend panel. Twitch category suggest. Friends-of-friends playing. Discord linked-account friend matching. Custom profile backgrounds. Public read API (profiles, signature, recaps). Screenshot-folder watcher. Paper-doll equipment view. |
| **v2** | **Servers tab** (A2S community-server games; favourite + friends' servers). More game accounts: **RuneScape/OSRS** hiscores (easy, no auth), **Guild Wars 2** (user API key), **Destiny 2** (Bungie OAuth), **Riot** (needs an approved production key + RSO). Epic/GOG hours import (mostly unofficial; evaluate). Curated downloadable client skins. Possibly macOS. |

---

## 3. Key user flows

**3.1 Web signup (the "no empty profile" path)**
1. Landing page → "Create your profile". Email or Discord.
2. **Link Steam** → OpenID → the server imports owned games + playtime → the live profile preview fills with games marked **Tracked** (Steam official).
3. **Estimate your history** → the calculator opens with the user's top Steam games plus a search box ("World of Warcraft", "Counter-Strike 1.6", "RuneScape"). For each game: year range(s) and hours/week per era → the preview shows "~9,000h" with the Estimated marker. Optional story note.
4. **Link Battle.net** (optional, suggested if WoW was picked) → character list → tick the ones to show.
5. **Download the tray client** → from now on hours are **Tracked** automatically.

**3.2 Tray install and first run**
1. Run the signed `Fraglink-setup.exe` (per-user, no admin; WebView2 installed only if missing).
2. Sign in through the system browser (deep link back).
3. Library scan ("Found 37 installed games"). Opt-in toggles for WoW character discovery, /played import and rig detection, each with a one-line "what we read" note.
4. Minimise to tray. Friends is the default tab.

**3.3 First detected session** (unchanged from v0.1): detector matches the exe → session opens locally → presence pushed → friends see it in under 5 s → on exit the session is saved and uploaded and presence goes back to Online. Idle trimming applies.

**3.4 Friend starts playing**: presence fan-out → Friends tab moves the row to *Playing* → toast "Dizz has started playing World of Warcraft" (unless muted/DND) → Activity tab gets an entry → click opens the side panel with a link to the friend's web profile.

**3.5 Quick glance → full stats**: Stats tab shows today / this week / top games → "Full stats on playfraglink.com" opens `/u/<handle>/stats`, already signed in.

**3.6 Link WoW characters**
1. Website → Linked accounts → "Link Battle.net" → Blizzard OAuth consent (`wow.profile`).
2. The server calls `/profile/user/wow` → all characters across the user's WoW accounts → pre-ticks the ones the tray reported from `WTF\Account` folders (characters actually played on this PC).
3. For each ticked character the server fetches public character data with the app token, plus Raider.IO data; the card shows "Verified via Battle.net".
4. Classic and Classic Era characters work the same way (different namespaces). Turtle WoW etc.: "Add character manually" → Unverified.

**3.7 Import WoW /played**
1. Website → WoW → "Import /played" → install the tiny **Fraglink Played** addon (or pick a supported existing addon).
2. The addon records each character's /played on login; WoW saves it to disk on logout.
3. The tray reads the SavedVariables file when WoW is closed and uploads `{character, realm, flavor, played_seconds, captured_at}` → the web history shows WoW totals with the **Imported** marker ("◆ /played, 6 characters, captured 26 Sep").
4. No addon? "Enter manually" with a `/played` screenshot → Imported (manual), shown as "self-reported".

**3.8 Export**: Settings → Data → "Export everything" → zip ready in under 1 min (target), or an email link if larger.

---

## 4. Friends from anywhere

- **Core (v1):** your Fraglink friends, on any launcher and any game the detector knows, visible in the tray and on the web friends page (phone-friendly). This is the promise.
- **Bonus (v1):** Steam friends' presence for users who link Steam. `ISteamUser/GetFriendList` needs the viewer's friends list to be public; `GetPlayerSummaries` returns `personastate` and `gameextrainfo` (current game) only for friends whose profile/game details are public. Shown as a separate "Steam friends" section labelled "via Steam", with "Invite to Fraglink".
- **Discord friends:** Discord doesn't expose friends lists or other users' presence to third-party OAuth apps. Users who linked Discord can find each other by matching linked accounts (opt-in, v1.1).
- **Not possible:** Xbox/PSN friends (no public API), Battle.net friends (no public friends API), Riot friends.

---

## 5. Historic hours: the three-tier trust system

Hours are Fraglink's trophy, so every number carries an honest label. Profiles, the signature image, the tray Stats tab and exports all show the tier.

| Tier | Marker | Sources | Where it counts |
|---|---|---|---|
| **Tracked** (fully trusted) | ● green, "Tracked" | Fraglink-logged sessions (tray); **Steam official playtime** (`playtime_forever`) via the Steam Web API | Totals, charts, achievements, ranks/leaderboards, game-page top players |
| **Imported** | ◆ blue, "Imported" | The game's own counters: **WoW /played** captured by the Fraglink addon (or a supported existing addon) and read from SavedVariables on disk by the tray; **manual /played entry with screenshot**; **Xbox yearly recap** hours with screenshot; later other official counters | Totals and achievements (addon-captured); ranks use Tracked only in v1 (open decision §12.3). Manual entries show "self-reported". |
| **Estimated** | "~" prefix + dotted underline, "Estimated" | The **history calculator**: game, year range(s), hours/week per era | Shown separately as "~9,000h". **Excluded from ranks, leaderboards and achievements.** |

### 5.1 Display rules
- A game's row shows the best tier's total, with a stacked tooltip, e.g. *World of Warcraft: ◆ 11,214h imported (/played, 6 characters) + ● 312h tracked since Mar 2026 + ~2,000h estimated (deleted characters, 2005–2008)*.
- Overall Hours shows **verified** (Tracked + Imported) and **estimated** separately: "14,210 h verified · ~3,400 h estimated".
- Estimated numbers are rounded (to the nearest 100 h above 1,000 h) and always shown with "~".
- **Optional story note per game** (≤140 chars), e.g. "Raided every night through TBC and Wrath". Shown under the row; filtered by the same blocklist as handles.

### 5.2 Merge and overlap rules (no double counting)
- **Steam games:** Total = Steam `playtime_forever` (Tracked). Local sessions still feed 7-day figures and streaks but never add on top.
- **Games with an Imported counter** (e.g. WoW /played): the counter is cumulative and already includes time Fraglink tracked. Displayed total = latest counter value, split in the UI into "Tracked since <date>" (local sessions since the first capture) and "Imported (before Fraglink)" = counter − tracked portion.
- **Estimated** only fills gaps the other tiers can't cover: years before Steam/Fraglink, **deleted characters** (their time is unrecoverable: the game no longer reports it, and Blizzard's API never exposed played time), dead platforms, other accounts. The calculator warns if an era overlaps a period that already has Tracked/Imported data for that game.
- **Non-Steam games without a counter:** Total = local sessions ("tracked since <date>"), plus any Estimated shown separately.

### 5.3 WoW /played capture (Imported)
- **Blizzard's Profile API does not expose played time.** The character profile covers level, class, spec, equipment, achievements and so on, but there is no /played field. The only official source is the game client.
- **Fraglink Played addon** (tiny, free, open source, Lua): on login it calls `RequestTimePlayed()`, handles the `TIME_PLAYED_MSG` event (total and this-level seconds) and stores `{name, realm, total, level, class, captured_at}` in its SavedVariables. WoW writes SavedVariables to `WTF\Account\<ACCOUNT>\SavedVariables\FraglinkPlayed.lua` on logout/reload. Each character has to be logged in once to be captured (true of every played-time addon).
- **The tray reads the file from disk** (only when WoW isn't running, or on file change after exit) with a strict Lua-table parser. It never touches game memory or the game process. Existing addons that already store /played in SavedVariables (e.g. Altoholic/DataStore, AccountPlayed) can be read too, opt-in, once their formats are checked (open decision).
- Follow Blizzard's UI Add-On Development Policy (addons must be free, code not obfuscated, no in-game ads). Publish on CurseForge/Wago and GitHub.

### 5.4 History calculator (Estimated)
- Inputs per game: one or more **eras** (`from_year`–`to_year`, `hours_per_week`), optional breaks and a story note.
- Output: `sum(weeks_in_era × hours_per_week)` with caps (≤80 h/week; warning above 40 h/week for multi-year eras). Shown as "~9,000h".
- Sanity checks: an era can't start before the game's release year (from metadata); estimated hours across all games in one year above ~4,000 h trigger a warning (soft cap).
- Stored as inputs (not just the result) so users can edit it and the export contains the reasoning.

---

## 6. WoW characters and other game accounts

**Goal:** beat xf1re, whose legacy Armory-based Character Sheet is dead (its endpoints return 404 and none of ~9,000 public profiles shows a character). Fraglink does it with today's official APIs.

### 6.1 Design
1. **Tray discovery (no API call):** character and realm names from `WTF\Account\<ACCOUNT>\<Realm>\<Character>\` folder names for `_retail_`, `_classic_` and `_classic_era_`; region from `WTF\Config.wtf` (`SET portal`). Uploaded as *suggested* characters only; folder mtime gives "last played on this PC".
2. **Ownership via Battle.net OAuth:** Authorization Code flow with scopes `openid wow.profile`. `GET https://{region}.api.blizzard.com/profile/user/wow?namespace=profile-{region}` returns the user's WoW accounts and all their characters (name, realm, level, class, race, faction). This **verifies ownership** and gives the BattleTag for the linked-accounts row. Only characters returned here get the **Verified via Battle.net** badge. Client-credentials tokens can't call this endpoint; it needs the user's token.
3. **Card data (public Profile API, app token via client credentials, server-side):** `/profile/wow/character/{realmSlug}/{name}` (level, race, class, active spec, guild, equipped + average item level, achievement points, title, faction, last login), `/character-media` (avatar/inset/main render URLs), `/equipment` (per-slot items; paper-doll view in v1.1), `/mythic-keystone-profile`, `/encounters/raids`.
4. **Raider.IO** (`raider.io/api/v1/characters/profile?region&realm&name&fields=mythic_plus_scores_by_season:current,raid_progression`) for M+ score and raid progress. Public; a free API key (Raider.IO app settings) gives higher rate limits. Attribute "Data from Raider.IO" and check its terms before launch.
5. **Classic:** Profile API namespaces `profile-classic-{region}` (Classic progression realms) and `profile-classic1x-{region}` (Classic Era, incl. Hardcore / Season of Discovery). The *protected* character endpoint has had reported 403s for Classic, so use the public character endpoints.
6. **Private servers** (e.g. Turtle WoW): no API. Manual entry (name, realm, level, class, optional guild) marked **Unverified**, with a placeholder portrait.
7. **Caching and freshness:** Blizzard publishes profile data **only after the player logs out** (account-level fields need a full logout of every WoW account on the Battle.net account), so it lags live play. Cache each character server-side for 6–12 h (estimate), refresh on the tray's "stopped playing WoW" event (+10 min delay) and on manual refresh (≤1 per 15 min). Long-inactive or transferred characters return 404: keep the last snapshot with "last updated <date>".
8. **Rate limits:** 36,000 requests/hour (Blizzard Developer API Terms) and 100 requests/second per client (Blizzard developer forums). One character refresh ≈ 3–5 calls (profile, media, M+, raids), so the hourly cap allows roughly 7,000–12,000 character refreshes/hour (estimate). Blizzard allows one client per application, so no key-sharding; use a server-side token bucket and exponential backoff on 429.
9. **Terms (Blizzard Developer API Terms of Use):** no selling/licensing the data; no marketing/advertising use of the data; **"Premium" versions offering for-pay features are not permitted when those features use the Blizzard APIs, and no donation interstitials in front of features.** So WoW features stay 100% free, the Supporter tier never includes or gates anything built on Blizzard data (e.g. supporter-only animated signatures must not render WoW character data), and there are no "support us" gates in front of WoW pages. Honour unlinking (delete Blizzard data on unlink or account deletion).
10. **Art:** use Blizzard renders only as returned by the API for verified characters, with attribution; Fraglink's own UI (and the prototype) uses original placeholder portraits. No Blizzard logos or game art in Fraglink branding.

### 6.2 Other games (Later)

| Game | Route | Plan |
|---|---|---|
| RuneScape / OSRS | Official hiscores `index_lite.json` (no auth) | v2, easy; verify with a code in the in-game name, or mark Unverified |
| Guild Wars 2 | Official API with a user-created key | v2; account playtime counters could feed Imported |
| Destiny 2 | Bungie.net OAuth | v2 |
| Riot (LoL/VALORANT/TFT) | RSO OAuth needs an approved production key | v2; typed Riot ID (Unverified) until then |
| Xbox / PSN | No public API | Skipped (typed gamertag/PSN ID, Unverified) |
| Battle.net non-WoW | SC2/D3 profile APIs; no Overwatch/D4 profile API | Skip |

---

## 7. Tech stack

### 7.1 Desktop framework: **Tauri 2 (decided)**

| Option | Pros | Cons |
|---|---|---|
| **Tauri 2** (Rust core + system WebView2) | Reuses the HTML/CSS/JS prototype almost verbatim. Small install because Chromium isn't bundled (WebView2 bootstrapper adds ~0–1.8 MB; [Tauri Windows installer docs](https://v2.tauri.app/distribute/windows-installer/)). First-party tray, notification, autostart, updater, deep-link, single-instance and window-state plugins. Rust suits Win32 process APIs and low idle CPU. | Rust learning curve. WebView2 spawns processes while a window is open, so destroy windows when hidden. Transparent frameless windows need testing across Windows 10/11 and DPI settings. |
| Electron | Most mature. | Bundles Chromium + Node: far heavier for a mostly idle tray app, which contradicts "tiny". |
| Native (C#/WPF/WinUI 3, C++) | Smallest runtime overhead. | The prototype would have to be rebuilt by hand; skins become expensive. |

The prototype's `index.html/styles.css/app.js` becomes the frontend (split into modules, mocks swapped for `invoke()` calls and events). Windows: `main` (chassis), `toast` (created on demand), `options`. The Wine Tahoma-metric clone (LGPL) stays as the fallback font (ship its licence; never bundle Microsoft's Tahoma).

**Rust core modules:** `detector` (process polling + matching), `store` (SQLite, WAL), `sync` (outbox → server), `realtime` (WebSocket client), `idle` (`GetLastInputInfo`), `secrets` (Windows Credential Manager via `keyring`), `discord_rp` (IPC), **`wow_local`** (WTF folder names + SavedVariables parser, read-only), **`rig`** (WMI/DXGI, opt-in).

### 7.2 Game detection on Windows (unchanged from v0.1)
User-mode process polling every 5 s (15 s on battery) with `CreateToolhelp32Snapshot`/`sysinfo`; resolve paths only for new PIDs via `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `QueryFullProcessImageNameW`. Never broader rights, never memory reads, never injection. ETW/WMI rejected for v1 (admin rights / CPU cost). Steam `HKCU\Software\Valve\Steam\RunningAppID` is a high-confidence signal (community knowledge; verify). Library scan: Steam `libraryfolders.vdf` + `appmanifest_*.acf`; Epic/GOG manifests as a stretch. The signature DB (`{exe_name, path_hint, game_id, platform, confidence, source}`) is served as a versioned, gzipped JSON snapshot. **Don't seed from Discord's undocumented detectable-games endpoint**: Discord has "no current plans to document" it ([discord-api-docs #1862](https://github.com/discord/discord-api-docs/issues/1862)) and its Developer Policy forbids scraping. Seed from beta library scans (Steam appid ↔ metadata), a hand-curated top-N non-Steam list, and user suggestions (auto-promoted at ≥3 distinct accounts older than 7 days, plus admin review).

### 7.3 Backend: **Supabase, Sydney region (ap-southeast-2)**
Postgres + RLS, Auth (email, Discord, Twitch providers), Realtime, Edge Functions, managed backups. Start on **Pro** for the beta (Free's 200-connection cap is too tight). Pro includes 500 concurrent Realtime connections, then $10 per 1,000 peak ([limits](https://supabase.com/docs/guides/realtime/limits), [pricing](https://supabase.com/docs/guides/realtime/pricing)). Domain logic lives in SQL functions + thin Edge Functions, so a later move to a dedicated Go/Node WebSocket service stays possible.

**Presence (unchanged):** one private Realtime channel `user:<id>` per client; `set_presence()` RPC + 60 s heartbeat; a DB function broadcasts to accepted, non-blocked friends; a sweeper marks users offline after 150 s. Don't use Supabase *Presence* for global friend presence (per-channel keys, client call limits). The **web friends page** subscribes to the same channel from the browser (read-only).

**New server jobs (Edge Functions + `pg_cron`/queues):** `steam_daily_sync`, `steam_friends_presence` (only while viewers are online), `bnet_character_refresh` (queue + token bucket under 36k/h and 100/s), `raiderio_refresh`, `recap_weekly`, `achievements_eval` (batched on session insert), `activity_aggregate`, `export_build`, `sig_render` (on demand + cache).

### 7.4 Auth
- **Login:** email (magic link + password) and Discord OAuth (`identify`).
- **Links:** Steam OpenID 2.0; Twitch OAuth (identity; `channel:manage:broadcast` only for v1.1 category suggest); **Battle.net OAuth** (`openid wow.profile`, Authorization Code flow; tokens encrypted with Supabase Vault/pgsodium; the user token is only needed for `/profile/user/wow`; character refreshes use the app token).
- **Desktop OAuth:** system browser only (never an embedded webview for third-party login), PKCE, `fraglink://` deep link, refresh token in Windows Credential Manager.

### 7.5 Discord Rich Presence
Unchanged from v0.1: legacy IPC crate (recommended) or the Social SDK via FFI; labelled as Fraglink; default off; no `SetName` that presents the activity as the game itself ([Setting Rich Presence](https://docs.discord.com/developers/discord-social-sdk/development-guides/setting-rich-presence), [Developer Policy](https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy)).

### 7.6 Twitch
v1: OAuth link, EventSub live status, OBS widget, and **live streams on game pages** for the game's Twitch category (Helix `Get Streams` with an app token). Terms: Twitch Data may not be sold, licensed, monetised or shared without permission ([Developer Services Agreement](https://legal.twitch.com/legal/developer-agreement)), so Twitch-derived features (live badges, live streams on game pages) stay free and are never part of the Supporter tier. Panel Extension stays v2 (review process).

### 7.7 Metadata: IGDB + SteamGridDB (+ fallbacks)
IGDB is used server-side only (4 req/s, 8 concurrent). It is **"free for non-commercial usage"** under the Twitch Developer Service Agreement, with commercial use via partner@igdb.com ([IGDB API docs](https://api-docs.igdb.com/)). **A Supporter tier probably makes Fraglink "commercial"**, so contact IGDB before launching Supporter, or switch to fallbacks: Steam store data for Steam games, Wikidata (CC0) for release years/genres, and Fraglink's own curated rows. SteamGridDB: terms page not located; treat as optional. Fallback art: the prototype's original lettered tiles.

### 7.8 Website hosting
**Astro (SSR) on Cloudflare Pages/Workers.** Reads public views from Supabase with the anon key under RLS; edge-caches 30–60 s. The prototype's website CSS (`.wp-*`, `.pf-*`, Cyan Grid / Gold 567) becomes a shared `@fraglink/skins` package. **Signature images** are rendered on a Worker with Satori/resvg (SVG → PNG), CDN-cached ≤10 min and purged on significant change. Domain: `playfraglink.com` primary (not yet registered, §12.3).

### 7.9 Supporting tooling
GitHub monorepo (`apps/desktop`, `apps/web`, `packages/skins`, `addons/wow-fraglink-played`, `supabase/`), GitHub Actions for Windows builds, code signing (OV/EV or Azure Trusted Signing; AU sole-trader eligibility to confirm), Sentry (opt-in), privacy-friendly analytics (Plausible-style) on the website only.

---

## 8. Data model and API

### 8.1 Tables (Postgres)

| Table | Key columns | Notes |
|---|---|---|
| `users` | `id uuid PK` (= auth.users), `handle citext unique`, `display_name`, `avatar_url`, `created_at`, `age_gate_ok`, `deleted_at` | Profile row. |
| `linked_accounts` | `user_id`, `provider enum(steam,discord,twitch,bnet)`, `provider_uid`, `display` (e.g. BattleTag), `verified bool`, `show_on_profile`, `linked_at`, `last_sync_at`, `sync_status` | Unique `(provider, provider_uid)`. Twitch/Battle.net tokens encrypted in Vault. |
| `unverified_handles` | `user_id`, `kind enum(riot,xbox,psn,other)`, `value`, `show_on_profile` | Always labelled Unverified. |
| `friendships`, `blocks` | as v0.1 (`user_a < user_b`, `status`, `requested_by`; one-directional blocks) | |
| `games` | `id`, `name`, `slug`, `igdb_id`, `steam_appid`, `twitch_category_id`, `release_year`, `cover_url`, `icon_url`, `genres[]` | |
| `exe_signatures`, `signature_suggestions` | as v0.1 | Suggestions store only exe name + install-relative path. |
| `sessions` | `id uuid` (client-generated, idempotent), `user_id`, `game_id`, `started_at`, `ended_at`, `duration_s`, `idle_trimmed_s`, `client_version` | Tracked source. |
| `steam_playtime` | `user_id`, `steam_appid`, `game_id`, `minutes_forever`, `minutes_2weeks`, `fetched_at` | Tracked source; daily snapshots kept for charts and recaps. |
| **`hours_entries`** | `id`, `user_id`, `game_id`, `tier enum(tracked,imported,estimated)`, `source enum(wow_addon,wow_manual,xbox_recap,estimator,…)`, `minutes`, `character_id null`, `evidence_path null` (private screenshot), `captured_at`, `inputs jsonb` (estimator eras), `note` | One row per imported counter snapshot or estimate. Tracked totals are derived from `sessions` + `steam_playtime`. |
| **`game_notes`** | `user_id`, `game_id`, `story` (≤140 chars), `hidden` | Story note per game. |
| `user_game_stats` (materialised) | `user_id`, `game_id`, `tracked_min`, `imported_min`, `estimated_min`, `display_min`, `display_tier`, `minutes_7d`, `last_played_at` | Feeds the tray Stats tab, profile and signature. |
| **`wow_characters`** | `id`, `user_id`, `flavor enum(retail,classic,classic_era,private)`, `region`, `realm_slug`, `name`, `verified bool`, `bnet_character_id null`, `source enum(bnet,tray_suggested,manual)`, `show`, `sort`, `snapshot jsonb` (level, race, class, spec, guild, ilvl, achievement points, media URLs), `rio jsonb` (M+ score, raid progress), `fetched_at` | Unique `(flavor, region, realm_slug, name)` among verified rows. |
| **`activity_events`** | `id`, `user_id`, `type enum(started,first_time,milestone,achievement,live,recap,friend)`, `game_id`, `payload jsonb`, `created_at` | Aggregated per day for feeds; filtered by visibility, hidden games and blocks. |
| **`achievements`** | `user_id`, `track`, `tier`, `progress`, `unlocked_at` | Plus a static `achievement_tracks` table. |
| **`recaps`** | `user_id`, `period enum(week,year)`, `period_start`, `data jsonb`, `created_at` | |
| **`rigs`** | `user_id`, `cpu`, `gpu`, `ram_gb`, `os`, `monitor`, `extras jsonb`, `auto_detected_at`, `edited_at`, `show` | |
| `presence` | as v0.1 (`status`, `game_id`, `session_started_at`, `live`, `stream_url`, `heartbeat_at`) | Invisible users appear offline to everyone else. |
| `profile_settings` | `visibility`, **`layout jsonb`** (`{section_id, column, order, hidden}[]`), `web_theme enum(cyan,gold,phoenix,stream,classic)`, `client_skin`, `widget_token`, `sig_style`, `hidden_game_ids[]`, `invisible` | |
| `notification_prefs` | `user_id`, `friend_id null`, `mute_toasts`, `dnd_in_game`, `recap_email` | |
| **`supporter`** (v1.1) | `user_id`, `status`, `provider_ref`, `since`, `custom_slug` | Cosmetics only. |
| **`exports`** | `user_id`, `requested_at`, `ready_at`, `path`, `expires_at` | |

**Local SQLite (client):** `sessions` (outbox), `signatures_cache`, `exclusions`, `manual_tags`, `settings`, `friends_cache`, `activity_cache`, `wow_local` (suggested characters, last parsed addon-file hash).

### 8.2 API and realtime surface (v0.1 surface plus the rows marked new)

| Kind | Endpoint / channel | Purpose |
|---|---|---|
| RPC | `set_presence`, `heartbeat`, `upload_sessions`, `friend_request/respond/unfriend/block/redeem_invite` | As v0.1 (rate-limited, validated). |
| RPC (new) | `suggest_wow_characters(chars[])`, `upload_wow_played(entries[])`, `upload_rig(rig)` | Tray sensors (T8). Played values must be monotonic per character and within plausible bounds. |
| RPC (new) | `save_estimate(game_id, eras[], note)`, `save_manual_import(game_id, minutes, evidence)`, `save_layout(layout[])` | Web calculator, manual import, profile layout. |
| REST (RLS) | `stats_glance_view` (today / week / top 5, for the tray), `activity_feed_view`, `user_game_stats`, `profile_settings`, `friends_view` | Reads. |
| Edge Fn (new) | `GET /auth/bnet/start`, `GET /auth/bnet/callback`, `POST /bnet/refresh` (≤1 per 15 min per user) | Battle.net link + character refresh. |
| Edge Fn | `GET /auth/steam/start|callback`, `POST /steam/sync`, cron `steam_daily_sync`, cron `steam_friends_presence` (new) | Steam. |
| Edge Fn | `POST /twitch/eventsub`, `POST /signatures/suggest`, `GET /signatures/snapshot?since=v` | As v0.1. |
| Edge Fn (new) | `POST /export` → signed URL | Export. |
| Realtime | private channel `user:<id>`: `presence`, `friend_request`, `friend_accepted`, `activity` (new) | One subscription per client (tray or browser). |
| Web | `/u/<handle>`, `/u/<handle>/stats`, `/u/<handle>/wow`, `/g/<slug>`, `/sig/<handle>.png`, `/recap/<handle>/<period>`, `/estimate`, `/add/<code>`, `/w/<token>` | Public pages, signature, recap, calculator, invite, OBS widget. |

### 8.3 Steam API budget (estimate)
The Steam Web API Terms cap usage at **100,000 calls/day** and require the key to stay confidential ([terms](https://steamcommunity.com/dev/apiterms)). Daily sync ≈ 3 calls per user. Friends presence: `GetPlayerSummaries` takes up to 100 SteamIDs per call; polling deduplicated IDs every 3 min, only while viewers are online, ~40,000 calls/day covers roughly 8,000 distinct friend IDs continuously (estimate). Log usage, alert at 50%, and degrade automatically to 5–10 min polling.

---

## 9. Privacy, data promise and anti-abuse

**What leaves the machine:** matched game ID + session start/end, status, client version; opt-in: WoW `{flavor, region, realm, character}` names, /played numbers parsed from the addon file, rig specs. **Never sent:** the process list, unmatched exe names, window titles, full file paths, WoW account folder names, file contents, hardware IDs. The one exception stays the explicit "suggest this tag" (exe name + install-relative path).

**Visibility controls:** see W13. RLS enforces every read path; the web profile, feeds, signature image and game-page lists all read through views that apply visibility, per-game hides and blocks. "Invisible" records hours but publishes no presence.

**Data promise (headline feature)**
1. **Export everything, any time, free** (JSON + CSV, documented schema; W14).
2. **Public shutdown pledge** (About page + ToS): "If Fraglink ever shuts down, we'll give at least **90 days' notice** (proposed; open decision), keep export working the whole time, and publish a final archive option. Your hours are yours." For contrast: Xfire gave 2 days' notice in June 2015, its export tool was buggy, and ArchiveTeam saved about 20% of screenshots and no profiles.
3. Account deletion removes all server data within 30 days (policy commitment), including data derived from Blizzard, Steam and Twitch.

**Integration ToS checklist**

| Integration | Key obligations | Our stance |
|---|---|---|
| Steam Web API / OpenID | Key confidential, 100k calls/day, privacy policy covering Steam data, user access/deletion, data location ([terms](https://steamcommunity.com/dev/apiterms)) | Server-side key; privacy policy names the Sydney region; deletion flow; friends presence only for public profiles. |
| Blizzard / Battle.net APIs | 36,000 calls/hour; no selling/licensing data; no marketing/ads use; **no premium for-pay features that use the APIs; no donation interstitials**; honour deletion ([Blizzard Developer API Terms of Use](https://www.blizzard.com/en-us/legal/a2989b50-5f16-43b1-abec-2ae17cc09dd6/blizzard-developer-api-terms-of-use)) | WoW features free forever; Supporter perks never touch Blizzard data; caching + token bucket; unlink deletes data. |
| Raider.IO | Public API, optional key; attribution | Attribute; check terms before launch. |
| Discord (OAuth, RPC / Social SDK) | Use API data only for the stated function, no scraping, no impersonation ([Developer Policy](https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy)); Social SDK: 13+ and review rights ([SDK Terms](https://support-dev.discord.com/hc/en-us/articles/30225844245271-Discord-Social-SDK-Terms)) | `identify` scope only; RP labelled as Fraglink. |
| Twitch | No selling/monetising/sharing Twitch Data; minimal scopes; honour revocation ([DSA](https://legal.twitch.com/legal/developer-agreement)) | Twitch features free; never in the Supporter tier. |
| IGDB | Non-commercial free; commercial needs a partnership ([docs](https://api-docs.igdb.com/)) | Contact before Supporter launch, or use fallbacks. |
| WoW add-on policy | Addons free, code not obfuscated, no in-game ads | Fraglink Played is free and open source. |
| Anti-cheat vendors | Injection/memory reads trigger bans | Read-only process enumeration; documented file reads only; public "what Fraglink reads" page. |

**Anti-abuse / hours integrity**
- Server validation: no overlapping sessions, ≤24 h per session, no future timestamps; idle trimming client-side.
- Tiers make faking pointless where it matters: ranks and leaderboards use Tracked only (v1); Estimated never counts; manual Imported entries are "self-reported" and reportable.
- Friend requests by handle or invite link; no public user directory in v1; rate limits on requests, handle changes, suggestions and estimates.
- Profiles: blocklists for handles and story notes; report-profile link; avatars only from linked Discord/Steam (no uploads in v1).
- **Age:** minimum 13 globally (Discord Social SDK terms, if used). **Australia's under-16 social media minimum age (in force since 10 Dec 2025)** excludes services whose primary purpose is messaging or online gaming, but public profile and posting features can bring a service into scope ([eSafety overview](https://www.esafety.gov.au/about-us/industry-regulation/social-media-age-restrictions), [assessment guide](https://www.esafety.gov.au/about-us/industry-regulation/social-media-age-restrictions/assessment)). v0.2 adds more social surface (activity feed, public profiles, story notes, achievements), which **raises** the chance Fraglink is in scope. Get advice before public launch (open decision).

---

## 10. Monetisation

**Decided direction:** the core is free, always. An optional **Supporter tier** at a few dollars a month (e.g. A$4/month or A$40/year; price is an open decision) offers **cosmetics only**:
- Premium client skins and profile themes (beyond the free 3–5)
- **Animated signature images** (APNG/GIF), rendered from Fraglink-owned stats only, never Blizzard or Twitch data
- **Longer history charts** (multi-year chart ranges; the underlying data and exports stay free)
- **Custom profile URL** (`playfraglink.com/<slug>`)
- **Supporter badge**

**Rules:** never paywall stats, tracking, friends, history, exports, WoW cards or any linked-account feature. No ads-first model. No gambling, coins, loot boxes or sweepstakes (Australian gambling rules). No bundleware (Xfire removed OpenCandy in 2011 after it hurt trust).

**Terms to settle before launching Supporter (target v1.1):**
- **IGDB** is free for non-commercial use only → contact partner@igdb.com, or move to fallback metadata (§7.7).
- **Twitch** data can't be monetised → Twitch features stay free; no Twitch data inside paid cosmetics.
- **Blizzard** forbids premium for-pay features that use its APIs, and donation interstitials → Supporter perks must be independent of Blizzard data, and "Support Fraglink" prompts must never sit in front of WoW features.
- Payments through Stripe, or a merchant of record (Paddle / Lemon Squeezy) to handle overseas GST/VAT. Australian GST registration is only required above A$75k turnover; confirm with an accountant.

---

## 11. Build plan (solo dev)

Effort is in **estimated full-time-equivalent weeks** (multiply by your real hours if part-time). **Total: about 22–30 weeks FTE (estimate)**, up from 15–20 in v0.1. The increase comes from the bigger v1 website (layout/themes, signature image, achievements, game pages, recaps, export), WoW characters and the trust-tier system; the tray shrinks slightly (no Profile tab, a simpler Stats view).

| Milestone | Scope | Est. effort | "Done" means |
|---|---|---|---|
| **M0 Setup** | Monorepo, Tauri 2 scaffold running the prototype, tray, single-instance, autostart, CI, code-signing path chosen, Discord/Twitch/Steam/Battle.net/IGDB/Raider.IO dev apps registered | 1 wk (est.) | Signed dev build installs on a clean Windows 11 VM; mock data renders in all three skins. |
| **M1 Local tracker + tray tabs** | Detector, idle trimming, SQLite store, seed signatures (~100 games, estimate), tagging/exclusions, **Stats** and **Activity** tabs on local data, local toast window | 3–4 wks (est.) | 7 days on Lucrona's PC with no missed/phantom sessions vs a manual log; idle CPU target met; works offline. |
| **M2 Accounts + Steam + trust tiers** | Supabase (Sydney), schema + RLS, email/Discord login via deep link, Steam link + import + daily sync, session outbox, **`hours_entries` + tier merge rules**, export v1 (CSV/JSON), privacy policy v1 | 3–4 wks (est.) | New user: sign up → link Steam → correct totals (20 games spot-checked) marked Tracked; offline sync without duplicates; export zip complete; account deletion works. |
| **M3 Friends + presence + feed** | Friend requests/invites/blocks, Realtime presence, Friends tab sections, side panel, toasts, Invisible/Away, **activity events + aggregation**, live Activity tab, **Steam friends presence** (bonus) | 3–4 wks (est.) | 5 accounts on 3 machines: presence under 5 s p95; a killed client shows offline within 3 min; blocked/invisible users leak nothing (direct API checks); Steam friends appear for public profiles within the call budget. |
| **M4 Website core** | Astro site: `/u/<handle>` with the layout model (drag/hide/reorder), 3–5 themes, gaming history with tier markers + story notes, **onboarding wizard with estimator**, linked accounts + Verified badges, privacy controls, web friends page, **signature image** renderer, OG cards, export UI | 4–5 wks (est.) | Profiles render correctly for public/friends/private; a new signup with Steam + estimator ends with a non-empty profile; the signature PNG renders in every theme and updates within 10 min. |
| **M5 WoW + Imported hours** | Battle.net OAuth, character list, public Profile API + media + Raider.IO, Classic namespaces, manual Unverified entries, caching/token bucket, tray `wow_local` (folder discovery + SavedVariables parser), **Fraglink Played addon**, manual /played import with screenshot | 3–4 wks (est.) | 10 real characters (retail + Classic Era) show correct level/class/guild/ilvl/M+ within one logout cycle; addon /played imports match in-game `/played` exactly; load test stays under 36k/h. |
| **M6 Website depth** | Game pages (7-day chart, rank, friends who play, Twitch live streams, JSON-LD), achievements (7 tracks × 5 tiers), weekly recap (+ tray notification), rig card (tray auto-detect + edit), Twitch link + LIVE badge + OBS widget, Discord RP (opt-in) | 3–4 wks (est.) | Achievements recompute correctly on a seeded dataset; recaps generated for all beta users on Monday; cached game page loads in under 1 s. *The rig card may slip to v1.1 without blocking beta.* |
| **M7 Beta hardening** | Updater channel, crash reporting, landing/download page, ToS + privacy policy + **shutdown pledge**, age gate, load tests, beta invites | 2 wks (est.) | 20–50 closed-beta testers (target) onboarded; auto-update tested from N-1; no P0 bugs open for 7 days. |
| *Post-beta v1.1* | Supporter tier (Stripe or MoR, cosmetics, custom URL, animated signatures), yearly recap (if not in M6), IGDB commercial route | *2–3 wks (est.)* | |

Sequencing note: M1 is useful on its own (a local tracker). M2 + M4 together already deliver the "track your stats on a real profile" promise, even if WoW (M5) or the depth features (M6) slip.

---

## 12. Beta success metrics, risks, decisions

### 12.1 Beta success metrics (targets to validate, not benchmarks)

| Metric | Proposed beta target |
|---|---|
| Signed up → non-empty profile (any tier) within 24 h | ≥ 85% (xf1re: 59% of profiles non-empty) |
| Installed tray → first detected session within 24 h | ≥ 60% |
| Detection coverage (survey + "untracked programs" data) | ≥ 85% |
| Median accepted friends per active user after 2 weeks | ≥ 3 |
| Tray still running on day 7 / day 30 | ≥ 50% / ≥ 30% |
| Active users who visit their own profile/stats weekly | ≥ 50% |
| Public profiles whose owner shares the link or signature | ≥ 25% |
| Users with WoW detected who link Battle.net | ≥ 40% |
| Presence reliability: p95 propagation < 5 s; phantom "still playing" | < 1 per user-month |
| "Very disappointed if Fraglink went away" | ≥ 40% |

### 12.2 Key risks and mitigations

| Risk | Mitigation |
|---|---|
| **Network effect** | Invite links; value without friends (your own stats, history, WoW card, signature); Steam friends presence as a bridge; seed whole friend groups. |
| **Website scope creep** (the v1 website is now large) | Strict M4 → M6 order; rig card and yearly recap may slip; anything new goes to v1.1. |
| **Detection gaps / false positives** | Library-directory attribution, manual tagging, untracked list, denylist of launchers/anti-cheat helpers. |
| **Hours trust disputes** (inflated history) | Tier markers everywhere; ranks use Tracked only; Estimated never counts; self-reported imports are reportable. |
| **Blizzard API changes / lag** | Cached snapshots with "last updated", logout-refresh explanation, Raider.IO as a second source, isolated module. |
| **Addon reading perceived as risky** | Read only after WoW exits, only Fraglink's (or opted-in) SavedVariables, documented publicly; no game-process access. |
| **Steam API budget** | Batching, adaptive polling, viewer-online-only polling, alerts at 50%. |
| **Platform/policy changes** (Discord IPC, Twitch, Steam privacy defaults, Blizzard terms) | Isolated integration modules; the core (local tracking + own presence + export) doesn't depend on third parties. |
| **Realtime cost at scale** | Budget alarm; re-evaluate a Go/Node WebSocket service when Realtime spend passes ~US$100/mo or 5k concurrent connections (estimate). |
| **Monetisation vs data terms** (IGDB, Twitch, Blizzard) | Cosmetic-only Supporter; perks independent of third-party data; IGDB conversation before launch. |
| **Trademark/IP** (Xfire-inspired look; WoW art) | Original mark and icons; no Xfire name/logo; placeholder portraits in product UI; Blizzard renders only as API-returned for verified characters, with attribution; trademark clearance for "Fraglink" and the skin names ("StreamEngine", "Phoenix"). |
| **SmartScreen / AV flags** | Code signing from the first beta build; public "what Fraglink reads" doc; no hooks. |
| **Legal/age (AU under-16 law, privacy)** | Advice before public launch; conservative age gate; APP-aligned privacy policy; data in Sydney. |
| **Shutdown trust** | Export from day one; public pledge (90 days proposed). |

### 12.3 Decisions

**Resolved (26 Sep 2026)**
1. ✅ **Name: Fraglink** (decided 25 Sep). On 25 Sep, `fraglink.gg`/`.app`/`.io`/`.com.au` looked unregistered and `fraglink.com` was listed for sale. **Domain:** `playfraglink.com` (bought 3 Oct 2026, replaces the planned `fraglink.gg`). **Still to do:** IP Australia (ATMOSS) + TMview searches; AU trademark filing in classes 9, 41 and 42. Brand assets are in `/workspace/fraglink-brand/`.
2. ✅ **Product direction:** stats + friends from anywhere; tiny tray, the website carries the bulk.
3. ✅ **Client tabs: Friends | Stats | Activity.** Profile moves to the website; Servers deferred (A2S community-server games only).
4. ✅ **Monetisation direction:** free core + optional cosmetic Supporter tier (post-beta); never paywall stats; no ads-first; no gambling/coins. (Was open decision 7.)
5. ✅ **Chat:** no in-app chat; hand off to Discord. (Was open decision 1.)
6. ✅ **Friends from anywhere:** Fraglink friends (core) + Steam friends presence (bonus).
7. ✅ **WoW characters in v1** via Battle.net OAuth + Profile API + Raider.IO; private servers as manual Unverified entries.
8. ✅ **Historic hours trust tiers:** Tracked / Imported / Estimated.
9. ✅ **Data promise:** export any time + public shutdown pledge.
10. ✅ **Stack:** Tauri 2 + Supabase (Sydney, Pro for beta) + Astro on Cloudflare. (Was open decision 5.)

**Still open**
1. **Minimum age: 13+ or 16+**, and whether Australia's under-16 social media minimum age applies (more likely now that v1 has public profiles, feeds and story notes). Get legal advice or do the eSafety self-assessment before public launch. Simplest conservative option: 16+ at signup.
2. **Default profile visibility:** Friends-only (recommended for privacy) or Public (better for sharing and growth; signature images need either a public profile or a public "card-only" mode).
3. **Do Imported hours count toward ranks/leaderboards?** v1 proposal: Tracked only; addon-captured Imported counts toward totals and achievements.
4. **Shutdown pledge length:** 90 days proposed (Xfire gave 2).
5. **Supporter price and timing** (e.g. A$4/mo or A$40/yr; v1.1) and the IGDB commercial route (partnership vs fallback metadata).
6. **Discord RP presentation:** "Fraglink" activity (recommended) vs `SetName`; legacy IPC crate (recommended) vs Social SDK FFI.
7. **Twitch category suggest:** v1.1 (recommended) or v1.
8. **Epic/GOG manifest detection:** v1 stretch, or strictly Steam mapping + curated exes.
9. **Code signing route:** OV/EV certificate vs Azure Trusted Signing (AU sole-trader eligibility).
10. **Yearly recap in v1 or v1.1** (must ship before Dec 2026 either way).
11. **Read existing WoW addons' SavedVariables** (Altoholic/DataStore, AccountPlayed) at launch, or Fraglink Played only.
12. **Rig card in v1 or v1.1.**

---

### Appendix A: Prototype → app mapping (`/workspace/fraglink-prototype`)

| Prototype element | v1 treatment |
|---|---|
| Main chassis, menus, maroon banner, slanted glossy tabs, blue headers, skinned scrollbars | Reused as-is. Tabs are now **Friends / Stats / Activity** in all three skins. |
| Friends tab (Playing / Friends of Friends / Offline) | Playing / Online / Steam friends / Requests / Offline. FoF hidden until v1.1. |
| **Stats tab** (Now playing, Today, This week, Top games with bars + trust markers, "Full stats on playfraglink.com") | Wired to `stats_glance_view` + the live session. |
| **Activity tab** (day-grouped friend events with lettered game tiles) | Wired to `activity_feed_view`; entries deep-link to website pages. |
| Former Profile tab | **Removed** from the client; Fraglink → My Profile opens the website. |
| Chat window, REPLIES mock | Kept in code, not shipped; the chat button opens the friend's Discord profile. |
| Toast | Becomes its own always-on-top Tauri window. |
| Hardware buttons: + / chat / join / info; screenshots/videos | + and info active; chat → Discord; Join, screenshots and videos hidden. |
| Skins: Classic Silver, Phoenix, StreamEngine (localStorage) | Kept; persisted to `profile_settings.client_skin`. |
| **Website profile v2** (two columns; trust-tier markers; WoW character card with placeholder portrait and "Verified via Battle.net"; signature preview; linked accounts with Verified badges; achievements strip; export link; Cyan Grid / Gold 567) | Basis for the Astro `/u/<handle>` page and `@fraglink/skins`. |
| **"Estimate your history" modal** | Basis for the onboarding calculator and `/estimate`. |
| Mock taskbar/tray, `compare=1`, `zoom` URL params | Taskbar dropped; zoom becomes a Tools → Options setting; params for dev builds only. |
| `fonts/` Wine Tahoma-metric clone (LGPL); Russo One (SIL OFL 1.1); Fraglink mark, wordmark, favicon | Bundled with licence notices; brand assets for app, installer, tray icon and site. |

### Appendix B: Sources (checked 25–26 Sep 2026)
- Blizzard Developer API Terms of Use (36,000 calls/hour; no premium for-pay features using the APIs; no donation interstitials; no selling data; no marketing use): https://www.blizzard.com/en-us/legal/a2989b50-5f16-43b1-abec-2ae17cc09dd6/blizzard-developer-api-terms-of-use
- Blizzard forums, WoW API update (Account Profile `/profile/user/wow` needs an Authorization Code token with `wow.profile`; `profile-{region}` namespace; account-level fields update only after a full account logout): https://us.forums.blizzard.com/en/blizzard/t/world-of-warcraft-api-update-visions-of-nzoth/3461
- Blizzard forums, rate limits (36,000/hour at 100/second): https://us.forums.blizzard.com/en/blizzard/t/lets-talk-throttling/32429 ; one client per application: https://us.forums.blizzard.com/en/blizzard/t/api-access-clients-rate-limits/5602
- Blizzard forums, characters must log in and out before their profile documents update: https://us.forums.blizzard.com/en/blizzard/t/api-document-publishing-delays-wow-shadowlands-pre-patch/12489
- Classic namespaces (`classic1x` = Era, `classic` = progression): https://us.forums.blizzard.com/en/blizzard/t/wow-classic-era-realm-apis/16812 ; profile namespaces `profile-classic1x-{region}` / `profile-classic-{region}`: https://registry.npmjs.org/@blizzard-api/classic-wow ; Classic protected-character 403 reports: https://us.forums.blizzard.com/en/blizzard/t/classic-protected-character-profile-summary-returns-403/54512
- Battle.net developer docs, WoW Profile APIs: https://community.developer.battle.net/documentation/world-of-warcraft/profile-apis
- No played-time field in the Profile API (API coverage table; /played only in game or via addons that log each character in): https://project.pages.drupalcode.org/wow/reference/apis/ ; https://www.reddit.com/r/wow/comments/1quo3h0/account_played_track_and_display_your_characters/
- Raider.IO API (character profile endpoint, `fields`, optional key for higher limits): https://raider.io/api ; https://github.com/tmaffia/raiderio/blob/main/README.md
- Discord Social SDK, Setting Rich Presence: https://docs.discord.com/developers/discord-social-sdk/development-guides/setting-rich-presence
- Discord Social SDK Terms: https://support-dev.discord.com/hc/en-us/articles/30225844245271-Discord-Social-SDK-Terms
- Discord Developer Policy: https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy
- Discord Developer Terms: https://support-dev.discord.com/hc/en-us/articles/8562894815383-Discord-Developer-Terms-of-Service
- Detectable endpoint undocumented: https://github.com/discord/discord-api-docs/issues/1862
- Twitch Developer Services Agreement: https://legal.twitch.com/legal/developer-agreement ; data storage clarification: https://discuss.dev.twitch.com/t/storage-of-data-from-twitch-api/63884 ; Extensions life cycle: https://dev.twitch.tv/docs/extensions/life-cycle/
- Steam Web API Terms: https://steamcommunity.com/dev/apiterms ; Steam OpenID / key: https://steamcommunity.com/dev ; IPlayerService: https://partner.steamgames.com/doc/webapi/IPlayerService ; Steam Web API (GetFriendList, GetPlayerSummaries): https://developer.valvesoftware.com/wiki/Steam_Web_API ; A2S server queries: https://developer.valvesoftware.com/wiki/Server_queries
- IGDB API docs: https://api-docs.igdb.com/
- Tauri Windows installer: https://v2.tauri.app/distribute/windows-installer/
- Supabase Realtime limits / pricing: https://supabase.com/docs/guides/realtime/limits , https://supabase.com/docs/guides/realtime/pricing
- eSafety social media age restrictions: https://www.esafety.gov.au/about-us/industry-regulation/social-media-age-restrictions
- Research notes: `/workspace/xfire-refs-hd/xfire-feature-history.md` (Xfire feature history, 2-day shutdown notice, lessons) and `/workspace/xf1re-refs/deep/xf1re-deep-dive.md` (xf1re profile sections, 41% empty profiles, dead WoW Character Sheet, achievements catalogue, miniprofile PNG).
