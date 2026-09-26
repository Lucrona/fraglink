# Testing Fraglink M1 on Windows

1. **Download:** open the latest green run under GitHub Actions (CI workflow), scroll to *Artifacts* and download `fraglink-windows-installers-<sha>`. Unzip it. It contains `Fraglink_0.1.0_x64-setup.exe` (recommended) and `Fraglink_0.1.0_x64_en-US.msi`.
2. **Install:** run the setup exe. It installs for your user only, no admin needed. SmartScreen will say "Windows protected your PC" because the build isn't code-signed yet. Click **More info → Run anyway**.
3. **First look:** the classic Fraglink window opens and an icon appears in the tray (check the ^ overflow). Left-click the tray icon to show or hide the window. Right-click it for Open / Status / Pause tracking / Export my data / Open data folder / Open logs folder / Quit. Closing or minimising the window keeps Fraglink running in the tray.
4. **Play something:**
   - A Steam game (any game, no list needed)
   - A Battle.net or Riot game (e.g. WoW, Overwatch 2, League, Valorant)
   - Something standalone (e.g. Minecraft launcher, RuneLite)

   Within about 5–10 s a bottom-right pop-up should say you started playing, and the **Stats** tab should show *Now Playing* with a running timer. Quit the game. After about 10 s a "stopped" pop-up appears. Sessions under 2 minutes are not kept.
5. **Check the numbers:** Stats shows Today / This week / Streak and your top games. The **Activity** tab lists your started/stopped events. Leave a game running and walk away for more than 15 min. That idle time should not count.
6. **Unknown game?** Use *Fraglink → This is a game…* while it runs, pick the program, and it counts from then on. *Tools → Manage games* lets you exclude a game or undo a tag.
7. **Crash safety (optional):** while a game runs for a few minutes, end Fraglink in Task Manager and start it again. The session should be recovered, up to the last minute.
8. **Autostart:** off by default. Turn it on under *Tools → Options* if you want it to start with Windows (it starts hidden in the tray).
9. **Where the data lives:**
   - Database: `%APPDATA%\gg.fraglink.desktop\fraglink.db`
   - Logs: `%LOCALAPPDATA%\gg.fraglink.desktop\logs\fraglink.log`
   - Exports: `Documents\Fraglink\`
10. **Sending feedback/logs:** tray → *Open logs folder*, and send `fraglink.log`. Optionally also tray → *Export my data* and send the JSON/CSV. Mention which games weren't detected or were mis-detected, and what the tray showed. It also helps to note Fraglink's memory/CPU in Task Manager: include both "Fraglink" and the "Microsoft Edge WebView2" entries while the window is open, then again after hiding it to the tray.
11. **Uninstall:** *Settings → Apps → Fraglink → Uninstall*. Your data folder is kept; delete `%APPDATA%\gg.fraglink.desktop` to remove it.
