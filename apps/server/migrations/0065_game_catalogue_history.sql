-- Admin > Games change history, and the public /games "Recent updates" list.
-- Additive only. No backfill of earlier edits; one 'history started' marker so the
-- empty log has a start point. Previous binaries ignore this table.

CREATE TABLE IF NOT EXISTS game_catalogue_history (
    id                 bigserial PRIMARY KEY,
    created_at         timestamptz NOT NULL DEFAULT now(),
    actor_kind         text NOT NULL CHECK (actor_kind IN ('admin', 'system')),
    actor_id           uuid REFERENCES users(id) ON DELETE SET NULL,
    actor_label        text NOT NULL,
    action             text NOT NULL CHECK (action IN (
        'added', 'edited', 'renamed', 'merged', 'hidden', 'unhidden', 'removed',
        'retired', 'unretired', 'detection', 'icon', 'steam_sync', 'list_sync',
        'manual', 'history_started'
    )),
    game_id            text,
    game_name          text,
    summary            text NOT NULL CHECK (char_length(summary) BETWEEN 1 AND 300),
    diff               jsonb,
    -- Visible on /games when true and not hidden. Public-safe types (added,
    -- renamed, icon, manual, and sync summaries that only mention those) set this.
    public             boolean NOT NULL DEFAULT false,
    hidden_from_public boolean NOT NULL DEFAULT false,
    -- Server-side games/detectable list version (timestamp string) when this was recorded.
    list_updated       text
);

CREATE INDEX IF NOT EXISTS game_catalogue_history_time
    ON game_catalogue_history (created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS game_catalogue_history_game
    ON game_catalogue_history (game_id, created_at DESC, id DESC)
    WHERE game_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS game_catalogue_history_public
    ON game_catalogue_history (created_at DESC, id DESC)
    WHERE public AND NOT hidden_from_public;
CREATE INDEX IF NOT EXISTS game_catalogue_history_action
    ON game_catalogue_history (action, created_at DESC, id DESC);

INSERT INTO game_catalogue_history (actor_kind, actor_label, action, summary, public)
SELECT 'system', 'system', 'history_started', 'Catalogue change history started.', false
WHERE NOT EXISTS (SELECT 1 FROM game_catalogue_history WHERE action = 'history_started');
