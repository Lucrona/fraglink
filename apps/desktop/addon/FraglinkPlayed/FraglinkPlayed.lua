-- Fraglink /played: asks the game for this character's /played total (quietly, no
-- chat line) on login, on level-up and every 10 minutes, and keeps it in
-- FraglinkPlayedDB. The game writes that file on logout / reload; the Fraglink app
-- reads it after you close WoW. Nothing is sent anywhere from inside the game.
--
-- Each entry carries a checksum so hand-edited totals are spotted. It is a deterrent,
-- not security: leaderboards also need the character verified through Battle.net.

local SIG_VERSION = 3 -- bump with KEY on every release that changes it
local KEY = "G6vP9cN2rT8mK4wQ1xH5bD7sL3fA0eYz"
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

local function later(seconds, fn)
  if C_Timer and C_Timer.After then
    C_Timer.After(seconds, fn)
  end
end

local function ask()
  if pending or type(RequestTimePlayed) ~= "function" then return end
  pending = true
  mute()
  if not pcall(RequestTimePlayed) then
    pending = false
    unmute()
    return
  end
  -- never leave the chat frames muted if the answer doesn't come
  later(15, function()
    if pending then
      pending = false
      unmute()
    end
  end)
end

-- Live Forever is WOW_PROJECT_CAMELOT (18) and not a beta/test client.
-- Interface 16001 is shared with the old beta, so it is not used here.
-- If the beta/test APIs are missing, the client is not called live Forever.
local function client_facts()
  local project = tonumber(WOW_PROJECT_ID) or 0
  local camelot = tonumber(WOW_PROJECT_CAMELOT) or 18
  local build = ""
  if type(GetBuildInfo) == "function" then
    local ok, _, b = pcall(GetBuildInfo)
    if ok then build = tostring(b or "") end
  end
  local saw = (IsBetaBuild ~= nil) or (IsPublicTestClient ~= nil) or (IsTestBuild ~= nil)
  local beta = (IsBetaBuild and IsBetaBuild())
    or (IsPublicTestClient and IsPublicTestClient())
    or (IsTestBuild and IsTestBuild())
  local flavour, confirmed = "unknown", 0
  if project == camelot or project == 18 then
    if beta then
      flavour, confirmed = "forever_beta", 1
    elseif saw then
      flavour, confirmed = "forever", 1
    else
      flavour, confirmed = "forever_unconfirmed", 0
    end
  elseif project == 1 or project == (tonumber(WOW_PROJECT_MAINLINE) or 1) then
    flavour, confirmed = "retail", 1
  end
  return flavour, project, build, beta and 1 or 0, confirmed
end

