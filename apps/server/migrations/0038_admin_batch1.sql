-- Admin batch 1: other names (aliases) and folder hints per game, edited in Admin > Games.
-- Additive only, so the previous binary keeps working.
ALTER TABLE game_overrides ADD COLUMN IF NOT EXISTS aliases text[] NOT NULL DEFAULT '{}';
ALTER TABLE game_overrides ADD COLUMN IF NOT EXISTS path_hints text[] NOT NULL DEFAULT '{}';
