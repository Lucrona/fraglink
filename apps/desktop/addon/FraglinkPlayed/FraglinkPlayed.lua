-- Fraglink /played: asks the game for this character's /played total (quietly, no
-- chat line) on login, on level-up and every 10 minutes, and keeps it in
-- FraglinkPlayedDB. The game writes that file on logout / reload; the Fraglink app
-- reads it after you close WoW. Nothing is sent anywhere from inside the game.
--
-- Each entry carries a checksum so hand-edited totals are spotted. It is a deterrent,
-- not security: leaderboards also need the character verified through Battle.net.

local SIG_VERSION = 1 -- bump with KEY on every release that changes it
local KEY = "OlApFDIsED1aJE04makEnBj7W6DZwvYG"
local REFRESH = 600

FraglinkPlayedDB = FraglinkPlayedDB or {}

local REGIONS = { [1] = "us", [2] = "kr", [3] = "eu", [4] = "tw", [5] = "cn" }

-- Two 31-bit rolling hashes (plain arithmetic: exact in Lua 5.1 doubles).
local function hash(s)
  local a, b = 7, 11
  for i = 1, #s do
    local c = string.byte(s, i)
    a = (a * 131 + c) % 2147483629
    b = (b * 137 + c + a % 251) % 2147483587
  end
  return string.format("%08x%08x", a, b)
end

local function sign(fields)
  return hash(KEY .. hash(KEY .. "|" .. table.concat(fields, "|")))
end

local pending = false
local muted = {}

local function mute()
  for i = 1, (NUM_CHAT_WINDOWS or 10) do
    local f = _G["ChatFrame" .. i]
    if f and f.IsEventRegistered and f:IsEventRegistered("TIME_PLAYED_MSG") then
      f:UnregisterEvent("TIME_PLAYED_MSG")
      muted[#muted + 1] = f
    end
  end
end

local function unmute()
  for _, f in ipairs(muted) do
    f:RegisterEvent("TIME_PLAYED_MSG")
  end
  muted = {}
end

local function ask()
  if pending then return end
  pending = true
  mute()
  RequestTimePlayed()
  -- never leave the chat frames muted if the answer doesn't come
  C_Timer.After(15, function()
    if pending then
      pending = false
      unmute()
    end
  end)
end

local function save(total)
  local region = REGIONS[GetCurrentRegion and GetCurrentRegion() or 0] or "unknown"
  local realm = GetRealmName() or ""
  local name = UnitName("player") or ""
  local _, class = UnitClass("player")
  class = class or ""
  local level = UnitLevel("player") or 0
  local played = math.floor(total or 0)
  local at = time()
  local battletag = ""
  if BNGetInfo then
    local _, bt = BNGetInfo()
    battletag = bt or ""
  end
  if realm == "" or name == "" or played <= 0 then return false end
  local e = {
    region = region, realm = realm, name = name, class = class,
    level = level, played = played, at = at, sv = SIG_VERSION, battletag = battletag,
  }
  e.sig = sign({
    region, realm, name, class,
    string.format("%d", level), string.format("%d", played), string.format("%d", at),
    string.format("%d", SIG_VERSION), battletag,
  })
  FraglinkPlayedDB.chars = FraglinkPlayedDB.chars or {}
  FraglinkPlayedDB.chars[region .. "/" .. realm .. "/" .. name] = e
  FraglinkPlayedDB.version = 1
  return true, name .. "-" .. realm
end

-- No in-game UI: one system-style chat line per login, unless /fraglink quiet is on.
local announced = true
local function announce(who)
  if announced or FraglinkPlayedDB.quiet then return end
  announced = true
  DEFAULT_CHAT_FRAME:AddMessage("|cffd4a017Fraglink|r: /played recorded for " .. who .. ".")
end

SLASH_FRAGLINK1 = "/fraglink"
SlashCmdList.FRAGLINK = function(msg)
  msg = (msg or ""):lower():gsub("^%s+", ""):gsub("%s+$", "")
  if msg == "quiet" then
    FraglinkPlayedDB.quiet = not FraglinkPlayedDB.quiet
    DEFAULT_CHAT_FRAME:AddMessage("|cffd4a017Fraglink|r: login message " .. (FraglinkPlayedDB.quiet and "hidden." or "shown."))
  else
    DEFAULT_CHAT_FRAME:AddMessage("|cffd4a017Fraglink|r: /fraglink quiet - hide or show the login message.")
  end
end

local f = CreateFrame("Frame")
f:RegisterEvent("PLAYER_ENTERING_WORLD")
f:RegisterEvent("PLAYER_LEVEL_UP")
f:RegisterEvent("TIME_PLAYED_MSG")
f:SetScript("OnEvent", function(_, event, a1, a2)
  if event == "TIME_PLAYED_MSG" then
    local ok, who = save(a1)
    if ok then announce(who) end
    if pending then
      pending = false
      -- re-enable chat after this event has gone past every frame
      C_Timer.After(0, unmute)
    end
  elseif event == "PLAYER_ENTERING_WORLD" then
    if a1 or a2 then -- first login or /reload
      if a1 then announced = false end -- the chat line is once per login, not per /reload
      C_Timer.After(5, ask)
    end
  elseif event == "PLAYER_LEVEL_UP" then
    C_Timer.After(2, ask)
  end
end)

C_Timer.NewTicker(REFRESH, ask)
