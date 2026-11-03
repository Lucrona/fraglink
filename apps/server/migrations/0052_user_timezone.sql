-- Per-user time zone (IANA name, e.g. "America/Los_Angeles"). NULL = automatic: the
-- browser's zone where there is one, else the site default (Australia/Sydney). The server
-- checks names against chrono-tz and Postgres before saving.
ALTER TABLE users ADD COLUMN timezone text CHECK (timezone IS NULL OR char_length(timezone) BETWEEN 1 AND 64);
