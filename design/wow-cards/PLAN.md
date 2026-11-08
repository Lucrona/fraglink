# WoW Forever, character add, and card designs

Review only. This branch does not change the server, the site build, detection, or hours.
Nothing here turns uploads on. The November forum and blog notes are cited as claims, not as tests we ran.

Source tree: `wow-played` at `428292366f79213451c01a0c96745ae319836d19`.

## What the repo does today

Retail and Forever are different game ids.

- `wow` is Retail. `/played` upload is open (`UPLOADS_OPEN`).
- `wow_forever` is live Forever: `Wow.exe` / `Wow-64.exe` inside `_forever_`. Upload is closed. The website radio is disabled. The API returns `forever_not_open`.
- `wow_forever_beta` is the old beta entry. Migration 0050 moved that entry's existing rows off `wow_forever` only where production had retired that id. Those hours stay on `wow_forever_beta`. `WowB.exe` is not Forever. This plan does not touch detection data or that split.

The addon is `FraglinkPlayed` 1.0.1. It asks `RequestTimePlayed`, stores a signed total in `FraglinkPlayed.lua`, and keys each character as `region/realm/name`. The signature key ships in the addon. It is a deterrent. It is not proof against the armory, and there is no `/played` field on the public profile APIs we know about.

The website upload (`POST /api/v1/hours/wow_played/file`) parses that file and calls `ingest`. The sync client has `wow_played()`, and this tree has no caller of it. The core comment says the app no longer installs the addon. A Forever file has to be chosen as `wow_forever` by the caller. The parser itself has no flavour field.

`ingest` for an accepted entry updates `wow_characters` in place and appends `wow_played_snaps`. It then calls `record_total`, which can insert a new `hours_entries` row (`tier = imported`, `source = wow_addon`). Old hour rows are not deleted, but the newest imported row becomes the displayed counter, with tracked play after that timestamp added by the existing stats view. Opening Forever on the current function would change displayed hours. That is why the upload stays closed until a path exists that does not call `record_total`.

Only characters on the linked Battle.net account (`wow.profile`), saved under that BattleTag, and not listed more recently on someone else's account, count toward that imported total. The rest are stored and shown as not counted. Seven-day figures ignore `/played`.

Retail armory calls use `profile-{region}` and `/profile/wow/character/{realm-slug}/{name}`. Forever calls exist only when `BNET_FOREVER_NAMESPACE` is set, and they still use that realm-slug URL. A reported Forever path with no realm would not match that builder. Leaving the env unset is required until we test the real URL.

`realms.rs` refreshes retail, classic, and classic-era realm indexes. It has no Forever flavour. A missing Forever realm list matches the report that Forever publishes a ruleset index instead. That is context, not a test.

Profile cards are a 48px portrait plus name, level, spec, class, realm, and region. The popup (`wowchar.rs`) is retail-only, verified characters only, and gated by the `wow_characters` feature. Public profile data is level, spec, race, and render. It is not ownership and it is not `/played`.

`wow_prefs.hide_all` hides every flavour from the profile and still counts the hours. There is no per-flavour show or count switch.

## Claims we have not tested

These stay out of production config until Fraglink requests them itself.

- Developer thread, 6–7 Nov 2026: https://us.forums.blizzard.com/en/blizzard/t/wow-forever-api-what-works-so-far-plus-a-ts-client/60334
- Independent notes: https://mhuijskens.com/WoWForever/Api/
- They report `profile-forever-{region}`, `static-forever-{region}`, and `dynamic-forever-{region}` for US, EU, KR, and TW.
- They report character URLs as `/profile/wow/character/{first}-{surname}` and `/character-media`, with no realm.
- They report `/data/wow/ruleset/index` instead of a realm index.
- Account `/profile/user/wow` needs a user token with `wow.profile`. An app token can read a public profile. That is not ownership.
- A public character is reported to need level 10 and a logout before it appears. Media can lag.
- A public armory page is reported at `https://worldofwarcraft.blizzard.com/en-gb/forever/eu/armory`.
- Wiki `Patch_1.60.1/API_changes` says interface `16001`, `WOW_PROJECT_CAMELOT = 18`, and `_Forever.toc` as an alias of `_Camelot.toc`. `RequestTimePlayed` / `TIME_PLAYED_MSG` are listed, with seconds. The same 1.60.1 interface is not enough to tell live Forever from the beta. Our toc already lists `16001` next to retail interface numbers. That listing is not a live-client capture.

No authoritative HTTP `/played` was found. Do not invent one.

## Hours rule for every later phase

Never rewrite or delete `sessions`, `hours_entries`, `steam_playtime`, `wow_played_snaps`, or existing `wow_characters` rows to "enable Forever".

Raw uploads append. The same character, total, and timestamp stored twice is a no-op. Cumulative `/played` still cannot go down. Provenance (file time, signature version, BattleTag, raw realm string if the client sent one) stays on the raw row.

Tracked app time for `wow_forever` and an imported `/played` total stay separate until a later review defines the overlap. There is no automatic carry and no new imported total in the first phases. A user switch cannot force that write while the global switch is off.

`wow_forever_beta` is never a merge target and never a destination for new Forever uploads.

## Identity

Retail stays region + realm + character name. Once a Battle.net character id is known, that id is the stable key across a rename. The current unique key includes the name, so a rename can look like a new character. A later additive column stores the stable id and an alias of the old name. It does not delete the old row's snapshots.

Forever's public identity is region + first name + surname. Cards and the manual form do not show a realm. If `GetRealmName()` still returns a string, that string is stored as raw provenance and is not the key and not the label. A rename changes the public first-surname path. The stable id, once known, keeps one character. Until then, a changed surname is an alias proposal for the owner to confirm, not a second hour pile and not a silent merge.

