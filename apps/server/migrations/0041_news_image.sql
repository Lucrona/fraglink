-- Optional cover image and button label for news posts.
ALTER TABLE news_posts ADD COLUMN IF NOT EXISTS image text CHECK (image IS NULL OR char_length(image) <= 500);
ALTER TABLE news_posts ADD COLUMN IF NOT EXISTS link_label text CHECK (link_label IS NULL OR char_length(link_label) <= 40);
