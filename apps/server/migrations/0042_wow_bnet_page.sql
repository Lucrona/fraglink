-- Battle.net page: portraits (race/gender from Battle.net), the player's spec pick (defaults
-- to Battle.net's active spec) and "Hide all from profile", which also covers characters
-- verified later. Additive only.
ALTER TABLE bnet_characters ADD COLUMN IF NOT EXISTS race text;
ALTER TABLE bnet_characters ADD COLUMN IF NOT EXISTS gender text;
ALTER TABLE bnet_characters ADD COLUMN IF NOT EXISTS class_name text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS race text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS gender text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS bnet_spec text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS spec text;
CREATE TABLE IF NOT EXISTS wow_prefs (
    user_id  uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    hide_all boolean NOT NULL DEFAULT false
);
CREATE INDEX IF NOT EXISTS wow_characters_user ON wow_characters (user_id, verified, played_s DESC);
