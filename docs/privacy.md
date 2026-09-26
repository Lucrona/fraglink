# What Fraglink reads and stores (M1)

**Reads (read-only, every 5 s):**
- The list of running processes: exe name, full path, PID. The path is only looked up once per new process.
- Steam's install location and the `RunningAppID` value in `HKCU\Software\Valve\Steam`.
- Steam's `libraryfolders.vdf` and `appmanifest_*.acf` files (game names and install folders). Refreshed every 10 min.
- Time since your last keyboard/mouse input (`GetLastInputInfo`). Only the idle duration is read, never which keys or where.

**Never:** reading or writing game memory, injecting DLLs, hooks, overlays, screenshots, keylogging, or reading chat/window contents. This is what keeps it safe with anti-cheat systems (VAC, EAC, BattlEye, Vanguard, Ricochet).

**Stores (locally, SQLite in `%APPDATA%\gg.fraglink.desktop\`):** games seen, play sessions (start, end, active seconds, idle seconds trimmed), daily totals, your manual tags and exclusions, your local activity feed, and settings.

**Sends:** nothing. M1 makes no network connections and has no telemetry. Friends and sync come in later milestones and will be opt-in.

**Your control:** Pause tracking (tray or Fraglink menu), exclude any game, Export my data (JSON + CSV), and delete everything by quitting and removing the folder above.
