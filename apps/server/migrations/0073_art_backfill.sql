-- Staff-started /games banner backfill: pick the next due game with one indexed
-- query. Additive only: no changes to sessions, hours, or player rows.
-- Nothing in this table starts a fetch on its own.

CREATE TABLE IF NOT EXISTS art_backfill_run (
    id bigserial PRIMARY KEY,
    status text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    started_by text,
    done bigint NOT NULL DEFAULT 0,
    found bigint NOT NULL DEFAULT 0,
    failed bigint NOT NULL DEFAULT 0,
    remaining bigint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS art_backfill_queue (
    game_id text PRIMARY KEY,
    state text NOT NULL DEFAULT 'pending',
    next_due_at timestamptz NOT NULL DEFAULT '-infinity'
);

-- Walk due pending rows in id order and stop at the first one.
CREATE INDEX IF NOT EXISTS art_backfill_queue_due
    ON art_backfill_queue (next_due_at, game_id)
    WHERE state = 'pending';
