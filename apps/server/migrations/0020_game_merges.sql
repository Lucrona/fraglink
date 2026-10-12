-- Admin > Edit game > Merge into…: the old entry is retired with its data moved and its id
-- aliased to the target. Several games can merge into one target, so successor_id is
-- only unique for ordinary retirements.
ALTER TABLE game_retirements ADD COLUMN IF NOT EXISTS merged boolean NOT NULL DEFAULT false;
ALTER TABLE game_retirements DROP CONSTRAINT IF EXISTS game_retirements_successor_id_key;
CREATE UNIQUE INDEX IF NOT EXISTS game_retirements_successor_unmerged
    ON game_retirements (successor_id) WHERE NOT merged;