## Toggles

Two layers, so one switch cannot mean both "hide my cards" and "stop the service".

Per user, stored, defaulted so today's Retail behaviour stays:

| Switch | Default | Off means |
|---|---|---|
| Show Retail cards | on | Profile hides Retail cards. Rows remain. Hours still follow the count switch. |
| Count Retail `/played` | on | New Retail totals are not written. Existing imported rows stay. |
| Show Forever cards | off | No Forever cards on the profile until the owner turns it on. |
| Count Forever `/played` | off | Forever `/played` is saved raw and is not an imported total. |

Turning any of these off does not delete characters, snaps, or hour rows. `hide_all` remains "hide from profile" and is not reused as a flavour switch.

Global service switches, default off, separate from the user switches:

- `accept_forever_uploads` — website and app may store Forever raw rows.
- `forever_public_lookup` — the manual form may call the public profile.
- `forever_account_sync` — the user-token account list may mark Forever characters verified.

A user "count" switch does nothing while the matching global switch is off.

## Manual add

Retail form: region, realm, name. Forever form: region, first name, surname. No realm field on Forever.

A hit shows badges only for what came back:

- Account — the character is on this user's `wow.profile` list.
- Public profile — an app-token profile returned a character. Not ownership. Not `/played`.
- Signed file — an addon snapshot was stored. Not an armory time.
- Not counted — the row is not in the imported total.
- Not found yet — the lookup missed. Level 10 and a logout are reported reasons. The form does not delete anything and does not invent level, class, or hours.

Lookups go in a new table. They do not insert `wow_characters`, because `played_s` is required there and counted queries sum it.

## Card choice

The roster and ledger sketches are not the design. They stay in this folder so the rejection is on record. Do not implement them.

The review mock is `option-c-infopanel.html`. It is a compact info panel: a small portrait, the name with class and level, one aligned metadata line, and the site's blue header. A single tick means the character is on the linked Battle.net account. Retail already does that check. Forever can use the same tick only after the user-token account list has been tested. A public Armory page is a link, not the tick, and addon `/played` is not an API verification. The exact arrangement of any older game-profile panel is not known, so this does not invent one. Fields are ones Fraglink already stores. Sample names are labelled. Forever has no realm line. Missing spec, class, hours, and the tick are omitted.

No profile code changes until this panel is accepted. The HTML is not copied by `apps/site-web/build.py`.

## The site runs character refresh itself

Routine refresh is a server job, the same kind as the detectable-list refresh and the Steam sync job record. It is not a staff queue and not an outside scheduler.

- One loop on the server refreshes public profiles and retries a failed Battle.net or profile request later.
- The existing detail cache (serve the last good summary, refresh in the background) stays. A miss does not delete the character or any hour row.
- Retail and Forever are separate runs. A Forever profile namespace stays off until Fraglink has requested it and the result is recorded. A report is not that test.
- Each run writes a job row: last run, last success, and a short detail. Admin health shows that. A job that has never run is not shown as healthy.
- The job does not merge characters, rewrite `hours_entries`, or delete snapshots. Ownership still comes only from the user's own `wow.profile` list.
- An Armory link is a link. It is not a copy of `/played` and it is not proof the account owns the character.

## Phases

0. This review. No behaviour change.
1. Additive tables and columns only: raw upload events, lookup stubs, per-flavour prefs, stable id and name aliases. No backfill. A test starts from a database that already has Retail hours and checks those `hours_entries` and `sessions` sums are unchanged after migrate.
2. Behind `accept_forever_uploads`, the website can store a Forever file as raw rows and snaps. `record_total` is not called for Forever. Retail ingest stays as it is. A file chosen as the wrong flavour is rejected. `wow_forever_beta` is rejected as a destination.
3. After a live Forever client capture, and only if `RequestTimePlayed` and the SavedVariables file actually work: addon packaging for that client. Interface `16001` alone is not that capture. Still no imported total.
4. After our own request of the public profile, not after the forum post: Forever lookup uses the tested path. Badges stay honest. Misses store nothing in `wow_characters`.
5. After our own user-token account list shows Forever characters: set verified from that list only. Public app-token data still cannot verify ownership.
6. Ship the chosen card. Hide empty fields. Forever still has no realm line.
7. Only with a separate review of the overlap rule: inserting a new imported `hours_entries` row for Forever, behind both the global switch and the user count switch. Old imported rows are not updated or deleted. Recount stays a staff action and keeps its audit log.

## Before any future deploy

Backup first. Compare, before and after, counts and sums of `sessions`, `hours_entries` by tier, `steam_playtime`, `wow_characters` by `game_id`, and `wow_played_snaps`. New tables may be empty. `wow_forever_beta` row counts stay the same. Roll the site and server back together if the UI depends on new routes. Leaving the new tables in place loses nothing, because they are not the live totals.

## Tests to add with each phase

- Forever upload while the global switch is off still returns `forever_not_open` and writes no row.
- Forever upload while it is on writes raw snaps and writes zero `hours_entries`.
- The same snapshot posted twice does not add a second raw row.
- A lower `/played` is rejected and the previous snap remains.
- Retail upload still updates Retail totals as it does now, and does not attach those characters to `wow_forever`.
- A Forever lookup does not insert `wow_characters`.
- Turning count or show off deletes nothing.
- A renamed Forever surname keeps one stable id once that id exists, and does not sum both names.
- Detection fixtures for `_forever_/Wow.exe` and `WowB.exe` stay as they are.
