-- Nullable format markers preserve all existing bodies and changelog arrays.
-- No backfill, data rewrite, deletion or default value.
ALTER TABLE news_posts ADD COLUMN content_format text
    CHECK (content_format IS NULL OR content_format = 'markdown');
ALTER TABLE changelog_entries ADD COLUMN content_format text
    CHECK (content_format IS NULL OR content_format = 'markdown');
