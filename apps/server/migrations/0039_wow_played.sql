-- WoW /played from the Fraglink addon: one row per character, its /played history,
-- the characters Battle.net says are on the user's account, and checks that failed.
-- Additive only, so the previous binary keeps working.

ALTER TABLE linked_accounts ADD COLUMN IF NOT EXISTS scopes text;

CREATE TABLE IF NOT EXISTS wow_characters (
    id              uuid PRIMARY KEY,
    user_id         uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id         text NOT NULL CHECK (game_id IN ('wow', 'wow_forever')),
    region          text NOT NULL,
    realm           text NOT NULL,
    name            text NOT NULL,
    realm_slug      text NOT NULL DEFAULT '',
    class           text NOT NULL DEFAULT '',
    level           integer NOT NULL DEFAULT 0,
    played_s        bigint NOT NULL,
    played_at       timestamptz NOT NULL,       -- when the game answered /played
    uploaded_at     timestamptz NOT NULL DEFAULT now(),
    verified        boolean NOT NULL DEFAULT false, -- on the user's Battle.net account
    bnet_level      integer,
    bnet_last_login timestamptz,
    bnet_checked_at timestamptz,
    hidden          boolean NOT NULL DEFAULT false, -- not shown on the profile
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS wow_characters_key
    ON wow_characters (user_id, game_id, region, lower(realm), lower(name));

CREATE TABLE IF NOT EXISTS wow_played_snaps (
    char_id  uuid NOT NULL REFERENCES wow_characters(id) ON DELETE CASCADE,
    played_s bigint NOT NULL,
    at       timestamptz NOT NULL,
    PRIMARY KEY (char_id, at)
);

CREATE TABLE IF NOT EXISTS bnet_characters (
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id    text NOT NULL,
    region     text NOT NULL,
    realm_slug text NOT NULL,
    realm_name text NOT NULL DEFAULT '',
    name       text NOT NULL,
    level      integer,
    fetched_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, game_id, region, realm_slug, name)
);

CREATE TABLE IF NOT EXISTS wow_flags (
    id         bigserial PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id    text NOT NULL,
    char_label text NOT NULL,
    kind       text NOT NULL,   -- wow_checksum | wow_decrease | wow_impossible | wow_bnet_mismatch
    played_s   bigint,
    detail     text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS wow_flags_recent ON wow_flags (created_at DESC);
