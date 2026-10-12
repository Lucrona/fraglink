-- 0.2.0-dev.20: game ids the app reports that the server doesn't know go to Game approvals
-- instead of silently becoming an "Unlisted game".
CREATE TABLE IF NOT EXISTS unknown_games (
    game_id    text PRIMARY KEY,
    exe        text,
    status     text NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'mapped', 'approved', 'rejected')),
    mapped_to  text,
    first_at   timestamptz NOT NULL DEFAULT now(),
    last_at    timestamptz NOT NULL DEFAULT now(),
    decided_by uuid REFERENCES users(id) ON DELETE SET NULL,
    decided_at timestamptz
);
