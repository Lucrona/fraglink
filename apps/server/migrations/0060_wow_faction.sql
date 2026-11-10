-- Profile metadata only. No backfill: populated by normal Blizzard profile refreshes.
ALTER TABLE wow_characters ADD COLUMN faction text
    CHECK (faction IN ('HORDE', 'ALLIANCE'));
