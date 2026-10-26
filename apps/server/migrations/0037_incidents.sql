-- Public status page: past incidents that staff post (website /status). Additive only.
CREATE TABLE IF NOT EXISTS incidents (
    id          bigserial PRIMARY KEY,
    title       text NOT NULL,
    body        text NOT NULL DEFAULT '',
    severity    text NOT NULL DEFAULT 'minor' CHECK (severity IN ('minor', 'major', 'maintenance')),
    started_at  timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    created_by  uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS incidents_started ON incidents (started_at DESC);
