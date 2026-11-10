-- Past names and merge records for a verified WoW character whose Battle.net
-- id stayed the same while the name or realm changed.
--
-- The Blizzard character id is already stored on wow_characters.bnet_id
-- (0049). This file does not add that column and does not copy any id,
-- name, hour, or snapshot. Existing character rows stay as they are.
-- Both tables are created empty.

CREATE TABLE IF NOT EXISTS wow_character_names (
    id          bigserial PRIMARY KEY,
    char_id     uuid NOT NULL REFERENCES wow_characters(id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        text NOT NULL,
    realm       text NOT NULL DEFAULT '',
    realm_slug  text NOT NULL DEFAULT '',
    recorded_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS wow_character_names_char
    ON wow_character_names (char_id, recorded_at DESC);

CREATE TABLE IF NOT EXISTS wow_character_merges (
    id          bigserial PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kept_id     uuid NOT NULL,
    dropped_id  uuid NOT NULL,
    before      jsonb NOT NULL,
    after       jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS wow_character_merges_user
    ON wow_character_merges (user_id, created_at DESC);
