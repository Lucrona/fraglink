-- New uploads can identify Forever characters independently of display surnames.
-- Existing rows remain untouched; no backfill or character/hour merge is performed.
ALTER TABLE wow_characters ADD COLUMN addon_guid text;
CREATE UNIQUE INDEX wow_forever_addon_guid ON wow_characters
    (user_id, region, lower(realm), addon_guid)
    WHERE game_id = 'wow_forever' AND addon_guid IS NOT NULL;