local function save(total)
  local region = REGIONS[GetCurrentRegion and GetCurrentRegion() or 0] or "unknown"
  local flavour, project, build, beta, confirmed = client_facts()
  local forever = flavour == "forever" or flavour == "forever_beta" or flavour == "forever_unconfirmed"
  local realm = forever and "" or (GetRealmName() or "")
  local name, second = UnitName("player")
  name = name or ""
  local first, surname, guid = "", "", ""
  if forever then
    -- Forever's second UnitName/UnitFullName result is the surname, not a realm.
    first, surname = name, type(second) == "string" and second or ""
    if surname == "" and type(UnitFullName) == "function" then
      local ok, given, family = pcall(UnitFullName, "player")
      if ok and given == first and type(family) == "string" then surname = family end
    end
    if surname == "" and type(GetUnitName) == "function" then
      local ok, full = pcall(GetUnitName, "player")
      local prefix = first .. " "
      if ok and type(full) == "string" and full:sub(1, #prefix) == prefix then
        surname = full:sub(#prefix + 1)
      end
    end
    name = surname ~= "" and (first .. " " .. surname) or first
    if type(UnitGUID) == "function" then
      local ok, value = pcall(UnitGUID, "player")
      if ok and type(value) == "string" then guid = value end
    end
  end
  local _, class = UnitClass("player")
  class = class or ""
  local level = UnitLevel("player") or 0
  local played = math.floor(total or 0)
  local at = time()
  local battletag = ""
  if type(BNGetInfo) == "function" then
    local ok, _, bt = pcall(BNGetInfo)
    if ok then battletag = bt or "" end
  end
  if name == "" or played <= 0 then return false end
  if not forever and realm == "" then return false end
  local e = {
    region = region, realm = realm, name = name, class = class,
    level = level, played = played, at = at, sv = SIG_VERSION, battletag = battletag,
    flavour = flavour, project = project, build = build,
    first = first, surname = surname, guid = guid, beta = beta, confirmed = confirmed,
  }
  e.sig = sign({
    region, realm, name, class,
    string.format("%d", level), string.format("%d", played), string.format("%d", at),
    string.format("%d", SIG_VERSION), battletag,
    flavour, string.format("%d", project), build, first, surname,
    string.format("%d", beta), string.format("%d", confirmed), guid,
  })
  FraglinkPlayedDB.chars = FraglinkPlayedDB.chars or {}
  local key = forever and (region .. "/forever/" .. (guid ~= "" and guid or first)) or (region .. "/" .. realm .. "/" .. name)
  -- Reuse a single compatible legacy slot, including 1.1.0's missing surname.
  -- Never combine different GUIDs or guess between two legacy characters.
  if forever and not FraglinkPlayedDB.chars[key] then
    local found, ambiguous
    for oldkey, old in pairs(FraglinkPlayedDB.chars) do
      if type(old) == "table" and old.region == region and old.first == first
        and ((guid ~= "" and old.guid == guid)
          or ((old.guid == nil or old.guid == "")
            and (old.surname == nil or old.surname == "" or old.surname == surname))) then
        if found then ambiguous = true else found = oldkey end
      end
    end
    if found and not ambiguous then key = found end
    if ambiguous and guid == "" then return false end
  end
  local prev = FraglinkPlayedDB.chars[key]
  -- Without a GUID, a changed known surname is not enough to identify a character.
  if forever and guid == "" and type(prev) == "table"
    and type(prev.surname) == "string" and prev.surname ~= ""
    and surname ~= "" and prev.surname ~= surname then return false end
  -- A lower answer must not replace the total already saved on this PC.
  if type(prev) == "table" and type(prev.played) == "number" and played < prev.played then
    return false
  end
  FraglinkPlayedDB.chars[key] = e
  FraglinkPlayedDB.version = 3
  return true, forever and name or (name .. "-" .. realm)
end

-- No in-game UI: one system-style chat line per login, unless /fragq turned it off.
local announced = true
local function announce(who)
  if announced or FraglinkPlayedDB.quiet then return end
  announced = true
  DEFAULT_CHAT_FRAME:AddMessage("|cffd4a017Fraglink|r: /played recorded for " .. who .. ".")
end

-- /fragq toggles the login line.
SLASH_FRAGQ1 = "/fragq"
SlashCmdList.FRAGQ = function()
  FraglinkPlayedDB.quiet = not FraglinkPlayedDB.quiet
  local state = FraglinkPlayedDB.quiet and "hidden. Type /fragq to show it again." or "shown."
  DEFAULT_CHAT_FRAME:AddMessage("|cffd4a017Fraglink|r: login message " .. state)
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
      if C_Timer and C_Timer.After then later(0, unmute) else unmute() end
    end
  elseif event == "PLAYER_ENTERING_WORLD" then
    if a1 or a2 then -- first login or /reload
      if a1 then announced = false end -- the chat line is once per login, not per /reload
      later(5, ask)
    end
  elseif event == "PLAYER_LEVEL_UP" then
    later(2, ask)
  end
end)

if C_Timer and C_Timer.NewTicker then
  C_Timer.NewTicker(REFRESH, ask)
end
