-- Mock both clients and check the one addon tags Retail and Forever correctly.
--   lua5.1 apps/desktop/addon/test_flavours.lua
local timers, handlers = {}, nil
local chat = { registered = true }
function chat:IsEventRegistered() return self.registered end
function chat:UnregisterEvent() self.registered = false end
function chat:RegisterEvent() self.registered = true end
ChatFrame1 = chat
NUM_CHAT_WINDOWS = 1
DEFAULT_CHAT_FRAME = { AddMessage = function() end }
SlashCmdList = {}
C_Timer = {
  After = function(_, fn) timers[#timers + 1] = fn end,
  NewTicker = function() end,
}
function CreateFrame()
  return { RegisterEvent = function() end, SetScript = function(_, _, fn) handlers = fn end }
end
function GetCurrentRegion() return 3 end
function GetRealmName() return "Area 52" end
function UnitClass() return "Mage", "MAGE" end
function UnitLevel() return 80 end
function BNGetInfo() return 1, "Sample#1234" end
function time() return 1793000000 end
function GetBuildInfo() return "12.1.5", "64000", "Oct 13 2026", 120105 end
WOW_PROJECT_MAINLINE = 1
WOW_PROJECT_CAMELOT = 18
WOW_PROJECT_ID = 1
function IsBetaBuild() return false end
function IsPublicTestClient() return false end
function IsTestBuild() return false end
local asked = 0
function RequestTimePlayed() asked = asked + 1 end
local function run_timers()
  local t = timers; timers = {}
  for _, fn in ipairs(t) do fn() end
end
local function fail(msg)
  io.stderr:write(msg .. "\n")
  os.exit(1)
end
local function eq(got, want, msg)
  if got ~= want then fail(msg .. ": got " .. tostring(got) .. " want " .. tostring(want)) end
end

dofile(arg and arg[0]:gsub("test_flavours%.lua$", "") .. "FraglinkPlayed/FraglinkPlayed.lua")

local ids, nextid = {}, 0
local function play(name, total, project, options)
  options = options or {}
  local first, surname = name:match("^(%S+)%s+(.+)$")
  first = first or name
  if project ~= 18 then first, surname = name, "Area 52" end
  UnitName = function() if options.fallback then return first end; return first, surname end
  UnitFullName = options.fallback == "tuple" and function() return first, surname end or nil
  GetUnitName = options.fallback == "full" and function() return name end or nil
  if not ids[name] then nextid = nextid + 1; ids[name] = "Player-70-" .. string.format("%08x", nextid) end
  UnitGUID = function() if options.no_guid then return nil end; return options.guid or ids[name] end
  WOW_PROJECT_ID = project
  timers = {}
  handlers(nil, "PLAYER_ENTERING_WORLD", true, false)
  run_timers()
  handlers(nil, "TIME_PLAYED_MSG", total, 10)
  run_timers()
end

play("Mockrend", 7200, 1)
local retail = FraglinkPlayedDB.chars["eu/Area 52/Mockrend"]
if not retail then fail("retail key missing") end
eq(retail.flavour, "retail", "retail flavour")
eq(retail.project, 1, "retail project")
eq(retail.realm, "Area 52", "retail realm")
eq(retail.confirmed, 1, "retail confirmed")
eq(retail.first, "", "retail has no forever first name")
eq(asked > 0, true, "retail asked for /played")

GetBuildInfo = function() return "1.60.1", "70790", "Nov 4 2026", 16001 end
play("Vale Mocken", 3600, 18)
local function ff(name) return FraglinkPlayedDB.chars["eu/forever/" .. ids[name]] end
local forever = ff("Vale Mocken")
if not forever then fail("forever key missing") end
eq(forever.flavour, "forever", "forever flavour")
eq(forever.project, 18, "forever project")
eq(forever.realm, "", "forever realm blank")
eq(forever.first, "Vale", "forever first")
eq(forever.surname, "Mocken", "forever surname")
eq(forever.beta, 0, "forever not beta")
eq(forever.confirmed, 1, "forever confirmed")
eq(retail.played, 7200, "retail row left alone")

IsBetaBuild = function() return true end
play("Beta Kin", 1000, 18)
eq(ff("Beta Kin").flavour, "forever_beta", "beta flavour")
eq(ff("Beta Kin").beta, 1, "beta flag")

IsBetaBuild, IsPublicTestClient, IsTestBuild = nil, nil, nil
play("Quiet Kin", 1000, 18)
eq(ff("Quiet Kin").flavour, "forever_unconfirmed", "unconfirmed flavour")
eq(ff("Quiet Kin").confirmed, 0, "unconfirmed flag")

IsBetaBuild = function() return false end
IsPublicTestClient = function() return false end
IsTestBuild = function() return false end
play("Vale Mocken", 5000, 18)
eq(ff("Vale Mocken").played, 5000, "higher total kept")
play("Vale Mocken", 4000, 18)
eq(ff("Vale Mocken").played, 5000, "lower total ignored")

-- Tuple APIs expose the surname as their second result, not a realm.
play("Tuple Family", 1000, 18, { fallback = "tuple" })
eq(ff("Tuple Family").surname, "Family", "UnitFullName fallback")
play("Display Family", 1000, 18, { fallback = "full" })
eq(ff("Display Family").surname, "Family", "GetUnitName full-name fallback")
-- Upgrade an actual 1.1.0-shaped slot in place without losing its higher total.
FraglinkPlayedDB.chars["eu/forever/Legacy"] = {
  region = "eu", first = "Legacy", surname = "", name = "Legacy", played = 7000,
}
play("Legacy Family", 6500, 18)
eq(FraglinkPlayedDB.chars["eu/forever/Legacy"].played, 7000, "legacy lower answer ignored")
play("Legacy Family", 7200, 18)
eq(FraglinkPlayedDB.chars["eu/forever/Legacy"].surname, "Family", "legacy surname filled")
eq(FraglinkPlayedDB.chars["eu/forever/Legacy"].guid, ids["Legacy Family"], "legacy GUID filled")
eq(FraglinkPlayedDB.chars["eu/forever/" .. ids["Legacy Family"]], nil, "no duplicate legacy slot")
play("Twin Alpha", 1000, 18)
play("Twin Beta", 2000, 18)
eq(ff("Twin Alpha").surname, "Alpha", "different GUIDs stay separate")
eq(ff("Twin Beta").surname, "Beta", "different GUIDs stay separate")

-- A missing GUID must not let a shared first name overwrite another surname.
play("Noid Alpha", 3000, 18, { no_guid = true })
play("Noid Beta", 4000, 18, { no_guid = true })
eq(FraglinkPlayedDB.chars["eu/forever/Noid"].surname, "Alpha", "unknown GUID surname conflict kept separate")
eq(FraglinkPlayedDB.chars["eu/forever/Noid"].played, 3000, "surname conflict cannot overwrite hours")

local before = FraglinkPlayedDB.chars["eu/Area 52/Mockrend"].played
RequestTimePlayed = nil
C_Timer = nil
local ok = pcall(function()
  handlers(nil, "PLAYER_ENTERING_WORLD", true, false)
  handlers(nil, "PLAYER_LEVEL_UP")
  handlers(nil, "TIME_PLAYED_MSG", 100, 1)
end)
eq(ok, true, "missing timer and request does not error")
eq(FraglinkPlayedDB.chars["eu/Area 52/Mockrend"].played, before, "retail hours unchanged")

io.write("flavours ok\n")

-- Optional cross-language fixture: run with its output path as the first arg.
if arg and arg[1] then
  local file = assert(io.open(arg[1], "w"))
  file:write("FraglinkPlayedDB = { chars = { sample = {\n")
  local keys = {}
  for key in pairs(forever) do keys[#keys + 1] = key end
  table.sort(keys)
  for _, key in ipairs(keys) do
    local value = forever[key]
    file:write("[", string.format("%q", key), "] = ", type(value) == "string" and string.format("%q", value) or tostring(value), ",\n")
  end
  file:write("} } }\n")
  file:close()
end
