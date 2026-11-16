-- Admin override for a game page's official or store link.
-- Additive only: no changes to sessions, hours, or player rows.

ALTER TABLE game_page_meta
    ADD COLUMN IF NOT EXISTS official_url text;
