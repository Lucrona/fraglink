-- Staff "Retire" (dev.18): a retired game entry is frozen (name, icon and every recorded
-- hour stay as they are) and its program is detached: play reported for it from now on
-- is recorded under `successor_id` (an `exe:<name>.exe` entry that goes through game
-- approvals like any new program). Deleting the row (unretire) re-attaches the program.
CREATE TABLE IF NOT EXISTS game_retirements (
    game_id      text PRIMARY KEY,
    successor_id text NOT NULL UNIQUE,
    retired_at   timestamptz NOT NULL DEFAULT now(),
    retired_by   uuid REFERENCES users(id) ON DELETE SET NULL
);
