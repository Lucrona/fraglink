-- Game-page warmer queue: pick the next due game with one indexed query.
-- Additive only: no changes to sessions, hours, or player rows.

CREATE TABLE IF NOT EXISTS game_warm_queue (
    game_id text PRIMARY KEY,
    name text NOT NULL,
    steam_appid integer,
    rank integer NOT NULL,
    next_due_at timestamptz NOT NULL DEFAULT '-infinity'
);

-- Walk in priority order (rank 0 first) and stop at the first due row.
CREATE INDEX IF NOT EXISTS game_warm_queue_rank_due
    ON game_warm_queue (rank, next_due_at);
