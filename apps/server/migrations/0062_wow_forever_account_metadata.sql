-- Optional ownership-list metadata only. No character/hour backfill or rewrites.
ALTER TABLE bnet_characters ADD COLUMN addon_guid text;
ALTER TABLE bnet_characters ADD COLUMN faction text
    CHECK (faction IN ('HORDE', 'ALLIANCE'));
