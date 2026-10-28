-- One WoW character = one Battle.net character id, so a rename or realm transfer stays one
-- character and a character sold to someone else counts only for whoever Battle.net lists it
-- under now (the newest listing). Ids are filled from Battle.net (listing at link time, the
-- character profile API for older rows). `ever_verified`: a character that was verified and
-- then vanished from every listing (deleted) keeps its hours.
ALTER TABLE bnet_characters ADD COLUMN IF NOT EXISTS bnet_id bigint;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS bnet_id bigint;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS ever_verified boolean NOT NULL DEFAULT false;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS bnet_id_tried_at timestamptz;
UPDATE wow_characters SET ever_verified = true WHERE verified;
CREATE INDEX IF NOT EXISTS bnet_characters_bnet_id ON bnet_characters (game_id, region, bnet_id) WHERE bnet_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS wow_characters_bnet_id ON wow_characters (game_id, region, bnet_id) WHERE bnet_id IS NOT NULL;
