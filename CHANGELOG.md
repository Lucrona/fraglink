# Changelog

All notable changes to Fraglink are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to use [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once public releases begin.

## [Unreleased]

Next release: 0.3.0-beta.1 (the first beta; releases are now numbered 0.3.0-beta.1, 0.3.0-beta.2, …).

### Added
- Announcements from the Fraglink team now show as a slim banner under the header. Click it to open the link, or × to hide it.

### Changed
- Friends you put in your own groups still show under Friends Online when they're on, like Steam.
- Your groups show how many are online out of everyone in them, e.g. (2/5).
- Your own status shows just the game icon and name while you play.
- Hovering a friend shows their nickname with their @username next to it.

## [0.2.0-dev.21] - 2026-10-12
Test update (builds report `0.2.0-21`).

### Added
- **Linked accounts in the app:** Options > Connections now shows your Steam, Battle.net and Discord links. Connect opens your browser to that service's sign-in page, and the app shows the linked name with a Disconnect button. Spotify is still coming soon.
- **Same privacy switches in the app and on the website:** Options > Privacy has every "show on my profile" switch from the website (private profile, Now Playing, games and hours, activity, friends, achievements, top players, gaming rig, previous nicknames and personal details). A change in one place shows in the other.
- A "When someone sends me a friend request" pop-up switch, shared with the website's notification settings, as is "a friend comes online or starts a game".
- Options > Account links to the website for your profile picture, two-step sign-in, signed-in devices and email notifications, and shows whether two-step sign-in is on. Options > Connections links to downloading your account data.

### Changed
- **Bigger pop-ups:** the bottom-right pop-ups are now the same size as Steam's (283 × 70, a little taller for long game names) with a larger picture or game icon and easier-to-read text. They grow with Windows display scaling.
- **Your game's icon in your status:** while you play, the game's small icon sits right before "Playing …" next to your name.
- **Website:** long lists (games on a profile, played games, friends) load a page at a time with search and sorting.

## [0.2.0-dev.20] - 2026-10-12
Test update (builds report `0.2.0-20`).

### Added
- **Nicknames:** your username is your permanent sign-in name and profile address; your nickname is the name everyone sees, in the friends list, side panel, pop-ups, activity, on the website and on your signature image. Change it by clicking your own name in the app, in Options > Account, or in Settings on the website (up to 32 characters, 3 changes a day in total; it can't be someone else's name or look like staff). The side panel shows the @username under the nickname, and people can find you by either name.
- **A new profile page on the website:** a cleaner layout with more details you choose to share, achievements, recent activity, a small profile card you can embed elsewhere, and a way to report a profile.
- **Linked accounts:** connect your Steam account in your website settings so it shows on your profile.
- **Picture cropping:** when you upload a profile picture you can position and crop it first.

### Changed
- **One look:** the Classic Silver skin is gone and everyone uses the dark look. Tools > Skin is removed; the accent colour under Tools > Theme stays.
- **Friends list back to the classic look:** a friend's game icon sits in front of their name while they play (a blank space otherwise). Pictures stay in the side panel, next to your own name and in pop-ups.
- Tidier wording throughout: the tray says "Sign Out" like the main menu, Help > Contents covers the newer features, and the unused "Music Status (coming soon)" entry is gone.

### Fixed
- Hovering a friend or a button showed two tooltips at once; now there's only one, and cut-off text shows in full there.
- Some menu items were missing a space ("Check forUpdates", "Always onTop", "SignOut").
- Long side panel headings such as "Friends of Friends Playing" wrap instead of being cut off.

## [0.2.0-dev.19] - 2026-10-12
Test update (builds report `0.2.0-19`).

### Added
- **Profile pictures:** upload a picture on the website (or pick one of the presets). It shows on your profile and around the site, and in the app: in the friends list, at the top of a friend's side panel, next to your own name and in pop-ups ("is now playing" shows the game with a small picture in its corner). No picture? You get the Fraglink mark on a colour. Pictures are kept on your PC, and a new one shows as soon as it's changed.
- **Two-step sign-in:** turn on an authenticator app code in your website settings, with backup codes in case you lose your phone. The app asks for the code (or a backup code) when you sign in.
- **A fresh website:** a live home page, a new profile page, settings split into tabs, an optional forum signature image, a copy of your data to download, and account deletion (with 7 days to change your mind). You're told by email about new sign-ins once email is switched on, and you can see and sign out your other sessions.
- **Privacy controls:** profiles are public by default, and you choose what others see: what you're playing now, your games, your activity, your friends, your PC specs, your details and whether you appear on leaderboards, or make the whole profile private.

### Fixed
- A game renamed on the website now shows its new name in the app even when it was added from a player's suggestion.

### Changed
- **Friends' activity grouped per game per day:** the app's Activity Report and the website show "played World of Warcraft for 3h today" instead of one line per session.
- **Game pictures on the website** come from the game's store art where there is some, with a tidy fallback.
- **Dark menus:** in the dark skin, the Fraglink, Tools and Help menus are dark grey with a blue highlight, like the right-click menu.
- **Resizable friends list columns:** drag the dividers between Name, Status and Game Info. Your widths are remembered; double-click a divider to reset them. Text that doesn't fit ends in "…"; hover to see all of it.
- **Group headings explain themselves:** click Friends of Friends Playing, Friend Requests or Friends Offline to see a short description in the side panel. Fold a group with its arrow or by double-clicking the heading.

## [0.2.0-dev.18] - 2026-10-11
Test update (builds report `0.2.0-18`).

### Added
- **Already friends?** Add Friend search results now show "Already friends" or "Request sent", and if someone has already asked you, the button says Accept.

### Changed
- **Smoother pop-ups:** notifications are now a single dark rounded box that slides in and fades out, laid out like Steam's: the name, what happened ("is now playing", "played 1h 33m of"), then the game on its own line. Long game names show in full instead of being cut off. The close button appears when you hover.
- **Bigger Add Friend window:** easier to read, with a taller results list and a darker background behind it.
- **Side panel fits the window better:** the info panel now sits neatly inside the main window's height instead of running edge to edge.
- **WoW realm hidden for now:** realm names weren't always right, so they're switched off until they're reliable.

### Fixed
- **Add Friend now always responds:** after you click Add Friend you see "Friend request sent", or a clear message if something went wrong. Before, nothing seemed to happen.
- **Renamed games update in the app:** when we rename a game, the new name now shows in the app (your status, Activity, Stats and your games list) without needing an update.

## [0.2.0-dev.17] - 2026-10-11

Test update (builds report `0.2.0-17`). Server update: friend search matches part of a name.

### Changed
- **Dark pop-up notifications:** the small bottom-right pop-ups (a friend logged on, started playing, sent a
  friend request) now match the dark look: dark grey box with a thin border, a blue title bar with the logo
  and a close X, the game icon (or the friend's letter avatar) on the left, their name in bold, one line
  of text, and the World of Warcraft realm underneath when known. Timing and clicks work as before.
- **Add Friend is a small pop-up window:** it opens over the friends list (which dims behind it) instead of
  replacing it. One window has the search box, Cancel / Search, the results table (Username, Display Name)
  with the number of users found, a **Personal Message** and **View Profile** / **Add Friend**, which stay
  disabled until you pick a result. Enter searches and Esc closes it. Searching for yourself says
  "That's you!" instead of "No users found".
- **Shorter friend side panel:** the panel now shows just the header, **Playing Game** (with Realm and the
  Playing for timer) while they're in a game, and **Stats** with This week and Total. The Recent games
  list and the Top game row are gone, and the spacing is tighter.
- **Server: friend search matches part of a name:** searching "doe" now finds "Jo Doe" or "johndoe"
  (before, only the start of a username or display name matched). Exact and starts-with matches are
  listed first.
- **Your realm on hover:** while you play World of Warcraft, hovering your status at the top of the main
  window shows the game with your realm, e.g. "Playing World of Warcraft: Forever · Frostmourne".
- **Friends' realm on its own line:** the friend side panel shows the game under "Playing Game" and then a
  separate **Realm** row (e.g. "Realm  Frostmourne"), like the classic client; the friend tooltip adds a
  "Realm:" line. Nothing is shown when there is no realm.
- **Cleaner friend side panel:** clicking a friend shows, top to bottom: a header (avatar, display name,
  username, status dot and text), **Playing Game** (bigger game icon and name, then the **Realm** row for
  WoW and a live **Playing for** timer), **Stats** (This week, Total on Fraglink, Top game), **Recent**
  (their last 3 games with hours) and **View Profile** with a small **Mute pop-ups** link underneath.
  Sections use the same heading bars as the Activity Report, rows are a muted label with the value on
  the right, and a row or section with nothing to show is left out. "Friends since" moved to the
  header's tooltip. A friend who keeps their play history private shows a one-line note instead of
  Stats. Stats and Recent come from the friend's profile (the same data as their page on
  playfraglink.com) and refresh at most once a minute. A **Message on Discord** button appears only
  for friends with Discord linked (nobody yet).
- **Your status uses the whole row:** the status under the banner (e.g. "Playing World of Warcraft: Forever")
  now takes all the room right of your name instead of a fixed 190 px box, so it only ends in "…" when it
  really doesn't fit. A very long username is shortened at about half the row. Hovering still shows the
  full status with your realm.
- **Side panel headings fit:** the links in the "Activity Report" and "N Installed Games" heading bars
  are now **More** and **Detect** (the full wording is in their tooltips), so the headings are no longer
  cut off at the default panel width. A heading that still doesn't fit ends in "…".

## [0.2.0-dev.16] - 2026-10-05

Test update (builds report `0.2.0-16`). No server change needed.

### Changed
- **One-click install.** The setup no longer shows a wizard: no welcome, licence, folder or shortcut
  pages. It installs just for you (no admin prompt) into `%LOCALAPPDATA%\Fraglink`, adds Desktop and
  Start menu shortcuts, shows a small dark Fraglink window ("Installing Fraglink…" and a progress bar,
  the same look as the startup window) and then opens Fraglink at its sign-in screen. Running it
  again updates the install in place. The uninstaller in Windows Apps & features is unchanged.
- **Updates** use the same window ("Updating Fraglink…") and restart Fraglink as before; nothing to click.
- **Announcements:** while signed in, a message from the Fraglink team (the website's banner) shows once as
  a small notification; click it to open its link.
- **Options > Games** lists only your games: installed ones, plus games you played on this PC before.
  The long "Not installed" list is gone. A new sort menu offers A–Z, Most played and Recently played,
  and remembers your choice.
- **Scan for games:** on first start Fraglink looks through your game folders once, in the background at
  low priority (`Games` folders on every fixed drive, game folders under Program Files and your
  launchers' library folders), and adds the games it recognises to Installed games. Run it again any
  time with the new **Scan for games** button in Options > Games. It only reads file and folder names,
  skips Windows and system folders, ignores launchers and test builds (PTR / Public Test), and the
  results stay on your PC.
- **Star Citizen** now shows as installed when the RSI Launcher installed it (found from the launcher's
  own settings and the `Roberts Space Industries` folders). Launch opens the RSI Launcher.
- **Star Citizen's icon** is now the official Star Citizen emblem.
- Under the games list a hint explains what to do when a game is missing: launch it once, or click
  Scan for games.
- The MSI (for managed installs) is unchanged.

## [0.2.0-dev.15] - 2026-10-05

Test update (builds report `0.2.0-15`). Needs the dev.15 server (deployed the same day).

### Changed
- **Sharper, official game icons.** Game icons now use each game's official icon, served by the Fraglink
  server from our own domain in 16, 32 and 64 px (cached, with long cache headers and an occasional refresh).
  The app never downloads images from third-party sites. The previous icon set is gone.
- **Steam games** without an icon in the games list use their official Steam icon.
- **Fallback:** games the server has no icon for use the icon from the game's own exe on your PC (read
  locally, never uploaded), otherwise a neutral game symbol.
- Old cached icons are cleared once after the update, so every game picks up its new icon.

## [0.2.0-dev.14] - 2026-10-05

Test update (builds report `0.2.0-14`). Needs the dev.14 server (deployed the same day).

### Changed
- **Hours only count while you're connected.** The Fraglink server now records play time itself from the
  live presence connection (game start, game stop, connection lost; a connection silent for 2½ minutes ends
  at its last heartbeat + 45 s). The app no longer tracks anything while offline or signed out, keeps no
  offline queue and no longer uploads sessions. Session uploads from older apps are acknowledged (so they
  clear their queue) but nothing from them is stored. Existing sessions and hours are untouched.
- **Game names come from the server.** The app sends only a game id; the server names it from its own
  games list, Fraglink's rules and signatures, Steam store names and games an admin approved. Any name a
  client sends is ignored. Unknown ids show as **Playing a game** (presence `game_id` `unlisted`, name
  `a game`; "Unlisted game" in stats). Typed game names on the website and in the hours API must match a
  listed game; nobody can create or rename games.
- **"Track this as a game?" / "This is a game…" programs** are sent as `exe:<program>.exe` and show to
  everyone else as **Playing a game** (friends list, tooltip, side panel, Activity Report, website) until an
  admin approves them (`fraglink-server approve-game <exe> <name>`). Your own label only shows on your PC.
  Approved games keep the hours already recorded and show under the approved name; they're also handed to
  every app as detection rules.
- **"Show my game to my friends" off** still counts the time (the game is sent marked hidden); friends see no game.
- **Improved game detection, World of Warcraft tidied up:** World of Warcraft (retail, incl. `_ptr_` / `_beta_` folders),
  World of Warcraft Classic and World of Warcraft: Forever (`WowB.exe`). "Public Test" / PTR entries
  are skipped and not tracked. Fraglink's own WoW folder rules (PTR, Beta, Classic Era, Classic PTR,
  Anniversary splits) are removed; the `wow` id is unchanged. The realm still shows in the friend side panel
  and tooltip.
- Reconnecting after a server restart or a full server uses exponential backoff with jitter and honours
  `Retry-After`.

### Server
- Migration `0010`: `games.verified` (approved/known names) and `presence.game_hidden`.
- Admin commands: `fraglink-server suggestions`, `approve-game <exe> <name>`, `unapprove-game <exe>`.

## [0.2.0-dev.13] - 2026-10-05

Test update (builds report `0.2.0-13`).

### Changed
- **WoW realm** now shows only in the friend side panel and the friend hover tooltip
  ("World of Warcraft · Frostmourne"). The friends list row, your own status and the web friends page show
  just the game again. The realm is still sent with presence (`game_detail`) as in dev.12.

## [0.2.0-dev.12] - 2026-10-05

Test update (builds report `0.2.0-12`).

### Added
- **Much bigger, more accurate games list** (about 10,000 Windows games). The Fraglink
  server refreshes the list once a day, keeps the last good copy if that fails, and serves a compact
  Windows-only version (no launchers) at `GET /api/v1/games/detectable` (gzip, ETag). The app only gets it
  from the Fraglink server, caches it, refreshes it at startup and every 24 hours, and falls back to a
  snapshot bundled in the app plus the existing signatures when offline. Matching uses the exe name and,
  for entries with a folder (`_retail_/wow.exe`), the end of the full path; the most specific match wins.
- **Folder rules that run first** (`packages/signatures/overrides.json`, updatable from the server at
  `GET /api/v1/games/overrides` so a renamed beta exe can be fixed without a release): World of Warcraft
  (`_retail_`), WoW Classic (`_classic_`), WoW Classic Era (`_classic_era_`), World of Warcraft: Forever
  (`_classic_beta_` or `WowB.exe`), WoW PTR (`_ptr_`, `_xptr_`), WoW Beta (`_beta_`), and Star Citizen
  (LIVE / PTU / EPTU / Hotfix / Tech Preview). Existing game ids (e.g. `wow`) stay the same.
- **WoW realm:** for every WoW version Fraglink reads `WTF\Config.wtf` next to the game (`SET realmName`)
  when it starts and whenever the file changes, and shows it after the game: "World of Warcraft ·
  Frostmourne" in your status, the friends list, the friend tooltip and the side panel. It's sent with your
  presence (new optional `detail` / `game_detail` field; older clients and servers ignore it). Hidden when
  there's no realm.
- **"Track <name> as a game?"**: a program that isn't on any list and stays in front, full screen or
  borderless over the whole monitor, for 5 minutes gets a small question (popup and a bar in the main
  window) with **Yes** / **Not a game**. Yes tracks it from then on and sends the exe and name to the server
  for review (`game_suggestions`, nothing is published automatically); Not a game is never asked again.
  Browsers, Explorer, video players, Discord, OBS and similar apps are never asked about.

### Changed
- Launcher install records (Epic, GOG, Battle.net, EA, Ubisoft, Xbox) are only used for the Installed
  Games list and Launch buttons, no longer to decide whether a running program is a game. A game the list
  recognises inside such an install keeps its old id, so its hours stay together.
- **Friend tooltip** is dark (dark grey background, light text) to match the client.
- **Automatic Away** is back to 10 minutes without input (dev.11 used 20 seconds for testing).

### Server
- New `game_suggestions` table (migration 0007) and `presence.game_detail` column (migration 0008).
- `POST /api/v1/games/suggest` (exe + name, rate limited, no account needed).

## [0.2.0-dev.11] - 2026-10-04

Test update (builds report `0.2.0-11`).

### Changed
- **Server address:** the client now talks to `https://playfraglink.com` (HTTPS, and `wss://` for live
  presence). If HTTPS or DNS fails it switches to `http://119.42.54.86` for the rest of the session.
  A saved server address pointing at the IP moves to the domain (same server).
- **Updates** are checked on `https://playfraglink.com/updates/` first and on the IP second; the update
  download is HTTPS.
- **Startup window:** after "Checking for updates…" it shows "Detecting games…" while the installed-games
  scan runs (at most about 2 seconds; a slower scan finishes in the background).
- **Friends list:** a little space above each group (Friends Online, Friends of Friends Playing, Friends
  Offline) and slightly taller rows.
- **Side panel:** Activity Report and Installed Games have heading bars like the blue "Friends" strip, with
  View More / Detect Games on the right. The "« My Games" link is gone: click empty space in the friends
  list (or press Esc) to deselect and go back to the default view.
- **About box:** shorter credits (logo and font licences).
- Website links now point at playfraglink.com.

### Testing only
- **Automatic Away after 20 seconds** without input (normally 10 minutes) so it's quick to test. The next
  build sets it back to 10 minutes.

## [0.2.0-dev.10] - 2026-10-03

Test update (builds report `0.2.0-10`).

### Added
- **Startup window** like Discord's: a small dark borderless window with the logo,
  "PLAY. CONNECT. TRACK.", "Checking for updates…" and a progress bar. A newer build is
  downloaded (progress shown), checked, installed, and Fraglink relaunches into it; otherwise the
  window closes after about a second and the client opens. Offline or a slow server: it gives up
  after 5 seconds and opens normally. Not shown when Fraglink starts to the tray (Windows startup /
  "Start minimised"), like Discord's "Start Minimized"; the background check covers that.
- **Check for Updates** in the Help menu and the tray menu: "You're up to date", or the update
  downloads, installs and restarts.
- **Automatic Away** like Discord: after 10 minutes without keyboard or mouse input your status
  shows "Away" (or "World of Warcraft (Away)" in a game) for you and your friends, and switches back
  on input. Hours keep counting. A status you picked yourself (Away, Invisible) isn't changed. No
  setting; the unused "Show me as Away after…" box is gone from Options.

### Changed
- **Background update check** every hour (was every 6 hours).
- **Update ready** is a small bright green download arrow in the title bar (Discord style) instead
  of the green "Update ready - Restart" pill. Tooltip "Update ready - click to restart"; one click
  installs and restarts. The tray dot and "Restart to Update" stay.
- **Title bar logo** is bigger (grey, embossed).
- **Info panel resize:** the whole right frame strip of the panel (scrollbar to window edge) is now
  the drag handle, wider than the window's own resize border, so it no longer fights it.
- **Game icons:** installed games without a server icon (e.g. Satisfactory) now get their icon
  from Steam's library cache or from the game's own exe (found in its install folder), cached like
  the others. Before, that only worked once the game had run in the current session.
- **Options > Games:** "Detect path" shows the exe actually found (full path) instead of the list of
  candidate exe names; "Game path" is shown in normal Windows form with the real casing
  (`E:\Steam\steamapps\common\Satisfactory\`) instead of lowercase with forward slashes.
- **Every session counts** (like Steam): "Ignore sessions shorter than…" and "Pause tracking" are
  gone from Options > Connections > Tracking, along with their logic.
- **Presence connection:** the client pings every 30 s (was 60 s). In testing, a network on the
  way cut idle connections after exactly 60 s (the server's own limit is 150 s), which could make a
  friend flicker offline/online.

## [0.2.0-dev.9] - 2026-10-03

Test update (builds report `0.2.0-9`) with the feedback from testing dev.8.

### Changed
- **Slogan** is now "PLAY. CONNECT. TRACK." (banner, About box, installer sidebar, README, social preview).
- **Title bar:** bigger embossed grey Fraglink logo (like the classic client's), chunkier minimise
  and close buttons, and a slightly taller bar.
- **Update ready** is now a clear green "Update ready - Restart" pill in the title bar (glows briefly
  when it appears). The tray dot and the tray "Restart to Update" item stay.
- **Tray:** the tooltip is just "Fraglink"; "Open Fraglink" is the first menu item; Launch entries show
  the game icons. The tray Status submenu uses the same items as the main window.
- **Status menu** and the **friend right-click menu** are real Windows menus. Status: Online, Away,
  Invisible, Looking to Play, Music Status (disabled until Spotify), then Set Custom Status... and
  Reset Status (recent custom statuses sit in a submenu). Group headings' right-click menu too.
- **Group headings:** clicking one shows a grey selection bar and no longer folds the group or swaps
  the info panel to a help text. Fold with the arrow.
- **Info panel:** default content is an **Activity Report** (friends' recent games: "X started
  playing Y", "X played Y for 2h 14m", with View More) and then **N Installed Games** with Detect
  Games and right-pinned Launch links. Drag the panel's outer edge to make it wider (180-560 px,
  remembered); game names wrap to two lines when narrow. Selecting a friend shows their game,
  "Playing for 2h 14m" and their hours this week.
- **Bottom bar:** the duplicate buttons under the info panel are gone; the main bar is Add Friend,
  Profile, Games, and Options on the right.
- **Dialogs** (About, notices, Games) can be dragged by their title bar.
- **Options window** has the same custom dark title bar as the main window (drag, minimise, close).
  Games tab uses the classic layout: the game list with icons (Installed / Not installed) on the left
  and the Select Game settings on the right.
- **Tracking tab is now Connections:** Steam, Battle.net, Spotify and Discord rows (Connect buttons
  disabled, "coming soon") and a small Tracking section (ignore short sessions, Pause tracking,
  Export my data, Open data folder).
- **No idle cutoff:** hours count all the time a game runs, like Steam. The "Stop counting after N
  minutes without input" option is gone and Fraglink no longer reads keyboard/mouse idle time.

### Added
- **Text size** in Options > Appearance (Normal, Large, Larger) scales the whole main window.
- **Privacy:** "Show my recent games and hours this week to friends" (on by default; friends only).
- **Server:** `GET /api/v1/friends/activity` (friends' live games + sessions from the last 7 days),
  `GET /api/v1/friends/{handle}/summary` (presence, hours this week / last 7 days, top game; friends
  only) and `POST /api/v1/me/privacy` (`share_activity`, migration 0006). `GET /api/v1/me` reports
  `share_activity`.

## [0.2.0-dev.8] - 2026-09-29

Small test update (builds report `0.2.0-8`), mainly to try the new quiet auto-update from dev.7.

### Changed
- **Tray icon** now uses the silver app-icon design instead of the old dark-blue badge. It's drawn
  for each size Windows uses (16/20/24/32 px for 100-200 % scaling), and the update-ready green dot
  is drawn crisply at each size. The app icon (`icon.ico`) gains a 20 px frame, and the old badge
  favicon and unused old logo files are gone.
- **About box** restyled to the dark theme with new text: version, "Track your game hours and see
  what friends are playing.", © 2026 Lucrona, and the same logo/font credits.
- **Dialogs** (notices, the Games dialog) and the **Options window** use the dark 2013 style in the
  Dark and Phoenix skins. Classic and Stream keep their silver/system look.

### Added
- **Account management** in Options > Account (when signed in):
  - **Change Password:** current password, new password and confirm. At least 10 characters, with
    clear messages. Every other sign-in of the account (other PCs, browser sessions) is signed out;
    this PC stays signed in.
  - **Change Email:** shows the current email; needs the new email and your current password. The
    address must be valid and not used by another account. It's stored as not verified until
    Fraglink has a mail service.
- **Server:** `POST /api/v1/me/password` and `POST /api/v1/me/email`. Both need the current password
  and are rate limited (5 attempts per 15 minutes per account). `GET /api/v1/me` now reports
  `email_verified` (migration 0005).

## [0.2.0-dev.7] - 2026-09-28

Pre-release for private testing (builds report `0.2.0-7`). Asked for as "0.2.0-dev.6.1", but
Windows MSI versions only allow one numeric pre-release number, so it's dev.7 (sorts after dev.6
for the updater).

### Changed
- **Classic game icons are the default everywhere.** Icon priority for every game, including
  your own installed ones: 1) the matched classic icon from the server, 2) Steam's icon (server CDN
  answer or Steam's local library cache), 3) the game's exe icon, 4) a generic icon. Friends' games
  use the same order. Icons fetched from the server are cached on disk, so they work offline after
  the first fetch.
- **Updates, Discord style.** No more "Restart to update?" prompt. A new build downloads quietly in
  the background and its signature is checked. Then a small green down arrow appears in the title
  bar next to the window buttons ("Update ready: click to restart and update (version)"). One click
  installs and restarts. The tray gets a **Restart to Update** item and a small green dot on its
  icon. If you ignore it, the update is applied the next time Fraglink starts. dev.6 still shows
  its old prompt once for this update.

### Server
- `GET /api/v1/games/icons` always includes the matched classic icon (`icon_path`), even when there's
  a Steam icon, plus which source is preferred.
- Optional classic icon source (turned on by a server setting, see `docs/deploy.md`); the images are
  not in the repo or the installer.

## [0.2.0-dev.6] - 2026-09-28

Pre-release for private testing (builds report `0.2.0-6`).

### Added
- **Accounts inside the client.** The Login screen takes your username (or email) and password; **Create Account** makes an account right there (username + password, email optional). "Sign in with your browser instead" keeps the old pairing flow.
- **Add Friend works for real:** search Fraglink users by username or display name (or exact email), pick one and send a request. They get it under Friend Requests and Accept or Decline in the info panel; once accepted you see each other with live status and game. (The Personal Message isn't sent yet.)
- **Server:** `POST /api/v1/device/signup` and `/device/login` (device tokens for the client), `GET /api/v1/users/search`, login by username as well as email, email optional (migration 0004), and rate limits: 5 sign-ups per IP per hour, 30 sign-in attempts per IP per 15 minutes (plus 10 failures per account), 30 friend requests per hour and 60 searches per minute per user.
- **Auto-update.** Fraglink checks quietly at startup and every 6 hours. When a newer build is out it asks "An update is available. Restart to update?"; Yes downloads it, checks its signature, installs and restarts. Updates come from `http://119.42.54.86/updates/latest.json` and are signed, so a tampered file is refused. dev.5 has no updater, so install dev.6 by hand once.

### Changed
- **Installer:** dark, understated sidebar and header art (charcoal gradient, silver mark and wordmark); still a per-user install with no admin prompt.
- **New app icon:** a light silver tile with a charcoal Split Orbit "F", hand-tuned at 16, 24, 32 and 48 px so it stays crisp on the desktop, Start menu, taskbar and installer. The tray icon is unchanged.
- **The client now uses the Fraglink server** at `http://119.42.54.86` by default (it was a server on your own PC). You can still change it in Options > Account or under Login. A saved `http://localhost:8787` from an older build switches to the new default unless this PC is signed in to it.

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
- **First run:** the main window shows "Detect installed games" with a progress bar and **Skip** before the friends list.
- **Signed out:** the main window shows the classic Login screen: "(Offline)" header and status, a Login bar, a small centred form (Username drop-down of accounts used on this PC plus &lt;New User&gt;, New User Registration, Login, Forgot Password, Save Password and Auto-Login), and an info panel with Learn About Fraglink, New Users and Resources (FAQ, Support, Release Notes). Login confirms in your browser (device pairing), so the password box is a placeholder; a Server field sits under a fold. While signing in it shows a progress bar, "Signing in…", the pairing code, "Time out in N secs" and Cancel.
- **Friend tooltip:** hovering a friend for a moment shows username, nickname, game, server, IP:port and ping; friends of friends add Common Friends; offline friends show when they were last online. It opens beside the row, never over it.
- **Friend right-click menu:** View Profile, Join Game (only on a joinable server; opens `steam://connect/<ip:port>`), Add to / Remove from Favorites, Move to Group (custom groups + Add Custom Group…), Remove Friend… and Block User… (both ask first).
- **Status dropdown:** Reset Status (back to automatic Playing <game> / Online) and your last 5 custom statuses.
- **Sign-on toasts:** "<name> logged on" / "<name> started playing <game>" in the bottom-right, with View More to jump to the friend. Options > General > Notification alerts: when a friend logs on or off, joins/leaves a game, and when connection state changes.
- **Add Friend** (the + button, or Tools > Add Friend > Search Fraglink Users…) opens inside the main window under an "Add Friend" bar: search box, example line, Cancel / Search (Search is off until you type) and "Invite friends to join Fraglink"; the results step (Username / Display Name table, Personal Message, View Profile / Add Friend) shows in the same place. Cancel goes back to the friends list. Tools > Add Friend > Invite via Email… is a placeholder.
- Options > Games: per-game Automatically Detect / Manual Detect with Browse; an **Apply** button in the footer.
- **Tray menu** (right-click) now matches the classic client: Launch > (installed games A–Z, then Detect Games), Skin > (current skin ticked, then More Skins…), Status > (Reset Status, Away From Keyboard, Invisible and your recent custom statuses), Log Off…, Exit. Left-click still opens the window.
- The info panel is joined flush to the main window at full height, with the ◀/▶ tab on the seam; the window grows by the panel width when it opens.
- Tools > Launch ends with a separator and **Detect Games**; the idle info panel reads "<N> Installed Games" with a Detect Games link.

### Changed
- The info panel changes only when you click a row, the same for every group. A clicked friend of a friend shows their details plus a **Mutual Friends** section; hovering no longer changes the panel.
- The plain name tooltip that repeated every name is gone (replaced by the friend tooltip above), and so is the "Right-click for options" tooltip on group headings.
- Selected-friend panel: large display name, Username, a Profile link, then status or Playing Game with icon, title and server.
- Fraglink > My Stats is now **Activity Report** (still opens the website).
- Options > Games shows the launcher name next to each game (Steam, Battle.net, EA app, Ubisoft Connect, Xbox, Epic, GOG).

### Not finished yet (saved, but inert)
- Pause tracking, Export my data and the data/log folder items left the tray menu; pause and export are still in Options, and the folders in Options > General > Advanced.
- "When connection state changes" alerts; Invite via Email (shows a note); Add Friend has no directory search yet (it offers the exact username you typed).
- Favorites and Move to Group are remembered, but there's no Favorites group or friends inside custom groups yet.
- Manual Detect only changes which exe Launch uses; detection itself still follows the signature list.
- Password sign-in: the Login form uses browser pairing, so the password box is disabled and Save Password / Auto-Login are saved but inert. The Resources / registration links go to placeholder website pages.

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

### Added (game icons, classic style)
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
