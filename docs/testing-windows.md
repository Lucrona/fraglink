# Testing Fraglink on Windows

## Current beta (0.3.0-beta)

1. **Install:** download the installer from [playfraglink.com/download](https://playfraglink.com/download) (or the latest [GitHub release](https://github.com/Lucrona/fraglink/releases)) and run it. It installs for your user only, with no questions and no admin prompt. The installer isn't code-signed yet, so SmartScreen may say "Windows protected your PC": click **More info → Run anyway**.
2. **Sign in or Create Account** in the main window (the app always uses playfraglink.com). Hours only count while you're signed in and connected.
3. **Play something** (Steam, Battle.net, Epic, GOG, EA, Ubisoft, Xbox, RSI or a standalone game). Your status shows the game, friends see it, and the hours appear on your profile on the website.
4. **Unknown game?** Fraglink asks "Playing X? Add it!", or use *Options → Games → Not seeing your game? Add it!*.
5. **Friends:** add a friend with **+**, check the friend toasts, the side panel, Away after 10 minutes without input, and Invisible.
6. **Updates:** the app checks at startup and every hour. *Help → Check for Updates* updates on demand.
7. **WoW /played (optional):** link Battle.net under *Options → Connections*, install the Fraglink addon with **Get the Fraglink addon**, log in to each character once, then upload on the website.
8. **Logs and data:** the data and log folders are shown in *Options* (About). If the app crashed, the `crash-<time>.txt` file in the log folder helps us find the cause. Report bugs on the [Discord](https://discord.gg/QbaYRtvADj) or with the [contact form](https://playfraglink.com/contact).

## Older test builds

The sections below are the checklists used for earlier builds. They are kept for reference; some steps (the Stats and Activity tabs, the Server field, local sessions) no longer apply.

### M1 (0.1.0)

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

### Installed games (0.2.0-dev.5)

Open *Tools → Options → Games* and press **Detect Games**. Every game installed through Steam, Battle.net, Epic, GOG, EA app, Ubisoft Connect or the Xbox app (default `XboxGames` folder) should be under **Installed games**, with its launcher next to its name. Then:

- Press **Launch** in the info panel (or *Tools → Launch*) for a Battle.net game: Battle.net should open and start it. Try one Steam, Epic, EA and Ubisoft game too.
- If a game is missing, note its launcher and install folder, and send the log folder (*Options → General → Advanced* shows where it is; the `libraries:` line lists how many installs each launcher gave).

### Main window (0.2.0-dev.5)

- First start on a clean profile: "Detect installed games" with a progress bar and Skip, then the friends list. Not shown on the next start.
- Signed out: the Login form is in the main window. Login opens your browser with a code; approve it and your friends appear.
- Hover a friend for about half a second: a tooltip beside the row (username, game, server, IP:port). Right-click a friend: View Profile, Join Game (only on a server), Favorites, Move to Group, Remove Friend…, Block User… (both ask first).
- Status dropdown: set a custom status, then Reset Status; the custom one is listed for reuse.
- Have a friend sign in: a bottom-right "<name> logged on" toast; View More selects them.
- Fold a group, restart: it stays folded. Tools > Launch ends with Detect Games.
- Right-click the tray icon: Launch > (games A–Z, Detect Games), Skin >, Status >, Log Off…, Exit. Launch a game from it; switch skin; pick Away From Keyboard.
- Press + (Add Friend): the search opens inside the main window; Cancel returns to the list.
- Open the info panel with the ◀ tab: it is joined to the window, same height, no gap; moving the window moves both.

### Updates, Away and Options (0.2.0-dev.10)

- Start Fraglink from the Start menu: a small dark "Checking for updates…" window appears, closes after about a second, then the main window opens. With the network off it waits at most about 5 s, then opens normally.
- Windows startup / "Start minimised": no startup window; Fraglink goes straight to the tray.
- When a newer build is on the server: the startup window shows "Downloading update…" with a progress bar, then the installer runs and Fraglink restarts into the new version.
- While running, a new build appears within an hour as a small green arrow left of the minimise button (tooltip "Update ready - click to restart"); clicking it restarts into it. *Help → Check for Updates* and the tray's *Check for Updates* say "You're up to date" or start the update.
- Leave the PC alone for 10 minutes: your status shows "Away" (or "<game> (Away)" while playing), and friends see Away. Move the mouse: back to Online within about 5 s. Pick Invisible first: it stays Invisible.
- Drag the info panel's right frame (between its scrollbar and the window edge): the panel widens; the cursor is a left-right arrow along the whole strip.
- *Options → Games*, select a Steam game: Game path like `E:\Steam\steamapps\common\Satisfactory\` (real casing), Detect path the full path of the exe found. Satisfactory shows its official icon.
- *Options → Connections → Tracking*: only Export my data and Open data folder; play a game for under a minute: the session is kept.

### Domain, startup and side panel (0.2.0-dev.11)

- dev.10 installed: within an hour (or on the next start) it updates itself to dev.11. Help > About shows 0.2.0-dev.11.
- Start Fraglink: "Checking for updates…", then briefly "Detecting games…", then the main window with your installed games already in the side panel.
- Signed in: friends and live status work as before (the client now uses `https://playfraglink.com`).
- Options > Account: the server shows `https://playfraglink.com` (an old server address setting is moved automatically).
- Friends list: a small gap above each group header; rows a little taller.
- Side panel: Activity Report and Installed Games have blue heading bars with View More / Detect Games on the right. Click a friend, then empty space in the list (or press Esc): back to the default panel.
- **Test build only (dev.11):** leave the mouse and keyboard alone for 20 seconds: status shows Away (friends see it); move the mouse: back to Online within about 5 s.

### Detection, WoW realm and the "Track as a game?" question (0.2.0-dev.12)

- dev.11 installed: it updates itself to dev.12. Help > About shows 0.2.0-dev.12.
- WoW retail (`...\World of Warcraft\_retail_\Wow.exe`): status shows **World of Warcraft · <your realm>**; a friend sees the same in the friends list, tooltip and side panel. Switch realm and log out to the character screen (WoW rewrites `WTF\Config.wtf`): the realm updates within a few seconds.
- WoW: Forever (`...\_classic_beta_\WowB.exe`): **World of Warcraft: Forever · <realm>**. Its hours are separate from retail.
- Star Citizen (`...\StarCitizen\LIVE\Bin64\StarCitizen.exe`, also PTU): **Star Citizen**.
- A game that isn't one of Fraglink's own but is on the downloaded games list (e.g. something small from itch.io): detected by name.
- An unlisted program full screen (or borderless) in front for 5 minutes: "Track <name> as a game?" pops up bottom-right and waits as a bar in the main window. **Yes**: it's tracked from now on. **Not a game**: never asked again. A browser or video player full screen is never asked about. For a quicker test start Fraglink with `FRAGLINK_PROMPT_SECS=30`.
- Hover a friend: the tooltip is dark with light text.
- Leave the mouse and keyboard alone for 10 minutes: Away; move the mouse: Online again.
- Offline (no network) start: games are still detected (bundled list); the log says the server wasn't reachable.

### WoW realm placement (0.2.0-dev.13)

- dev.12 installed: it updates itself to dev.13. Help > About shows 0.2.0-dev.13.
- Friend playing WoW (retail or Forever): the friends list row and their status show just **World of Warcraft** (no realm). Hover them: the tooltip says **Playing: World of Warcraft · <realm>**. Click them: the side panel shows **World of Warcraft · <realm>**.
- You playing WoW: your own status box shows **Playing World of Warcraft** without the realm.
- Everything else from the dev.12 list still applies.

### Live-only hours, server names, WoW list (0.2.0-dev.14)

- dev.13 installed: it updates itself to dev.14. Help > About shows 0.2.0-dev.14.
- Signed in and connected, start a game: it shows under Now Playing and friends see it. Quit it: the hours appear in **My Account** right away (no upload delay).
- Pull the network cable (or sign out) while a game runs: Now Playing says **Offline · hours count again once reconnected** (signed out: **Not tracking · sign in to count hours**) and no time is added. Reconnect: tracking resumes within a few seconds.
- Tag an unlisted program ("Track this as a game?" > Yes, or Fraglink > This is a game…) and run it: you see your own name for it; a friend sees **Playing a game** in the list, tooltip, side panel and Activity Report, and on the website. After `fraglink-server approve-game <exe> "<Name>"` on the server, restart the game: friends see the approved name and its earlier hours show under it.
- Options > Privacy > "Show my game to my friends" off: friends see you Online with no game; your hours still go up.
- WoW: retail shows **World of Warcraft**, Classic (`_classic_`, `_classic_era_`, anniversary) shows **World of Warcraft Classic**, `WowB.exe` shows **World of Warcraft: Forever**. A PTR build (`WowT.exe`) isn't tracked. The realm still shows in the friend tooltip and side panel only.

### Official game icons (0.2.0-dev.15)

- dev.14 installed: it updates itself to dev.15. Help > About shows 0.2.0-dev.15.
- Friends list, Now Playing, the friend tooltip and side panel: icons are the games' official icons, the same size and position as before (no blurry or stretched ones).
- Check World of Warcraft, World of Warcraft Classic, World of Warcraft: Forever, Marvel Rivals, Satisfactory, Gray Zone Warfare and Star Citizen (yours or a friend's).
- A friend's game you don't have installed still shows its icon (it comes from the Fraglink server).
- A program you tagged yourself ("This is a game…") shows the icon from its exe; anything without one shows the generic game symbol.
- `%APPDATA%\gg.fraglink.desktop\icons`: old icon files are cleared on the first start; official icons are cached as `sv_*.png`.

### One-click installer, announcements, games list (0.2.0-dev.16)

- dev.15 installed: it updates itself to dev.16 (startup or Help > Check for Updates). The update window is the dark Fraglink window saying **Updating Fraglink…**, then Fraglink restarts by itself. Help > About shows 0.2.0-dev.16.
- Fresh install (Apps & features > Fraglink > Uninstall first, or another Windows user): run `Fraglink_0.2.0-16_x64-setup.exe`. After SmartScreen (*More info > Run anyway*) there are **no questions and no UAC prompt**: a small borderless dark window (logo, "PLAY. CONNECT. TRACK.", **Installing Fraglink…**, blue progress bar on a dark track) closes after a few seconds and Fraglink opens at its sign-in screen.
- Check: `%LOCALAPPDATA%\Fraglink\fraglink.exe` exists; **Fraglink** shortcuts on the Desktop and in Start > Fraglink; Apps & features lists Fraglink 0.2.0-16 (publisher Lucrona).
- At 125 %, 150 % and 200 % display scaling the window is sharp and proportionally bigger (not blurry or cut off).
- Run the setup again while Fraglink is running: it closes Fraglink, updates in place without asking, and starts it again.
- Uninstall from Apps & features: the usual uninstaller page (with "Delete the application data") removes the app and both shortcuts.
- `Fraglink_0.2.0-16_x64-setup.exe /S` installs silently (no window, Fraglink is not started).
- Announcement: with an active banner on the website (Admin > Announcements), a signed-in app shows it once as a bottom-right notification within ~20 s of start (or within 15 min); clicking it opens the link if there is one. It doesn't come back after a restart.
- Options > Games: the tree shows only **Installed games** (exe on disk now, added-by-you games included); there is no "Played before" or "Not installed" section. The sort menu (A–Z / Most played / Recently played) reorders it and is remembered after closing and reopening Options.
- Scan for games: about 45 s after the first start of dev.16 the log (`%APPDATA%\gg.fraglink.desktop\logs`) shows `folder scan: … folders in …s` and `folder scan: … games`; the PC stays responsive meanwhile. Games outside Steam/Epic/GOG/Battle.net/EA/Ubisoft/Xbox (e.g. a standalone game in `D:\Games`, `E:\Battle.net Games`) appear under Installed games with the source "found in your game folders" and can be launched. Press **Scan for games**: the message says "Scanning your game folders…", then "Found N installed games". Launchers, PTR / Public Test builds and uninstallers are not listed.
- Star Citizen installed with the RSI Launcher (any drive/folder): it appears under Installed games with "RSI Launcher"; Launch opens the RSI Launcher. Starting Star Citizen (LIVE or PTU) still shows "Playing Star Citizen". Its icon is the white Star Citizen emblem (laurel wreath) on dark blue, not the old power symbol.
- Under the games tree: "Not seeing a game? Launch it once, or click Scan for games." The link starts a scan.

