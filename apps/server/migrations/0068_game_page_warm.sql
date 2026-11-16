-- Game page warmer + Wikipedia/Wikidata cache extras.
-- Additive only: no changes to sessions, hours, or player rows.

ALTER TABLE game_page_meta
    ADD COLUMN IF NOT EXISTS description text;

ALTER TABLE game_feed_cache
    ADD COLUMN IF NOT EXISTS details_kind text,
    ADD COLUMN IF NOT EXISTS details_fail_count integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS news_fail_count integer NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS game_page_views (
    game_id text PRIMARY KEY,
    views bigint NOT NULL DEFAULT 0,
    last_viewed_at timestamptz NOT NULL DEFAULT now()
);
