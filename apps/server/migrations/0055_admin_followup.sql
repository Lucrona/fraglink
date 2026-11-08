-- Follow-up Admin tools. Additive: the previous binary keeps running.
-- Hours Review "mark as reviewed" remembers the evidence it was judging, so a
-- later change to the sessions is not hidden by an old review. Nothing here
-- deletes or rewrites sessions.
CREATE TABLE IF NOT EXISTS hours_flag_reviews (
    id         bigserial PRIMARY KEY,
    flag_key   text NOT NULL,
    kind       text NOT NULL,
    evidence   text NOT NULL,
    status     text NOT NULL CHECK (status IN ('reviewed', 'reopened')),
    reason     text,
    actor_id   uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS hours_flag_reviews_key
    ON hours_flag_reviews (flag_key, created_at DESC, id DESC);

-- Dismissals for launcher-id suggestions. Exe: suggestions stay in merge_suggestions.
-- A dismissal only hides the same evidence; a changed launcher id or name pair shows again.
CREATE TABLE IF NOT EXISTS suggested_merge_decisions (
    id             bigserial PRIMARY KEY,
    suggestion_key text NOT NULL,
    evidence       text NOT NULL,
    status         text NOT NULL CHECK (status IN ('dismissed', 'reopened')),
    reason         text,
    decided_by     uuid REFERENCES users(id) ON DELETE SET NULL,
    decided_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS suggested_merge_decisions_key
    ON suggested_merge_decisions (suggestion_key, decided_at DESC, id DESC);
