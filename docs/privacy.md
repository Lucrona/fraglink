# What Fraglink reads, stores and sends

**Reads (read-only, every 5 s):**
- The list of running processes: exe name, full path, PID. The path is only looked up once per new process.
- Steam's install location and the `RunningAppID` value in `HKCU\Software\Valve\Steam`.
- Steam's `libraryfolders.vdf` and `appmanifest_*.acf` files (game names and install folders). Refreshed every 10 min.
- Epic Games Launcher install manifests (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item`: game name, install folder, main exe) and GOG Galaxy's install list in the registry (`HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`: name, folder, exe). Refreshed every 10 min. Nothing else in those files is used.
- Other launchers' install lists (from dev.5): the Windows Uninstall entries (HKLM and HKCU), Battle.net's `%ProgramData%\Battle.net\Agent\product.db`, Ubisoft Connect's install keys and `<drive>:\XboxGames` folders. Only game names, install folders and launch commands are used.

**Never:** reading or writing game memory, injecting DLLs, hooks, overlays, screenshots, keylogging, watching keyboard/mouse input (since dev.9 not even the idle time), or reading chat/window contents. This is what keeps it safe with anti-cheat systems (VAC, EAC, BattlEye, Vanguard, Ricochet).

**Stores (locally, SQLite in `%APPDATA%\gg.fraglink.desktop\`):** games seen, play sessions (start, end, active seconds; the idle field is always 0 since dev.9), daily totals, your manual tags and exclusions, your local activity feed, and settings.

**Sends:** no telemetry. Signed out, the only network request is the **update check** (from dev.6): at startup and every 6 hours the app downloads `http://119.42.54.86/updates/latest.json`, and it downloads an update only if you say Yes; updates are signature-checked. Nothing about you or your games is sent until you log in (Login or Create Account in the main window, or browser pairing).

**After you sign in**, the app uploads to the server you chose (default: the Fraglink server at `http://119.42.54.86`, changeable in Options > Account):
- closed game sessions: a random session id, game id and name, Steam app id if known, start and end time, active seconds, an idle field (always 0 since dev.9), and the app version.
- **Never:** program names, install paths, the list of running programs, idle timestamps or anything about other apps.

The device token is encrypted with Windows DPAPI on your PC. Signing out forgets it and revokes it on the server. Excluded games are never uploaded. Sessions already on the server stay there until you delete them (or your account) on the website. Details: [`decisions.md`](decisions.md) D4, D9, D14.

**Friends and presence (M3, only while signed in):** while the app runs it keeps a WebSocket to the server and sends your status (Online / Away / Invisible) and the game being tracked right now (id, name, start time), plus a ping every minute. Only your friends see it. Friends also see your recently played games (sessions of 1 min or more from the last 7 days) and your hours this week and over the last 7 days; turn this off with **Options › Privacy › Show my recent games and hours this week to friends** (from dev.9). **Invisible** is shown to them as offline with no game. Friend pop-up settings and muted friends stay on this PC. Full draft policy: [`privacy-policy.md`](privacy-policy.md).

**Game icons:** read from the detected game's exe and Steam's local library cache, then cached in `icons\` in the data folder. While you're signed in, icons of friends' games that aren't installed on this PC may be downloaded from Steam's public CDN.

**Your control:** Pause tracking (Options), exclude any game, Export my data (a zip with JSON + CSV), and delete everything by quitting and removing the folder above.
