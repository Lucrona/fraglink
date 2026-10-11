-- dev.18: retire only some of a game's programs, and a per-game lifecycle status.

-- `exes`: the programs detached by the retirement. `partial`: only those programs
-- moved to new entries (exe:<name> each); the game itself stays live.
ALTER TABLE game_retirements ADD COLUMN IF NOT EXISTS exes text[] NOT NULL DEFAULT '{}';
ALTER TABLE game_retirements ADD COLUMN IF NOT EXISTS partial boolean NOT NULL DEFAULT false;

-- Staff-kept status of a game (beta ending, early access, ...), also updated by the
-- weekly "Fraglink check" routine via `fraglink-server game-status`.
CREATE TABLE IF NOT EXISTS game_status (
    game_id    text PRIMARY KEY,
    lifecycle  text NOT NULL DEFAULT 'live'
               CHECK (lifecycle IN ('live', 'beta', 'test', 'early_access', 'ended')),
    key_date   date,
    note       text CHECK (note IS NULL OR char_length(note) <= 500),
    checked_at timestamptz,
    checked_by text,
    updated_at timestamptz NOT NULL DEFAULT now()
);
