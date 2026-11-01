-- Runs FraglinkPlayed.lua under plain Lua 5.1 with stubbed WoW API and prints the
-- SavedVariables file the game would write. Used to make the Rust test fixture:
--   lua5.1 apps/desktop/addon/test_addon.lua > crates/fraglink-core/tests/fixtures/FraglinkPlayed.lua
local timers, handlers = {}, nil
local chat = { registered = true }
function chat:IsEventRegistered() return self.registered end
function chat:UnregisterEvent() self.registered = false end
function chat:RegisterEvent() self.registered = true end
ChatFrame1 = chat
NUM_CHAT_WINDOWS = 1
local lines = {}
DEFAULT_CHAT_FRAME = { AddMessage = function(_, m) lines[#lines + 1] = m end }
SlashCmdList = {}
C_Timer = {
  After = function(_, fn) timers[#timers + 1] = fn end,
  NewTicker = function() end,
}
function CreateFrame()
  return { RegisterEvent = function() end, SetScript = function(_, _, fn) handlers = fn end }
end
function GetCurrentRegion() return 1 end
function GetRealmName() return "Area 52" end
function UnitName() return "Thrallé" end
function UnitClass() return "Shaman", "SHAMAN" end
function UnitLevel() return 80 end
function BNGetInfo() return 1, "Thrall#1234" end
function time() return 1793000000 end
local asked = false
function RequestTimePlayed() asked = true end
local function run_timers()
  local t = timers; timers = {}
  for _, fn in ipairs(t) do fn() end
end

dofile(arg and arg[0]:gsub("test_addon%.lua$", "") .. "FraglinkPlayed/FraglinkPlayed.lua")
handlers(nil, "PLAYER_ENTERING_WORLD", true, false)
run_timers()
assert(asked, "asked")
assert(chat.registered == false, "chat muted while waiting")
handlers(nil, "TIME_PLAYED_MSG", 1234567, 1000)
run_timers()
assert(chat.registered == true, "chat unmuted")
assert(#lines == 1 and lines[1] == "|cffd4a017Fraglink|r: /played recorded for Thrallé-Area 52.", lines[1])
-- the 10-minute refresh and a /reload don't repeat the line
handlers(nil, "TIME_PLAYED_MSG", 1234600, 1000)
handlers(nil, "PLAYER_ENTERING_WORLD", false, true)
handlers(nil, "TIME_PLAYED_MSG", 1234700, 1000)
assert(#lines == 1, "one line per login")
-- /fragq hides it on the next login, and toggles back
SlashCmdList.FRAGQ("")
assert(FraglinkPlayedDB.quiet == true)
handlers(nil, "PLAYER_ENTERING_WORLD", true, false)
handlers(nil, "TIME_PLAYED_MSG", 1234800, 1000)
assert(#lines == 2, "quiet: only the toggle reply")
SlashCmdList.FRAGQ("")
assert(FraglinkPlayedDB.quiet == false)
timers = {}
-- the fixture keeps the first answer
handlers(nil, "TIME_PLAYED_MSG", 1234567, 1000)

local function ser(v, ind)
  if type(v) == "table" then
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = k end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    local out = { "{\n" }
    for _, k in ipairs(keys) do
      out[#out + 1] = ind .. "\t[" .. string.format("%q", k) .. "] = " .. ser(v[k], ind .. "\t") .. ",\n"
    end
    out[#out + 1] = ind .. "}"
    return table.concat(out)
  elseif type(v) == "string" then
    return string.format("%q", v)
  end
  return tostring(v)
end
io.write("\nFraglinkPlayedDB = " .. ser(FraglinkPlayedDB, "") .. "\n")
