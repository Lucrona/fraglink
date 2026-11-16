-- Public game pages and the Games tab extras. Additive only: no changes to
-- sessions, hours, or player tables. Admin overrides and cached store/news
-- live here, separate from detection overrides.
CREATE TABLE game_page_meta (
    game_id text PRIMARY KEY,
    slug text UNIQUE,
    developer text,
    publisher text,
    release_date text,
    genres text,
    platforms text,
    news_feed_url text,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE game_feed_cache (
    game_id text PRIMARY KEY,
    steam_appid integer,
    details_json jsonb,
    details_fetched_at timestamptz,
    details_ok boolean,
    news_json jsonb,
    news_fetched_at timestamptz,
    news_ok boolean,
    news_kind text
);
