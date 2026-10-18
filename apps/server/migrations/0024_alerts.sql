-- Admin email alerts (src/alerts.rs). Switches live in site_settings (alert_* keys); the
-- recipient is the text value of the alert_email row.
ALTER TABLE site_settings ADD COLUMN IF NOT EXISTS value text CHECK (value IS NULL OR char_length(value) <= 200);

-- Watermarks, debounce times and error counters, one row per key.
CREATE TABLE IF NOT EXISTS alert_state (
    key text PRIMARY KEY,
    at  timestamptz,
    n   bigint NOT NULL DEFAULT 0
);

-- Every alert email attempted (for the daily cap and the admin page).
CREATE TABLE IF NOT EXISTS alert_log (
    id        bigserial PRIMARY KEY,
    kind      text NOT NULL,
    recipient text NOT NULL,
    subject   text NOT NULL,
    ok        boolean NOT NULL,
    error     text,
    sent_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS alert_log_time ON alert_log (sent_at DESC);
