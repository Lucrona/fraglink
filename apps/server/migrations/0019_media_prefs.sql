-- Admin > Edit game: pick where the icon and banner art come from (NULL = automatic).
ALTER TABLE game_overrides
    ADD COLUMN IF NOT EXISTS icon_pref text CHECK (icon_pref IN ('games_list', 'steam', 'custom')),
    ADD COLUMN IF NOT EXISTS art_pref text CHECK (art_pref IN ('games_list', 'steam', 'custom')),
    ADD COLUMN IF NOT EXISTS steam_appid integer;
