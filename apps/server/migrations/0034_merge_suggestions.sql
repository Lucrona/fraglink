-- Suggested merges (0.3.0-beta.5): a program someone added themselves (`exe:<name>.exe`)
-- that Fraglink's games list now knows. Staff merge it (Admin > Games > Merge) or dismiss
-- it; nothing moves on its own. Additive only, so the previous binary keeps working.
CREATE TABLE IF NOT EXISTS merge_suggestions (
    game_id    text PRIMARY KEY,
    into_id    text NOT NULL,
    status     text NOT NULL DEFAULT 'new',  -- new | dismissed
    first_at   timestamptz NOT NULL DEFAULT now(),
    decided_by uuid REFERENCES users(id) ON DELETE SET NULL,
    decided_at timestamptz
);
