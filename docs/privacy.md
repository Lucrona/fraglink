# What Fraglink reads and stores (M1)

**Reads (read-only, every 5 s):**
- The list of running processes: exe name, full path, PID. The path is only looked up once per new process.
- Steam's install location and the `RunningAppID` value in `HKCU\Software\Valve\Steam`.
- Steam's `libraryfolders.vdf` and `appmanifest_*.acf` files (game names and install folders). Refreshed every 10 min.
- Epic Games Launcher install manifests (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item`: game name, install folder, main exe) and GOG Galaxy's install list in the registry (`HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`: name, folder, exe). Refreshed every 10 min. Nothing else in those files is used.
- Time since your last keyboard/mouse input (`GetLastInputInfo`). Only the idle duration is read, never which keys or where.

**Never:** reading or writing game memory, injecting DLLs, hooks, overlays, screenshots, keylogging, or reading chat/window contents. This is what keeps it safe with anti-cheat systems (VAC, EAC, BattlEye, Vanguard, Ricochet).

**Stores (locally, SQLite in `%APPDATA%\gg.fraglink.desktop\`):** games seen, play sessions (start, end, active seconds, idle seconds trimmed), daily totals, your manual tags and exclusions, your local activity feed, and settings.

**Sends:** nothing by default. The app makes no network connections and has no telemetry until you choose **Options → Sync (optional) → Sign in to sync** (M2 preview).

**After you sign in to sync**, the app uploads to the server you chose (default: the Fraglink server at `http://119.42.54.86`, changeable in Options > Account):
- closed game sessions: a random session id, game id and name, Steam app id if known, start and end time, active seconds, idle-trimmed seconds, and the app version.
- **Never:** program names, install paths, the list of running programs, idle timestamps or anything about other apps.

The device token is encrypted with Windows DPAPI on your PC. Signing out forgets it and revokes it on the server. Excluded games are never uploaded. Sessions already on the server stay there until you delete them (or your account) on the website. Details: [`decisions.md`](decisions.md) D4, D9, D14.

**Friends and presence (M3, only while signed in):** while the app runs it keeps a WebSocket to the server and sends your status (Online / Away / Invisible) and the game being tracked right now (id, name, start time), plus a ping every minute. Only your friends see it. **Invisible** is shown to them as offline with no game. Friend pop-up settings and muted friends stay on this PC. Full draft policy: [`privacy-policy.md`](privacy-policy.md).

**Game icons:** read from the detected game's exe and Steam's local library cache, then cached in `icons\` in the data folder. While you're signed in, icons of friends' games that aren't installed on this PC may be downloaded from Steam's public CDN.

**Your control:** Pause tracking (tray or Fraglink menu), exclude any game, Export my data (a zip with JSON + CSV), and delete everything by quitting and removing the folder above.
