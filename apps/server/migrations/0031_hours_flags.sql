-- Admin alert "Unusual hours": the account is flagged for staff to look at (nothing is
-- removed; idle and away time count as played time).
ALTER TABLE users ADD COLUMN IF NOT EXISTS hours_flag_at timestamptz;
ALTER TABLE users ADD COLUMN IF NOT EXISTS hours_flag_reason text;
