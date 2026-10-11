-- 0.2.0-dev.19: profiles are public by default, with per-feature hide switches.
-- public_profile = false is now the "Private profile" master switch (only friends see the
-- profile; hidden from home lists; signature image off). Nobody could tell "chose private"
-- apart from the old default, so every existing account becomes public.
ALTER TABLE users ALTER COLUMN public_profile SET DEFAULT true;
UPDATE users SET public_profile = true WHERE NOT public_profile;
-- Per-feature visibility; a missing key means "shown". Keys: now_playing, games, rig,
-- about, location, gaming_style, leaderboards.
ALTER TABLE users ADD COLUMN IF NOT EXISTS privacy jsonb NOT NULL DEFAULT '{}'::jsonb;
