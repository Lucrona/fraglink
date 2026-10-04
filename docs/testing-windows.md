# Testing Fraglink M1 on Windows

1. **Download:** open the latest green run under GitHub Actions (CI workflow), scroll to *Artifacts* and download `fraglink-windows-installers-<sha>`. Unzip it. It contains `nsis/Fraglink_0.1.0_x64-setup.exe` (recommended) and `msi/Fraglink_0.1.0_x64_en-US.msi`. (Downloading artifacts needs you to be signed in to GitHub.)
2. **Install:** run the setup exe. It installs for your user only, no admin needed. SmartScreen will say "Windows protected your PC" because the build isn't code-signed yet. Click **More info → Run anyway**.
3. **First look:** the classic Fraglink window opens and an icon appears in the tray (check the ^ overflow). Left-click the tray icon to show or hide the window. Right-click it for Open / Status / Pause tracking / Export my data / Open data folder / Open logs folder / Quit. Closing or minimising the window keeps Fraglink running in the tray.
4. **Play something:**
   - A Steam, Epic or GOG game (any game, no list needed)
   - A Battle.net or Riot game (e.g. WoW, Overwatch 2, League, Valorant)
   - Something standalone (e.g. Minecraft launcher, RuneLite)

   Within about 5–10 s a bottom-right pop-up should say you started playing, and the **Stats** tab should show *Now Playing* with a running timer. Quit the game. After about 10 s a "stopped" pop-up appears. Sessions under 2 minutes are not kept.
5. **Check the numbers:** Stats shows Today / This week / Streak and your top games. The **Activity** tab lists your started/stopped events. Leave a game running and walk away for more than 15 min. That idle time should not count.
6. **Unknown game?** While it runs, open *Tools → Options → Games → Add game manually…*, pick the program, name it, and it counts from then on. Select any game in that tree to set its path or launch arguments, or tick *Don't track this game*.
7. **Crash safety (optional):** while a game runs for a few minutes, end Fraglink in Task Manager and start it again. The session should be recovered, up to the last minute.
8. **Autostart:** off by default. Turn it on under *Tools → Options* if you want it to start with Windows (it starts hidden in the tray).
9. **Where the data lives:**
   - Database: `%APPDATA%\gg.fraglink.desktop\fraglink.db`
   - Logs: `%LOCALAPPDATA%\gg.fraglink.desktop\logs\fraglink.log`
   - Exports: `Documents\Fraglink\`
10. **Sending feedback/logs:** tray → *Open logs folder*, and send `fraglink.log`. Optionally also tray → *Export my data* and send the JSON/CSV. Mention which games weren't detected or were mis-detected, and what the tray showed. It also helps to note Fraglink's memory/CPU in Task Manager: include both "Fraglink" and the "Microsoft Edge WebView2" entries while the window is open, then again after hiding it to the tray.
11. **Uninstall:** *Settings → Apps → Fraglink → Uninstall*. Your data folder is kept; delete `%APPDATA%\gg.fraglink.desktop` to remove it.

## Installed games (0.2.0-dev.5)

Open *Tools → Options → Games* and press **Detect Games**. Every game installed through Steam, Battle.net, Epic, GOG, EA app, Ubisoft Connect or the Xbox app (default `XboxGames` folder) should be under **Installed games**, with its launcher next to its name. Then:

- Press **Launch** in the info panel (or *Tools → Launch*) for a Battle.net game: Battle.net should open and start it. Try one Steam, Epic, EA and Ubisoft game too.
- If a game is missing, note its launcher and install folder, and send the log folder (*Options → General → Advanced* shows where it is; the `libraries:` line lists how many installs each launcher gave).

## Main window (0.2.0-dev.5)

- First start on a clean profile: "Detect installed games" with a progress bar and Skip, then the friends list. Not shown on the next start.
- Signed out: the Login form is in the main window. Login opens your browser with a code; approve it and your friends appear.
- Hover a friend for about half a second: a tooltip beside the row (username, game, server, IP:port). Right-click a friend: View Profile, Join Game (only on a server), Favorites, Move to Group, Remove Friend…, Block User… (both ask first).
- Status dropdown: set a custom status, then Reset Status; the custom one is listed for reuse.
- Have a friend sign in: a bottom-right "<name> logged on" toast; View More selects them.
- Fold a group, restart: it stays folded. Tools > Launch ends with Detect Games.
- Right-click the tray icon: Launch > (games A–Z, Detect Games), Skin >, Status >, Log Off…, Exit. Launch a game from it; switch skin; pick Away From Keyboard.
- Press + (Add Friend): the search opens inside the main window; Cancel returns to the list.
- Open the info panel with the ◀ tab: it is joined to the window, same height, no gap; moving the window moves both.

## Updates, Away and Options (0.2.0-dev.10)

- Start Fraglink from the Start menu: a small dark "Checking for updates…" window appears, closes after about a second, then the main window opens. With the network off it waits at most about 5 s, then opens normally.
- Windows startup / "Start minimised": no startup window; Fraglink goes straight to the tray.
- When a newer build is on the server: the startup window shows "Downloading update…" with a progress bar, then the installer runs and Fraglink restarts into the new version.
- While running, a new build appears within an hour as a small green arrow left of the minimise button (tooltip "Update ready - click to restart"); clicking it restarts into it. *Help → Check for Updates* and the tray's *Check for Updates* say "You're up to date" or start the update.
- Leave the PC alone for 10 minutes: your status shows "Away" (or "<game> (Away)" while playing), and friends see Away. Move the mouse: back to Online within about 5 s. Pick Invisible first: it stays Invisible.
- Drag the info panel's right frame (between its scrollbar and the window edge): the panel widens; the cursor is a left-right arrow along the whole strip.
- *Options → Games*, select a Steam game: Game path like `E:\Steam\steamapps\common\Satisfactory\` (real casing), Detect path the full path of the exe found. A game with no Xfire icon (e.g. Satisfactory) shows its own icon.
- *Options → Connections → Tracking*: only Export my data and Open data folder; play a game for under a minute: the session is kept.

## Domain, startup and side panel (0.2.0-dev.11)

- dev.10 installed: within an hour (or on the next start) it updates itself to dev.11. Help > About shows 0.2.0-dev.11.
- Start Fraglink: "Checking for updates…", then briefly "Detecting games…", then the main window with your installed games already in the side panel.
- Signed in: friends and live status work as before (the client now uses `https://playfraglink.com`).
- Options > Account: the server shows `https://playfraglink.com` (an old `http://119.42.54.86` setting is moved automatically).
- Friends list: a small gap above each group header; rows a little taller.
- Side panel: Activity Report and Installed Games have blue heading bars with View More / Detect Games on the right. Click a friend, then empty space in the list (or press Esc): back to the default panel.
- **Test build only:** leave the mouse and keyboard alone for 20 seconds: status shows Away (friends see it); move the mouse: back to Online within about 5 s.
