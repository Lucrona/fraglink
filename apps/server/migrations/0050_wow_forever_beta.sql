-- WoW Forever launched (4 Nov 2026) with Wow.exe in `_forever_`. The beta entry (WowB.exe,
-- retired on 28 Oct) becomes its own game, `wow_forever_beta` "World of Warcraft: Forever Beta",
-- still retired, with every session, hour and record it had (only the id changes). The freed
-- `wow_forever` id is live Forever: same games row, nothing attached, not retired.
-- Runs only where `wow_forever` is the fully retired beta (production); elsewhere a no-op
-- apart from the wider CHECK.
ALTER TABLE wow_characters DROP CONSTRAINT IF EXISTS wow_characters_game_id_check;
ALTER TABLE wow_characters ADD CONSTRAINT wow_characters_game_id_check
    CHECK (game_id IN ('wow', 'wow_forever', 'wow_forever_beta'));

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM game_retirements
                   WHERE game_id = 'wow_forever' AND NOT partial AND NOT merged)
       OR EXISTS (SELECT 1 FROM games WHERE id = 'wow_forever_beta') THEN
        RETURN;
    END IF;
    -- the beta's own row first (foreign keys point at games.id)
    INSERT INTO games (id, name, steam_appid, created_at, steam_icon_hash, verified)
        SELECT 'wow_forever_beta', 'World of Warcraft: Forever Beta', steam_appid, created_at,
               steam_icon_hash, verified
        FROM games WHERE id = 'wow_forever';
    UPDATE sessions          SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE hours_entries     SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE steam_playtime    SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE wow_characters    SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE bnet_characters   SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE wow_flags         SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE hidden_games      SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE own_game_names    SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE unknown_games     SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE merge_suggestions SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE merge_suggestions SET into_id = 'wow_forever_beta' WHERE into_id = 'wow_forever';
    UPDATE detection_aliases SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE game_status       SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE game_overrides    SET game_id = 'wow_forever_beta', name = 'World of Warcraft: Forever Beta'
        WHERE game_id = 'wow_forever';
    UPDATE game_retirements  SET game_id = 'wow_forever_beta' WHERE game_id = 'wow_forever';
    UPDATE game_retirements  SET successor_id = 'wow_forever_beta' WHERE successor_id = 'wow_forever';
    -- staff log: the earlier actions were on the beta entry (their before/after kept as written)
    UPDATE admin_audit       SET target = 'wow_forever_beta' WHERE target = 'wow_forever';
    UPDATE games SET name = 'World of Warcraft: Forever' WHERE id = 'wow_forever';
END $$;
