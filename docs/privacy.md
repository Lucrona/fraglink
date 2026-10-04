# What Fraglink reads, stores and sends

**Reads (read-only, every 5 s):**
- The list of running processes: exe name, full path, PID. The path is only looked up once per new process.
- Steam's install location and the `RunningAppID` value in `HKCU\Software\Valve\Steam`.
- Steam's `libraryfolders.vdf` and `appmanifest_*.acf` files (game names and install folders). Refreshed every 10 min.
- Epic Games Launcher install manifests (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item`: game name, install folder, main exe) and GOG Galaxy's install list in the registry (`HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`: name, folder, exe). Refreshed every 10 min. Nothing else in those files is used.
- Time since your last keyboard/mouse input (Windows `GetLastInputInfo`, from dev.10), only to show you as **Away** after 10 minutes without input, like Discord. It's one number; never which keys or where. It doesn't change your hours and isn't stored or sent (friends only see the resulting Away status).
- Other launchers' install lists (from dev.5): the Windows Uninstall entries (HKLM and HKCU), Battle.net's `%ProgramData%\Battle.net\Agent\product.db`, Ubisoft Connect's install keys and `<drive>:\XboxGames` folders. Only game names, install folders and launch commands are used.

**Never:** reading or writing game memory, injecting DLLs, hooks, overlays, screenshots, keylogging, input hooks, or reading chat/window contents. This is what keeps it safe with anti-cheat systems (VAC, EAC, BattlEye, Vanguard, Ricochet).

**Stores (locally, SQLite in `%APPDATA%\gg.fraglink.desktop\`):** games seen, play sessions (start, end, active seconds; the idle field is always 0 since dev.9), daily totals, your manual tags and exclusions, your local activity feed, and settings.

**Sends:** no telemetry. Signed out, the only network request is the **update check** (from dev.6): at startup (the "Checking for updates…" window, from dev.10) and every hour the app downloads `https://playfraglink.com/updates/latest.json` (from dev.11; `http://119.42.54.86/updates/latest.json` if that fails, and before dev.11); a newer build is downloaded and signature-checked before it installs. From dev.12 it also downloads the public game detection lists from the Fraglink server (`/api/v1/games/detectable` and `/api/v1/games/overrides`) at startup and once a day; these are plain downloads and send nothing about your PC. Nothing about you or your games is sent until you log in (Login or Create Account in the main window, or browser pairing).

**After you sign in**, the app uploads to the server you chose (default: the Fraglink server at `https://playfraglink.com`, over HTTPS from dev.11, or `http://119.42.54.86` for the rest of the session if HTTPS can't be reached; changeable in Options > Account):
- closed game sessions: a random session id, game id and name, Steam app id if known, start and end time, active seconds, an idle field (always 0 since dev.9), and the app version.
- **Never:** program names, install paths, the list of running programs, idle timestamps or anything about other apps.

The device token is encrypted with Windows DPAPI on your PC. Signing out forgets it and revokes it on the server. Excluded games are never uploaded. Sessions already on the server stay there until you delete them (or your account) on the website. Details: [`decisions.md`](decisions.md) D4, D9, D14.

**Friends and presence (M3, only while signed in):** while the app runs it keeps a WebSocket to the server and sends your status (Online / Away / Invisible) and the game being tracked right now (id, name, start time, and from dev.12 the WoW realm), plus a ping every 30 seconds. While you're Away (10 minutes without input) it sends Away instead of Online. Only your friends see it. Friends also see your recently played games (sessions of 1 min or more from the last 7 days) and your hours this week and over the last 7 days; turn this off with **Options › Privacy › Show my recent games and hours this week to friends** (from dev.9). **Invisible** is shown to them as offline with no game. Friend pop-up settings and muted friends stay on this PC. Full draft policy: [`privacy-policy.md`](privacy-policy.md).

**Game detection (dev.12):** games are recognised from Discord's public list of detectable games (downloaded by the Fraglink server once a day; the app only ever talks to the Fraglink server) plus Fraglink's own rules. To spot unlisted games the app checks whether the window in front covers the whole screen (its size and frame only, never its contents). After 5 minutes it asks "Track <name> as a game?". **Yes** tracks it and sends that exe name and the game name (nothing else, no account id) to the server so it can be added to the list after a manual review; **Not a game** is remembered on this PC and sends nothing.

**World of Warcraft realm (dev.12):** while a WoW version runs, the app reads the realm name from that version's `WTF\Config.wtf` (`SET realmName "…"`) and shows it next to the game. While signed in it's sent with your presence like the game name, so friends see "World of Warcraft · Frostmourne". "Show my game to my friends" off hides it too.

**Game icons:** read from the detected game's exe and Steam's local library cache, then cached in `icons\` in the data folder. While you're signed in, icons of friends' games that aren't installed on this PC may be downloaded from Steam's public CDN.

**Your control:** exclude any game (Options > Games > "Don't track this game"), Export my data (a zip with JSON + CSV), and delete everything by quitting and removing the folder above.
