-- Steam's public icon hash per game (from GetOwnedGames `img_icon_url`), so a
-- friend's tray can show the icon of a game it doesn't have installed.
ALTER TABLE games ADD COLUMN steam_icon_hash text
    CHECK (steam_icon_hash IS NULL OR steam_icon_hash ~ '^[0-9a-f]{40}$');
