-- Fraglink server schema v1 (M2). Mirrors the spec's §8.1 tables that M2 needs,
-- so it can move to Supabase/any managed Postgres later without reshaping data.

CREATE TABLE users (
    id            uuid PRIMARY KEY,
    email         text NOT NULL,
    handle        text NOT NULL,
    display_name  text NOT NULL,
    password_hash text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    deleted_at    timestamptz
);
CREATE UNIQUE INDEX users_email_key ON users (lower(email));
CREATE UNIQUE INDEX users_handle_key ON users (lower(handle));

-- Opaque bearer tokens; only a SHA-256 of the token is stored.
CREATE TABLE auth_tokens (
    id           uuid PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash   bytea NOT NULL UNIQUE,
    kind         text NOT NULL CHECK (kind IN ('web', 'device')),
    device_name  text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz,
    revoked_at   timestamptz
);
CREATE INDEX auth_tokens_user ON auth_tokens (user_id);

-- Desktop pairing (device-code flow): the tray shows user_code, the signed-in
-- browser approves it, the tray polls with the secret device_code.
CREATE TABLE device_codes (
    device_code_hash bytea PRIMARY KEY,
    user_code        text NOT NULL UNIQUE,
    device_name      text NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    expires_at       timestamptz NOT NULL,
    approved_by      uuid REFERENCES users(id) ON DELETE CASCADE,
    approved_at      timestamptz,
    consumed_at      timestamptz
);

CREATE TABLE games (
    id          text PRIMARY KEY,            -- Fraglink game id: "wow", "steam:570", "epic:Min", "manual:foo"
    name        text NOT NULL,
    steam_appid integer,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX games_steam_appid ON games (steam_appid);

-- Tracked tier, source 1: sessions logged by the tray (client-generated ids, idempotent upload).
CREATE TABLE sessions (
    id             uuid PRIMARY KEY,
    user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id        text NOT NULL REFERENCES games(id),
    started_at     timestamptz NOT NULL,
    ended_at       timestamptz NOT NULL,
    duration_s     integer NOT NULL CHECK (duration_s >= 0),
    idle_trimmed_s integer NOT NULL DEFAULT 0 CHECK (idle_trimmed_s >= 0),
    client_version text,
    source         text NOT NULL DEFAULT 'tracked',
    uploaded_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (ended_at >= started_at),
    CHECK (ended_at - started_at <= interval '24 hours')
);
CREATE INDEX sessions_user_time ON sessions (user_id, started_at);
CREATE INDEX sessions_game ON sessions (game_id);

CREATE TABLE linked_accounts (
    user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider      text NOT NULL CHECK (provider IN ('steam', 'discord', 'twitch', 'bnet')),
    provider_uid  text NOT NULL,
    display       text,
    verified      boolean NOT NULL DEFAULT false,
    show_on_profile boolean NOT NULL DEFAULT true,
    linked_at     timestamptz NOT NULL DEFAULT now(),
    last_sync_at  timestamptz,
    sync_status   text,
    PRIMARY KEY (user_id, provider)
);
-- One Fraglink account per verified provider identity.
CREATE UNIQUE INDEX linked_accounts_verified_uid ON linked_accounts (provider, provider_uid) WHERE verified;

-- Tracked tier, source 2: Steam's official playtime (latest snapshot per game).
CREATE TABLE steam_playtime (
    user_id         uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    steam_appid     integer NOT NULL,
    game_id         text NOT NULL REFERENCES games(id),
    minutes_forever integer NOT NULL CHECK (minutes_forever >= 0),
    minutes_2weeks  integer NOT NULL DEFAULT 0,
    fetched_at      timestamptz NOT NULL,
    PRIMARY KEY (user_id, steam_appid)
);

-- Imported (game counters, self-reported) and Estimated (history calculator) hours.
CREATE TABLE hours_entries (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id     text NOT NULL REFERENCES games(id),
    tier        text NOT NULL CHECK (tier IN ('imported', 'estimated')),
    source      text NOT NULL CHECK (source IN ('wow_addon', 'wow_manual', 'xbox_recap', 'manual', 'estimator')),
    minutes     integer NOT NULL CHECK (minutes >= 0),
    captured_at timestamptz NOT NULL DEFAULT now(),
    inputs      jsonb,
    note        text CHECK (char_length(note) <= 140),
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX hours_entries_user_game ON hours_entries (user_id, game_id, tier);

-- Per-game totals by tier (spec §5.2 merge rules). Tracked = max(local sessions, Steam
-- playtime) so Steam games never double count; Imported = latest cumulative counter;
-- Estimated is kept separate and never folded into display/rank totals.
CREATE VIEW user_game_stats AS
WITH s AS (
    SELECT user_id, game_id,
           sum(duration_s) / 60 AS session_min,
           sum(duration_s) FILTER (WHERE started_at >= now() - interval '7 days') / 60 AS min_7d,
           max(ended_at) AS last_played_at
    FROM sessions GROUP BY user_id, game_id
), st AS (
    SELECT user_id, game_id, max(minutes_forever) AS steam_min, max(minutes_2weeks) AS steam_2w
    FROM steam_playtime GROUP BY user_id, game_id
), imp AS (
    SELECT DISTINCT ON (user_id, game_id) user_id, game_id, minutes AS imported_min
    FROM hours_entries WHERE tier = 'imported'
    ORDER BY user_id, game_id, captured_at DESC, created_at DESC
), est AS (
    SELECT user_id, game_id, sum(minutes) AS estimated_min
    FROM hours_entries WHERE tier = 'estimated' GROUP BY user_id, game_id
), keys AS (
    SELECT user_id, game_id FROM s UNION SELECT user_id, game_id FROM st
    UNION SELECT user_id, game_id FROM imp UNION SELECT user_id, game_id FROM est
)
SELECT k.user_id, k.game_id, g.name,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0))::bigint AS tracked_min,
       coalesce(imp.imported_min, 0)::bigint AS imported_min,
       coalesce(est.estimated_min, 0)::bigint AS estimated_min,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0), coalesce(imp.imported_min, 0))::bigint AS display_min,
       CASE
         WHEN coalesce(imp.imported_min, 0) > GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0)) THEN 'imported'
         WHEN GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0)) > 0 THEN 'tracked'
         WHEN coalesce(est.estimated_min, 0) > 0 THEN 'estimated'
         ELSE 'tracked'
       END AS display_tier,
       coalesce(s.min_7d, 0)::bigint AS minutes_7d,
       s.last_played_at
FROM keys k
JOIN games g ON g.id = k.game_id
LEFT JOIN s   ON s.user_id = k.user_id AND s.game_id = k.game_id
LEFT JOIN st  ON st.user_id = k.user_id AND st.game_id = k.game_id
LEFT JOIN imp ON imp.user_id = k.user_id AND imp.game_id = k.game_id
LEFT JOIN est ON est.user_id = k.user_id AND est.game_id = k.game_id;
