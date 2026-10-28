-- The BattleTag the addon saw for each character's latest accepted entry. A character
-- verifies only while it matches the linked Battle.net account. Additive only.
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS battletag text NOT NULL DEFAULT '';
