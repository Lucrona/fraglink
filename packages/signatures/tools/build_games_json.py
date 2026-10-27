#!/usr/bin/env python3
"""Regenerates ../games.json from the hand-curated table below.

Sources: hand-curated from public knowledge of each game's executable name and
Steam app id; the 0.1.1 review of previously-uncertain rows is documented in
../SOURCES.md. NOT derived from Discord's detectable-games list (see README).
Rows marked U=True are unverified guesses: please confirm on a real install.

Row format: (id, name, exes, path_exes, path_hints, steam_appids, platform, uncertain, note)
"""
import json, pathlib

G = []
def g(id, name, exes=(), appids=(), platform="", U=False, path_exes=(), hints=(), note=None):
    G.append(dict(id=id, name=name, exes=list(exes), path_exes=list(path_exes),
                  path_hints=list(hints), steam_appids=list(appids), platform=platform,
                  uncertain=U, **({"note": note} if note else {})))

# ---- Blizzard / Battle.net
# WoW follows Discord's detectable list as-is (0.2.0-dev.14): World of Warcraft (_retail_ /
# _ptr_ / _beta_ folders), World of Warcraft Classic (WowClassic.exe), World of Warcraft: Forever
# (Wow.exe in the _forever_ folder since its launch; WowB.exe from its _classic_beta_ test days kept
# as a fallback). Retail only claims Wow.exe outside the Forever folders: a path hint beats a plain exe
# name. Public test builds (WowT.exe, WowClassicT.exe) are ignored, not tracked.
g("wow", "World of Warcraft", ["Wow.exe", "Wow-64.exe"], platform="battlenet", hints=["/_retail_/", "/_ptr_/", "/_beta_/"])
g("wow_classic", "World of Warcraft Classic", ["WowClassic.exe", "WowClassic-64.exe"], platform="battlenet")
g("wow_forever", "World of Warcraft: Forever", ["WowB.exe", "WowB-64.exe"], platform="battlenet", path_exes=["Wow.exe", "Wow-64.exe"], hints=["/_forever_/", "/_classic_beta_/"])
g("star_citizen", "Star Citizen", ["StarCitizen.exe"], platform="rsi")
g("overwatch2", "Overwatch 2", ["Overwatch.exe"], [2357570], "battlenet")
g("diablo4", "Diablo IV", ["Diablo IV.exe"], [2344520], "battlenet")
g("diablo3", "Diablo III", ["Diablo III64.exe", "Diablo III.exe"], platform="battlenet")
g("d2r", "Diablo II: Resurrected", ["D2R.exe"], platform="battlenet")
g("diablo2", "Diablo II", ["Diablo II.exe"], platform="standalone", path_exes=["Game.exe"], hints=["/diablo ii/"], note="Classic D2 runs as Game.exe inside the 'Diablo II' folder")
g("hearthstone", "Hearthstone", ["Hearthstone.exe"], platform="battlenet")
g("sc2", "StarCraft II", ["SC2_x64.exe", "SC2.exe"], platform="battlenet")
g("starcraft", "StarCraft: Remastered", ["StarCraft.exe"], platform="battlenet")
g("wc3", "Warcraft III", ["Warcraft III.exe", "war3.exe"], platform="battlenet", note="war3.exe = classic Reign of Chaos / Frozen Throne")
g("hots", "Heroes of the Storm", ["HeroesOfTheStorm_x64.exe", "HeroesOfTheStorm.exe"], platform="battlenet")
# ---- Valve
g("cs2", "Counter-Strike 2", ["cs2.exe"], [730], "steam")
g("cs16", "Counter-Strike 1.6", [], [10], "steam", note="Runs as hl.exe (shared with Half-Life); detected via Steam RunningAppID")
g("css", "Counter-Strike: Source", ["cstrike_win64.exe", "cstrike.exe"], [240], "steam", path_exes=["hl2.exe"], hints=["/counter-strike source/"])
g("half_life", "Half-Life", ["hl.exe"], [70], "steam", note="hl.exe is shared by all GoldSrc games incl. CS 1.6; Steam RunningAppID disambiguates")
g("hl2", "Half-Life 2", [], [220], "steam", path_exes=["hl2.exe", "hl2_win64.exe"], hints=["/half-life 2/hl2", "/half-life 2/bin/win64/hl2"])
g("hl2dm", "Half-Life 2: Deathmatch", ["hl2mp_win64.exe"], [320], "steam", path_exes=["hl2.exe"], hints=["/half-life 2 deathmatch/"])
g("tf2", "Team Fortress 2", ["tf_win64.exe", "tf.exe"], [440], "steam", path_exes=["hl2.exe"], hints=["/team fortress 2/"])
g("portal", "Portal", [], [400], "steam", path_exes=["hl2.exe"], hints=["/common/portal/"])
g("portal2", "Portal 2", ["portal2.exe"], [620], "steam")
g("l4d2", "Left 4 Dead 2", ["left4dead2.exe"], [550], "steam")
g("gmod", "Garry's Mod", ["gmod.exe"], [4000], "steam", path_exes=["hl2.exe"], hints=["/garrysmod/"])
g("dota2", "Dota 2", ["dota2.exe"], [570], "steam")
g("deadlock", "Deadlock", ["deadlock.exe", "project8.exe"], [1422450], "steam")  # verified: exe renamed project8.exe -> deadlock.exe (Mar 2025), both still shipped; see SOURCES.md
# ---- Riot
g("lol", "League of Legends", ["League of Legends.exe"], platform="riot", note="Counts the in-match client only; the lobby (LeagueClientUx.exe) is ignored")
g("valorant", "VALORANT", ["VALORANT-Win64-Shipping.exe"], platform="riot")
# ---- Epic
g("fortnite", "Fortnite", ["FortniteClient-Win64-Shipping.exe"], platform="epic")
g("rocketleague", "Rocket League", ["RocketLeague.exe"], [252950], "epic")
g("fallguys", "Fall Guys", ["FallGuys_client_game.exe", "FallGuys_client.exe"], [1097150], "epic")  # verified: Steam launch config + Epic/community docs
# ---- Microsoft / Mojang
g("minecraft_java", "Minecraft: Java Edition", [], platform="standalone", path_exes=["javaw.exe", "java.exe"], hints=["/.minecraft/", "/minecraft launcher/", "/microsoft.4297127d64ec6_8wekyb3d8bbwe/", "/runtime/java-runtime-", "/runtime/jre-legacy/", "/prismlauncher/", "/multimc/", "/curseforge/minecraft/", "/modrinthapp/"], note="Java edition runs inside javaw.exe; counted when the Java runtime is Mojang's bundled runtime or lives in a launcher folder. A system-wide Java (e.g. Program Files\\Java) is not detected: tag it manually.")
g("minecraft_bedrock", "Minecraft (Bedrock)", ["Minecraft.Windows.exe"], platform="msstore")
g("halo_mcc", "Halo: The Master Chief Collection", ["MCC-Win64-Shipping.exe", "MCCWinStore-Win64-Shipping.exe"], [976730], "steam")
g("halo_infinite", "Halo Infinite", ["HaloInfinite.exe"], [1240440], "steam")
g("forza_h5", "Forza Horizon 5", ["ForzaHorizon5.exe"], [1551360], "steam")
g("forza_h4", "Forza Horizon 4", ["ForzaHorizon4.exe"], [1293830], "steam")
g("sea_of_thieves", "Sea of Thieves", ["SoTGame.exe"], [1172620], "steam")
g("aoe2de", "Age of Empires II: Definitive Edition", ["AoE2DE_s.exe"], [813780], "steam")
g("aoe2", "Age of Empires II (2000)", ["age2_x1.exe", "empires2.exe"], platform="standalone")
g("aoe4", "Age of Empires IV", ["RelicCardinal.exe"], [1466860], "steam")
# ---- Activision
g("cod", "Call of Duty (HQ / Warzone / Black Ops 6+)", ["cod.exe", "cod24-cod.exe", "cod25-cod.exe"], [1938090], "battlenet", note="Call of Duty HQ runs cod.exe (Steam launches bootstrapper.exe first); some betas use codNN-cod.exe")
g("cod_mw2019", "Call of Duty: Modern Warfare (2019)", ["ModernWarfare.exe"], platform="battlenet")
g("cod_bocw", "Call of Duty: Black Ops Cold War", ["BlackOpsColdWar.exe"], platform="battlenet")
g("cod4", "Call of Duty 4: Modern Warfare", ["iw3mp.exe", "iw3sp.exe"], [7940], "steam")
g("cod_mw2_2009", "Call of Duty: Modern Warfare 2 (2009)", ["iw4mp.exe", "iw4sp.exe"], [10180, 10190], "steam")
g("cod_waw", "Call of Duty: World at War", ["CoDWaW.exe", "CoDWaWmp.exe"], [10090], "steam")
g("cod_bo1", "Call of Duty: Black Ops", ["BlackOps.exe", "BlackOpsMP.exe"], [42700, 42710], "steam")
g("cod2", "Call of Duty 2", ["CoD2MP_s.exe", "CoD2SP_s.exe"], [2630], "steam")
# ---- EA
g("bf2042", "Battlefield 2042", ["BF2042.exe"], [1517290], "ea")
g("bf6", "Battlefield 6", ["bf6.exe"], [2807960], "ea")  # verified: Steam launch config
g("bf1", "Battlefield 1", ["bf1.exe"], [1238840], "ea")
g("bfv", "Battlefield V", ["bfv.exe"], [1238810], "ea")
g("bf4", "Battlefield 4", ["bf4.exe"], [1238860], "ea")
g("bf3", "Battlefield 3", ["bf3.exe"], [1238820], "ea")
g("bfbc2", "Battlefield: Bad Company 2", ["BFBC2Game.exe"], [24960], "ea")
g("bf2", "Battlefield 2", ["BF2.exe"], platform="standalone")
g("bf1942", "Battlefield 1942", ["BF1942.exe"], platform="standalone")
g("bfvietnam", "Battlefield Vietnam", ["BfVietnam.exe"], platform="standalone")
g("apex", "Apex Legends", ["r5apex.exe", "r5apex_dx12.exe"], [1172470], "ea")
g("titanfall2", "Titanfall 2", ["Titanfall2.exe"], [1237970], "ea")
g("sims4", "The Sims 4", ["TS4_x64.exe"], [1222670], "ea")
# ---- Rockstar / CDPR / Bethesda
g("gta5", "Grand Theft Auto V", ["GTA5.exe", "GTA5_Enhanced.exe"], [271590, 3240220], "rockstar")
g("rdr2", "Red Dead Redemption 2", ["RDR2.exe"], [1174180], "rockstar")
g("cyberpunk", "Cyberpunk 2077", ["Cyberpunk2077.exe"], [1091500], "steam")
g("witcher3", "The Witcher 3: Wild Hunt", ["witcher3.exe"], [292030], "steam")
g("skyrim_se", "The Elder Scrolls V: Skyrim Special Edition", ["SkyrimSE.exe"], [489830], "steam")
g("skyrim", "The Elder Scrolls V: Skyrim", ["TESV.exe"], [72850], "steam")
g("fallout4", "Fallout 4", ["Fallout4.exe"], [377160], "steam")
g("fallout76", "Fallout 76", ["Fallout76.exe"], [1151340], "steam")
g("falloutnv", "Fallout: New Vegas", ["FalloutNV.exe"], [22380], "steam")
g("starfield", "Starfield", ["Starfield.exe"], [1716740], "steam")
g("eso", "The Elder Scrolls Online", ["eso64.exe"], [306130], "steam")
g("doom_eternal", "DOOM Eternal", ["DOOMEternalx64vk.exe"], [782330], "steam")
g("doom_tda", "DOOM: The Dark Ages", ["DOOMTheDarkAges.exe"], [3017860], "steam")  # verified: Bethesda support + community (Steam launches idTechLauncher.exe first)
g("quake3", "Quake III Arena", ["quake3.exe"], [2200], "steam")
g("quakelive", "Quake Live", ["quakelive_steam.exe"], [282440], "steam")
# ---- FromSoftware
g("eldenring", "Elden Ring", ["eldenring.exe"], [1245620], "steam")
g("nightreign", "Elden Ring Nightreign", ["nightreign.exe"], [2622380], "steam")  # verified: PCGamingWiki (Steam launches start_protected_game.exe)
g("ds3", "Dark Souls III", ["DarkSoulsIII.exe"], [374320], "steam")
g("sekiro", "Sekiro: Shadows Die Twice", ["sekiro.exe"], [814380], "steam")
# ---- MMOs
g("ffxiv", "Final Fantasy XIV", ["ffxiv_dx11.exe"], [39210], "square")
g("gw2", "Guild Wars 2", ["Gw2-64.exe", "Gw2.exe"], [1284210], "arenanet")
g("osrs", "Old School RuneScape", ["osclient.exe", "RuneLite.exe"], [1343370], "jagex", note="osclient.exe = official client (Steam launch config); RuneLite.exe = popular third-party client (PCGamingWiki)")
g("rs3", "RuneScape 3", ["rs2client.exe"], [1343400], "jagex")
g("lostark", "Lost Ark", ["LOSTARK.exe"], [1599340], "steam")
g("newworld", "New World: Aeternum", ["NewWorld.exe"], [1063730], "steam")
g("swtor", "Star Wars: The Old Republic", ["swtor.exe"], [1286830], "steam")
g("eve", "EVE Online", ["exefile.exe"], [8500], "ccp")
g("albion", "Albion Online", ["Albion-Online.exe"], [761890], "steam")  # verified: game\\Albion-Online.exe (launcher is AlbionLauncher.exe)
g("bdo", "Black Desert", ["BlackDesert64.exe"], [582660], "steam")
g("everquest", "EverQuest", ["eqgame.exe"], [205710], "daybreak")
g("everquest2", "EverQuest II", ["EverQuest2.exe"], [201230], "daybreak")
g("warframe", "Warframe", ["Warframe.x64.exe"], [230410], "steam")
g("destiny2", "Destiny 2", ["destiny2.exe"], [1085660], "steam")
g("poe", "Path of Exile", ["PathOfExile.exe", "PathOfExile_x64.exe", "PathOfExileSteam.exe", "PathOfExile_x64Steam.exe"], [238960], "ggg")
g("poe2", "Path of Exile 2", [], [2694490], "ggg", path_exes=["PathOfExile.exe", "PathOfExile_x64.exe", "PathOfExileSteam.exe", "PathOfExile_x64Steam.exe"], hints=["/path of exile 2/"])  # verified: Steam launch config (PathOfExileSteam.exe) + GGG forum (standalone PathOfExile.exe)
g("lastepoch", "Last Epoch", ["Last Epoch.exe"], [899770], "steam")
g("genshin", "Genshin Impact", ["GenshinImpact.exe", "YuanShen.exe"], platform="hoyoverse")
g("hsr", "Honkai: Star Rail", ["StarRail.exe"], platform="hoyoverse")
g("zzz", "Zenless Zone Zero", ["ZenlessZoneZero.exe"], platform="hoyoverse")
# ---- Shooters / multiplayer
g("pubg", "PUBG: Battlegrounds", ["TslGame.exe"], [578080], "steam")
g("r6siege", "Tom Clancy's Rainbow Six Siege", ["RainbowSix.exe", "RainbowSix_Vulkan.exe", "RainbowSix_DX11.exe"], [359550], "ubisoft")
g("tarkov", "Escape from Tarkov", ["EscapeFromTarkov.exe"], [3932890], "bsg")  # verified: Steam store appdetails 3932890 (2025 Steam release)
g("marvel_rivals", "Marvel Rivals", ["Marvel-Win64-Shipping.exe", "MarvelRivals_Launcher.exe"], [2767030], "steam")  # verified: official FAQ + Steam launch config
g("thefinals", "THE FINALS", [], [2073850], "steam", path_exes=["Discovery.exe"], hints=["/the finals/"])  # verified: Steam launch config + PCGamingWiki; generic exe name so path-gated
g("helldivers2", "Helldivers 2", ["helldivers2.exe"], [553850], "steam")
g("hunt", "Hunt: Showdown 1896", ["HuntGame.exe"], [594650], "steam")  # verified: bin\\win_x64\\HuntGame.exe (Steam launches hunt.exe first)
g("squad", "Squad", ["SquadGame.exe"], [393380], "steam")
g("arma3", "Arma 3", ["arma3_x64.exe", "arma3.exe"], [107410], "steam")
g("arma_reforger", "Arma Reforger", ["ArmaReforgerSteam.exe"], [1874880], "steam")  # verified: Bohemia wiki / Proton reports (Steam launches ArmaReforger_BE.exe first)
g("dayz", "DayZ", ["DayZ_x64.exe"], [221100], "steam")
g("rust", "Rust", ["RustClient.exe"], [252490], "steam")
g("dbd", "Dead by Daylight", ["DeadByDaylight-Win64-Shipping.exe"], [381210], "steam")
g("chivalry2", "Chivalry 2", ["Chivalry2-Win64-Shipping.exe"], [1824220], "steam")
g("warthunder", "War Thunder", ["aces.exe"], [236390], "gaijin")
g("wot", "World of Tanks", ["WorldOfTanks.exe"], [1407200], "wargaming")  # verified: Steam store appdetails 1407200
g("payday2", "PAYDAY 2", ["payday2_win32_release.exe"], [218620], "steam")
g("ut2004", "Unreal Tournament 2004", ["UT2004.exe"], [13230], "steam")
g("tekken8", "Tekken 8", ["Polaris-Win64-Shipping.exe", "TEKKEN 8.exe"], [1778820], "steam")  # verified: Steam launch config + UEVR profile name
g("sf6", "Street Fighter 6", ["StreetFighter6.exe"], [1364780], "steam")
g("amongus", "Among Us", ["Among Us.exe"], [945360], "steam")
g("roblox", "Roblox", ["RobloxPlayerBeta.exe"], platform="roblox")
g("osu", "osu!", ["osu!.exe"], platform="standalone")
# ---- Co-op / survival / sandbox
g("drg", "Deep Rock Galactic", ["FSD-Win64-Shipping.exe"], [548430], "steam")
g("valheim", "Valheim", ["valheim.exe"], [892970], "steam")
g("terraria", "Terraria", ["Terraria.exe"], [105600], "steam")
g("stardew", "Stardew Valley", ["Stardew Valley.exe", "StardewModdingAPI.exe"], [413150], "steam")
g("palworld", "Palworld", ["Palworld-Win64-Shipping.exe"], [1623730], "steam")
g("lethal_company", "Lethal Company", ["Lethal Company.exe"], [1966720], "steam")
g("phasmophobia", "Phasmophobia", ["Phasmophobia.exe"], [739630], "steam")
g("sons_forest", "Sons of the Forest", ["SonsOfTheForest.exe"], [1326470], "steam")
g("7dtd", "7 Days to Die", ["7DaysToDie.exe"], [251570], "steam")
g("zomboid", "Project Zomboid", ["ProjectZomboid64.exe"], [108600], "steam")
g("enshrouded", "Enshrouded", ["enshrouded.exe"], [1203620], "steam")
g("vrising", "V Rising", ["VRising.exe"], [1604030], "steam")
g("ark_asa", "ARK: Survival Ascended", ["ArkAscended.exe"], [2399830], "steam")
g("ark_se", "ARK: Survival Evolved", [], [346110], "steam", path_exes=["ShooterGame.exe"], hints=["/ark/shootergame/"])
g("subnautica", "Subnautica", ["Subnautica.exe"], [264710], "steam")
g("dst", "Don't Starve Together", ["dontstarve_steam_x64.exe", "dontstarve_steam.exe"], [322330], "steam")
g("nms", "No Man's Sky", ["NMS.exe"], [275850], "steam")
g("factorio", "Factorio", ["factorio.exe"], [427520], "steam")
g("satisfactory", "Satisfactory", ["FactoryGame-Win64-Shipping.exe", "FactoryGameSteam-Win64-Shipping.exe", "FactoryGameEGS-Win64-Shipping.exe"], [526870], "steam")
g("rimworld", "RimWorld", ["RimWorldWin64.exe"], [294100], "steam")
g("ets2", "Euro Truck Simulator 2", ["eurotrucks2.exe"], [227300], "steam")
g("cities2", "Cities: Skylines II", ["Cities2.exe"], [949230], "steam")
g("cities", "Cities: Skylines", ["Cities.exe"], [255710], "steam")
g("mh_wilds", "Monster Hunter Wilds", ["MonsterHunterWilds.exe"], [2246340], "steam")
g("mh_world", "Monster Hunter: World", ["MonsterHunterWorld.exe"], [582010], "steam")
g("mh_rise", "Monster Hunter Rise", ["MonsterHunterRise.exe"], [1446780], "steam")
# ---- RPG / action / indie
g("bg3", "Baldur's Gate 3", ["bg3.exe", "bg3_dx11.exe"], [1086940], "steam")
g("wukong", "Black Myth: Wukong", ["b1-Win64-Shipping.exe"], [2358720], "steam", path_exes=["b1.exe"], hints=["/blackmythwukong/", "/black myth wukong/"])  # verified: Steam launch config (b1.exe) + UEVR profile name
g("stalker2", "S.T.A.L.K.E.R. 2", ["Stalker2-Win64-Shipping.exe", "Stalker2.exe"], [1643320], "steam")  # verified: Steam launch config + PCGamingWiki
g("kcd2", "Kingdom Come: Deliverance II", ["KingdomCome.exe"], [1771300], "steam")  # verified: Steam launch config (Bin\\Win64MasterMasterSteamPGO\\KingdomCome.exe)
g("borderlands3", "Borderlands 3", ["Borderlands3.exe"], [397540], "steam")
g("hades2", "Hades II", ["Hades2.exe"], [1145350], "steam")
g("hades", "Hades", ["Hades.exe"], [1145360], "steam")
g("hollow_knight", "Hollow Knight", ["hollow_knight.exe"], [367520], "steam")
g("silksong", "Hollow Knight: Silksong", ["Hollow Knight Silksong.exe"], [1030300], "steam")  # verified: Steam launch config
g("ror2", "Risk of Rain 2", ["Risk of Rain 2.exe"], [632360], "steam")
g("vampire_survivors", "Vampire Survivors", ["VampireSurvivors.exe"], [1794680], "steam")
g("balatro", "Balatro", ["Balatro.exe"], [2379780], "steam")
g("bannerlord", "Mount & Blade II: Bannerlord", ["Bannerlord.exe"], [261550], "steam")
# ---- Strategy
g("civ6", "Sid Meier's Civilization VI", ["CivilizationVI.exe", "CivilizationVI_DX12.exe"], [289070], "steam")
g("civ7", "Sid Meier's Civilization VII", ["Civ7_Win64_DX12_FinalRelease.exe", "Civ7_Win64_Vulkan_FinalRelease.exe", "Civ7_Win64_DX12_Release.exe"], [1295660], "steam")  # FIXED (was CivilizationVII.exe, the macOS name); verified: Steam launch config
g("tww3", "Total War: WARHAMMER III", ["Warhammer3.exe"], [1142710], "steam")
g("stellaris", "Stellaris", ["stellaris.exe"], [281990], "steam")
g("ck3", "Crusader Kings III", ["ck3.exe"], [1158310], "steam")
g("hoi4", "Hearts of Iron IV", ["hoi4.exe"], [394360], "steam")
g("eu4", "Europa Universalis IV", ["eu4.exe"], [236850], "steam")

