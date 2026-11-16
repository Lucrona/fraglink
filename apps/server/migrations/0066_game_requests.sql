-- Public /games "Missing a game?" requests, and the Admin > Games requests queue.
-- Additive only. No hours, sessions, or existing catalogue tables are touched.

CREATE TABLE IF NOT EXISTS game_requests (
    id           bigserial PRIMARY KEY,
    created_at   timestamptz NOT NULL DEFAULT now(),
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    name_norm    text NOT NULL CHECK (char_length(name_norm) BETWEEN 1 AND 80),
    exe_or_link  text CHECK (exe_or_link IS NULL OR char_length(exe_or_link) BETWEEN 1 AND 240),
    note         text CHECK (note IS NULL OR char_length(note) BETWEEN 1 AND 500),
    status       text NOT NULL DEFAULT 'open' CHECK (status IN (
        'open', 'approved', 'done', 'duplicate', 'dismissed'
    )),
    reports      integer NOT NULL DEFAULT 1 CHECK (reports >= 1),
    decided_at   timestamptz,
    decided_by   uuid REFERENCES users(id) ON DELETE SET NULL,
    game_id      text
);

-- One open request per normalised name: later reports bump `reports` instead.
CREATE UNIQUE INDEX IF NOT EXISTS game_requests_open_norm
    ON game_requests (name_norm) WHERE status = 'open';
CREATE INDEX IF NOT EXISTS game_requests_open
    ON game_requests (created_at DESC, id DESC) WHERE status = 'open';
CREATE INDEX IF NOT EXISTS game_requests_user_day
    ON game_requests (user_id, created_at DESC);
