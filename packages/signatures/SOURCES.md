# Signature verification notes

In 0.1.1 the 20 entries previously flagged `uncertain` were checked against public sources (26 Sep 2026). **Steam launch config** means the app's public Steam app info (`config.launch` executables and `installdir`), read from the public SteamCMD mirror `https://api.steamcmd.net/v1/info/<appid>`. That is the same data SteamDB shows on an app's *Configuration* tab. Every Steam app id below was also checked against the name in that app info.

Steam copies are detected by install folder anyway. The exe names matter for copies from other launchers (Epic, Battle.net, Xbox, standalone) and give a better name when two games share a folder.

| Game | Result | Evidence |
|---|---|---|
| Deadlock (1422450) | ✅ kept `deadlock.exe`, `project8.exe` | Exe renamed from project8.exe to deadlock.exe in Mar 2025 and both still ship (r/DeadlockTheGame PSA; playdeadlock forum). Steam launch config is token-protected. |
| Fall Guys (1097150) | ✅ kept | Steam launch config `FallGuys_client.exe`; `FallGuys_client_game.exe` is the game process (Epic support article, Heroic/Lutris configs). |
| Minecraft: Java | ✅ improved | Added the Microsoft Store launcher's package folder `Microsoft.4297127D64EC6_8wekyb3d8bbwe` and Mojang's bundled runtime folders (`runtime/java-runtime-*`, `jre-legacy`), plus Modrinth App (Mojira MC-227913, MinecraftHopper). System-wide Java is still not detected. |
| Call of Duty HQ (1938090) | ✅ kept | Steam launch config `bootstrapper.exe` then `cod.exe` (community troubleshooting guides; CoD Extract wiki for `cod25-cod.exe`). |
| Battlefield 6 (2807960) | ✅ kept `bf6.exe` | Steam launch config. |
| DOOM: The Dark Ages (3017860) | ✅ kept | Bethesda support install paths + community reports of `DOOMTheDarkAges.exe`. Steam launches `idTechLauncher.exe` first. |
| Elden Ring Nightreign (2622380) | ✅ kept `nightreign.exe` | PCGamingWiki. Steam launches `start_protected_game.exe` first. |
| Old School RuneScape (1343370) | ✅ kept | Steam launch config `bin/win64/osclient.exe`; PCGamingWiki mentions RuneLite.exe. |
| Albion Online (761890) | ✅ kept | `game\Albion-Online.exe` (Albion forum/community); Steam launch config is the launcher `AlbionLauncher.exe`. |
| Path of Exile 2 (2694490) | ✅ kept | Steam launch config `PathOfExileSteam.exe`; GGG forum: standalone `PathOfExile.exe` (the `_x64` exe is a stub). Path-gated to `/path of exile 2/` because PoE1 uses the same names. |
| Escape from Tarkov (3932890) | ✅ appid added | Steam store appdetails, 2025 Steam release. |
| World of Tanks (1407200) | ✅ appid added | Steam store appdetails. |
| Marvel Rivals (2767030) | ✅ kept + launcher | Official Marvel Rivals FAQ names `Marvel-Win64-Shipping.exe`; Steam launch config `MarvelRivals_Launcher.exe`. |
| THE FINALS (2073850) | ✅ now path-gated | Steam launch config + PCGamingWiki `Discovery.exe`. It's a generic name, so it only counts under `/the finals/`. |
| Hunt: Showdown 1896 (594650) | ✅ kept | `bin\win_x64\HuntGame.exe` (Steam discussions, Crytek post on r/HuntShowdown). Steam launches `hunt.exe` first. |
| Arma Reforger (1874880) | ✅ kept | `ArmaReforgerSteam.exe` (EveronLife docs, Proton issue); Steam launches `ArmaReforger_BE.exe` first. |
| Tekken 8 (1778820) | ✅ kept + `TEKKEN 8.exe` | Steam launch config `TEKKEN 8.exe`; UE process `Polaris-Win64-Shipping` (UEVR profile name, PCGamingWiki). |
| Black Myth: Wukong (2358720) | ✅ kept + `b1.exe` (path-gated) | Steam launch config `b1.exe`; UE process `b1-Win64-Shipping` (UEVR profile name). |
| S.T.A.L.K.E.R. 2 (1643320) | ✅ kept + `Stalker2.exe` | Steam launch config + PCGamingWiki. |
| Kingdom Come: Deliverance II (1771300) | ✅ kept | Steam launch config `Bin\Win64MasterMasterSteamPGO\KingdomCome.exe`. |
| Hollow Knight: Silksong (1030300) | ✅ kept | Steam launch config. |
| Civilization VII (1295660) | 🔧 **fixed** | `CivilizationVII.exe` was the macOS name. Windows runs `Base\Binaries\Win64\Civ7_Win64_DX12_FinalRelease.exe` (or the Vulkan/Release variants), per the Steam launch config. |

None of this came from Discord's detectable-games list. Exe names change with patches, so reports from testers still help: use the bug template.
