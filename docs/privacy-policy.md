# Fraglink Privacy Policy (DRAFT v1)

> **DRAFT. Not legal advice.** This draft was written by the developers to describe what the code
> in this repository actually does as of September 2026 (tray 0.1.x plus the M2/M3 server).
> A lawyer needs to review it before Fraglink is offered to the public, especially for the
> Australian Privacy Act, GDPR/UK GDPR and COPPA. Items marked **TBD** aren't decided yet.

Last updated: 26 September 2026 · Operator: **TBD** (who runs the public server) · Contact: **TBD**

## The short version

- The Fraglink tray app works **fully offline**. It records which games you play and for how long,
  on your PC. Nothing is sent anywhere unless you sign in.
- If you sign in, your **game sessions**, your **friends** and your **current status and game**
  are stored on the Fraglink server so they work across devices and your friends can see them.
- We don't sell data, show ads, use analytics or tracking scripts, or share your data with anyone
  except where listed below.
- You can **export everything** and **delete your account** at any time. Deletion is immediate.

## 1. The tray app on your PC

**What it reads** (read-only, about every 5 seconds):

- The list of running programs (name, full path, process id), which it compares against a list of known games.
- Steam's current game id and library files, Epic Games install manifests and GOG Galaxy's install
  list. It uses these only to recognise games you installed through them.
- Nothing about your keyboard or mouse. Since version 0.2.0-dev.9 playtime counts all the time a
  game is running, like Steam, so the app no longer reads even your idle time.

**What it never does:** read or change game memory, inject code into games, hook input, take
screenshots, read chat or window contents, or log keystrokes. That's also why it's safe to use
alongside anti-cheat software.

**What it stores on your PC** (a SQLite file in `%APPDATA%\gg.fraglink.desktop\`): games it has
seen, play sessions (start, end, active time), daily totals, games you tagged or
excluded, a local activity list and your settings. If you sign in, it also stores your username,
the server address and a sign-in token that Windows DPAPI encrypts.

**Signed out, the app makes no network connections.** It has no telemetry, crash reporting or
update pings.

## 2. If you sign in to sync

Signing in is optional. You start it in **Options → Sync** and approve a pairing code in your browser.

**What the tray uploads:**

| Data | Why |
|---|---|
| Finished game sessions: a random session id, the game's id and name, its Steam app id if known, start and end time, active seconds, an idle field that is always 0 since dev.9, app version | So your hours work across PCs and appear on your profile and stats |
| While the app is running: your status (online, away or invisible) and the game you're playing right now (id, name, start time), plus a keep-alive message every minute | So your friends can see you're online and what you're playing |
| Your recently played games (last 7 days) and hours this week, worked out from the sessions above | Shown only to your friends in their Activity Report and friend details. Turn it off in **Options → Privacy** |

**Never uploaded:** program names or paths, your list of running programs, idle timestamps,
anything about non-game apps, or sessions for games you excluded.

## 3. Your account on the server

When you create an account we store:

- **Email address**, used to sign in. We don't send marketing email. Email verification and
  password reset aren't built yet (**TBD**).
- **Username** (handle) and **display name**. Other Fraglink users can see these.
- **Password**, stored only as an Argon2 hash. We can't read your password.
- **Sign-in tokens**, stored only as hashes. Website sign-ins expire after 30 days. Tray (device)
  tokens last until you sign out in the tray or revoke the device on the website. For each device
  we keep its name (for example "Fraglink on DESKTOP-1"), when it was created and when it was last used.
- **Game data:** your uploaded sessions, hours you **Imported** yourself (for example WoW `/played`,
  plus an optional note of up to 140 characters) and **Estimated** hours from the history calculator
  (the years and hours per week you entered). The website labels these tiers ● Tracked,
  ◆ Imported and ~ Estimated. Imported and Estimated hours never count towards rankings.

### Steam (optional)

If you link Steam, we store your **SteamID64**, the display name Steam gives us, and your
**official playtime per game** from Steam's Web API (`GetOwnedGames`). Your Steam profile's
**game details must be public** for this to work. We use Steam's OpenID sign-in to confirm the
account is yours. We never see your Steam password. Unlinking Steam on its own isn't built yet
(**TBD**). Deleting your account removes the link and the imported playtime.

## 4. Friends and presence

- **Friend requests are by exact username only.** There's no public directory or search, and no
  address-book or contacts upload.
- We store friend requests, friendships (with the date they were accepted) and your **block list**.
- **Friends can see:** your username, display name, when you became friends, and your presence,
  meaning online, away or offline plus the game you're playing right now and since when.
  Non-friends see none of this.
- **Invisible:** if you pick *Invisible*, the server stores that choice but shows you to everyone
  as **offline**, with no game.
- If the tray disconnects, or hasn't been heard from for about 2½ minutes, you show as offline.
  We keep only your latest presence (one row per account), not a history of it.
- **Blocking** is silent. The blocked person isn't told, can't send you requests and no longer sees
  your status. Declined requests are also deleted silently.
- "Started playing" pop-ups are shown by your friends' tray apps. Each person controls them in their
  own settings (off, not while they're playing, or muted for a particular friend). Those settings
  stay on their PC.

### Game icons

The tray shows each game's icon. It reads icons from the game's own files and from Steam's local
cache on your PC. For a friend's game you don't have installed, a signed-in tray asks the Fraglink
server for the game's public Steam icon and downloads that image from Steam's CDN (Valve). That
request includes no account details, but like any web request it shows Valve your IP address.

## 5. Cookies

The website sets **one cookie**, `fl_session`. It's HttpOnly and SameSite=Lax, and it keeps you
signed in. There are no analytics, advertising or third-party cookies, and no tracking pixels.

## 6. Who else gets your data

- **Hosting provider:** **TBD** (where the public server and database run, and in which country).
  Until that's decided, the code only runs on your own machine.
- **Steam/Valve:** only if you link Steam. We send your SteamID to Steam's API to read playtime.
- **Nobody else.** We don't sell, rent or share personal data for advertising.
- We'd disclose data only if the law requires it (**TBD**: wording about law-enforcement requests).

## 7. Your choices and rights

- **Export:** the website offers a JSON export (any time) and a zip with JSON plus CSV files
  (sessions, games per tier, Steam playtime, hours entries, friends). The zip can be downloaded
  once per hour. The tray exports its local data as a zip too.
- **Delete:** deleting your account on the website (password plus a confirmation box) immediately removes your account, sessions,
  hours, Steam link, tokens, friendships, requests, blocks and presence from the database.
  Backups: **TBD** (how long database backups are kept once hosting exists).
- **Correct:** you can delete individual imported or estimated entries and exclude games in the
  tray. Changing your display name isn't built yet (**TBD**).
- **Stop syncing:** sign out in the tray. Sessions already uploaded stay until you delete them or
  your account.
- If Fraglink ever shuts down we'll give at least **90 days' notice** and keep a working export
  available during that time.

## 8. Security

Passwords are stored as Argon2 hashes and tokens are stored hashed. The tray's token is encrypted
with Windows DPAPI. Website forms have CSRF protection. Sign-in is limited to 10 failed attempts per
15 minutes per email, friend requests to 30 an hour and zip exports to one an hour. HTTPS for the
public server is **TBD** (it comes with hosting).

## 9. Children

Fraglink isn't meant for children under **TBD** (13, or 16 in some countries). We don't knowingly
create accounts for them.

## 10. Changes

We'll update this page and change the date at the top. If a change is significant (for example a
new kind of data), we'll say so in the tray and on the website before it takes effect.
