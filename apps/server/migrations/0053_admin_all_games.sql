-- Admin > Games > All games (sidebar console). Additive only, so the previous binary keeps working.

-- When a Steam sync last changed a game's row (name, Steam app id, icon). Staff edits
-- live in game_overrides; this covers the automatic changes. Each change is also written
-- to admin_audit as actor 'Steam sync' (action game.steam_sync).
ALTER TABLE games ADD COLUMN IF NOT EXISTS updated_at timestamptz;

-- Every game the downloaded games list has carried, with what it looked like last time.
-- first_seen is the first refresh that had it; baseline = it was already on the list when
-- this tracking started (so its real first day is unknown). removed_at is set when the
-- list drops it and cleared if it comes back.
CREATE TABLE IF NOT EXISTS game_list_seen (
    game_id    text PRIMARY KEY,
    name       text NOT NULL,
    exes       text[] NOT NULL DEFAULT '{}',
    first_seen timestamptz NOT NULL DEFAULT now(),
    baseline   boolean NOT NULL DEFAULT false,
    removed_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- What each daily refresh changed, per game: new, renamed, exes, removed, back.
CREATE TABLE IF NOT EXISTS game_list_changes (
    id         bigserial PRIMARY KEY,
    game_id    text NOT NULL,
    kind       text NOT NULL CHECK (kind IN ('new', 'renamed', 'exes', 'removed', 'back')),
    before     jsonb,
    after      jsonb,
    list_updated text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS game_list_changes_game ON game_list_changes (game_id, created_at DESC);
CREATE INDEX IF NOT EXISTS game_list_changes_time ON game_list_changes (created_at DESC);

-- Per-game history and "last changed by" in Admin.
CREATE INDEX IF NOT EXISTS admin_audit_target ON admin_audit (target, created_at DESC);
