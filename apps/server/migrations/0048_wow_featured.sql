-- Featured WoW characters: the owner picks up to 3 per flavour, shown first on the profile
-- in that order (1, 2, 3). None picked: the profile keeps sorting by hours.
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS featured smallint CHECK (featured BETWEEN 1 AND 3);
CREATE UNIQUE INDEX IF NOT EXISTS wow_characters_featured
    ON wow_characters (user_id, game_id, featured) WHERE featured IS NOT NULL;