G = [x for x in G if x and x["name"]]

IGNORE = [
    "WowT.exe", "WowT-64.exe", "WowClassicT.exe", "WowClassicT-64.exe",
  # launchers / clients / overlays
  "steam.exe", "steamwebhelper.exe", "steamservice.exe", "gameoverlayui.exe", "gameoverlayui64.exe",
  "EpicGamesLauncher.exe", "EpicWebHelper.exe", "EOSOverlayRenderer-Win64-Shipping.exe",
  "Battle.net.exe", "Agent.exe", "BlizzardError.exe", "Blizzard Battle.net.exe",
  "RiotClientServices.exe", "RiotClientUx.exe", "RiotClientUxRender.exe", "RiotClientCrashHandler.exe",
  "LeagueClient.exe", "LeagueClientUx.exe", "LeagueClientUxRender.exe", "LeagueCrashHandler64.exe",
  "EADesktop.exe", "EABackgroundService.exe", "EALocalHostSvc.exe", "Origin.exe", "OriginWebHelperService.exe",
  "GalaxyClient.exe", "GalaxyClientService.exe", "GOG Galaxy Notifications Renderer.exe",
  "UbisoftConnect.exe", "upc.exe", "UplayWebCore.exe", "UbisoftGameLauncher.exe", "UbisoftGameLauncher64.exe",
  "RockstarService.exe", "LauncherPatcher.exe", "SocialClubHelper.exe", "PlayGTAV.exe",
  "XboxPcApp.exe", "XboxPcAppFT.exe", "GamingServices.exe", "GamingServicesNet.exe",
  "Discord.exe", "overwolf.exe", "MinecraftLauncher.exe", "Minecraft.exe", "JagexLauncher.exe",
  # anti-cheat
  "EasyAntiCheat.exe", "EasyAntiCheat_EOS.exe", "EasyAntiCheat_launcher.exe", "EasyAntiCheat_EOS_Setup.exe",
  "BEService.exe", "BEService_x64.exe", "BEClient.exe", "vgc.exe", "vgtray.exe", "PnkBstrA.exe", "PnkBstrB.exe",
  "EAAntiCheat.GameService.exe", "EAAntiCheat.GameServiceLauncher.exe", "mhyprot.exe", "faceit.exe", "faceitservice.exe",
  "DayZ_BE.exe", "RainbowSix_BE.exe", "GTA5_BE.exe",
  # crash handlers / redistributables / installers
  "UnityCrashHandler64.exe", "UnityCrashHandler32.exe", "CrashReportClient.exe", "crashpad_handler.exe",
  "CrashSender1403.exe", "BsSndRpt.exe", "BsSndRpt64.exe", "werfault.exe",
  "vcredist_x64.exe", "vcredist_x86.exe", "VC_redist.x64.exe", "VC_redist.x86.exe", "DXSETUP.exe",
  "UE4PrereqSetup_x64.exe", "UEPrereqSetup_x64.exe", "dotNetFx40_Full_setup.exe", "oalinst.exe",
]
NON_GAME = [
  "explorer.exe", "svchost.exe", "chrome.exe", "msedge.exe", "msedgewebview2.exe", "firefox.exe", "brave.exe", "opera.exe",
  "spotify.exe", "code.exe", "devenv.exe", "notepad.exe", "notepad++.exe", "slack.exe", "teams.exe", "ms-teams.exe",
  "outlook.exe", "winword.exe", "excel.exe", "powerpnt.exe", "onedrive.exe", "obs64.exe", "obs32.exe",
  "taskmgr.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "windowsterminal.exe", "conhost.exe", "openconsole.exe",
  "searchhost.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe", "textinputhost.exe", "runtimebroker.exe",
  "dwm.exe", "ctfmon.exe", "sihost.exe", "taskhostw.exe", "applicationframehost.exe", "systemsettings.exe",
  "lockapp.exe", "widgets.exe", "phoneexperiencehost.exe", "securityhealthsystray.exe", "rtkaudusservice64.exe",
  "nvcontainer.exe", "nvidia share.exe", "nvidia overlay.exe", "nvidia app.exe", "radeonsoftware.exe", "amdrsserv.exe",
  "logioptionsplus_agent.exe", "lghub.exe", "lghub_agent.exe", "icue.exe", "razer synapse 3.exe", "steelseriesgg.exe",
  "wallpaper64.exe", "7zfm.exe", "vlc.exe", "fraglink.exe", "fraglink-desktop.exe", "msiafterburner.exe", "rtss.exe",
  "whatsapp.exe", "telegram.exe", "signal.exe", "zoom.exe", "dropbox.exe", "googledrivefs.exe", "1password.exe",
  "bash", "zsh", "sh", "systemd", "sshd", "node", "cargo", "rustc", "python3", "Xorg", "Xvfb",
]

out = dict(schema=1, version="2026.10.05.1",
           note="Fraglink seed signatures. Hand-curated exe names + Steam app ids only; uncertain=true means unverified. Not derived from Discord's detectable list.",
           ignore_exes=sorted(set(IGNORE), key=str.lower), non_game_exes=sorted(set(NON_GAME), key=str.lower), games=G)
p = pathlib.Path(__file__).resolve().parent.parent / "games.json"
p.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
print(f"wrote {p} with {len(G)} games ({sum(1 for x in G if x['uncertain'])} uncertain)")
