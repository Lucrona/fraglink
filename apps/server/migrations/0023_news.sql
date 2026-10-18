-- Home page / /news posts, written in Admin > News. Separate from `announcements`
-- (the short site/app banner); a post can be promoted to the banner from the admin page.
CREATE TABLE IF NOT EXISTS news_posts (
    id           bigserial PRIMARY KEY,
    slug         text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND char_length(slug) <= 80),
    title        text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 140),
    body         text NOT NULL DEFAULT '' CHECK (char_length(body) <= 20000),
    link         text CHECK (link IS NULL OR char_length(link) <= 300),
    pinned       boolean NOT NULL DEFAULT false,
    published_at timestamptz NOT NULL DEFAULT now(),
    created_by   uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS news_posts_published ON news_posts (published_at DESC, id DESC);
