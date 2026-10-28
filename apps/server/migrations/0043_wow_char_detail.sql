-- WoW character popup: a summarised Blizzard profile per character (12 h TTL, served stale
-- while it refreshes), item icon ids for gear (kept forever), and a per-character
-- "Show guild" switch (off by default). Additive only.
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS show_guild boolean NOT NULL DEFAULT false;
CREATE TABLE IF NOT EXISTS wow_char_details (
    char_id    uuid PRIMARY KEY REFERENCES wow_characters(id) ON DELETE CASCADE,
    status     text NOT NULL,             -- ok | missing (private or not found)
    data       jsonb NOT NULL DEFAULT '{}',
    fetched_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS wow_item_icons (
    item_id    bigint PRIMARY KEY,
    icon       text NOT NULL,             -- render CDN icon name, e.g. 132963
    fetched_at timestamptz NOT NULL DEFAULT now()
);
