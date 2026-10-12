-- 0.2.0-dev.20: fuller profile details and profile reports.
ALTER TABLE users ADD COLUMN IF NOT EXISTS real_name text CHECK (real_name IS NULL OR char_length(real_name) <= 40);
ALTER TABLE users ADD COLUMN IF NOT EXISTS gender text CHECK (gender IS NULL OR char_length(gender) <= 20);
ALTER TABLE users ADD COLUMN IF NOT EXISTS birth_date date;
ALTER TABLE users ADD COLUMN IF NOT EXISTS country text CHECK (country IS NULL OR country ~ '^[A-Z]{2}$');
CREATE TABLE IF NOT EXISTS profile_reports (
    id          bigserial PRIMARY KEY,
    target      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reporter    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason      text CHECK (reason IS NULL OR char_length(reason) <= 300),
    created_at  timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    resolved_by uuid REFERENCES users(id) ON DELETE SET NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS profile_reports_open ON profile_reports (target, reporter) WHERE resolved_at IS NULL;
