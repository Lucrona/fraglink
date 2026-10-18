-- Admin > Site settings: feature switches. A row exists only once staff changed a switch;
-- missing keys use the defaults in src/features.rs.
CREATE TABLE IF NOT EXISTS site_settings (
    key        text PRIMARY KEY CHECK (char_length(key) <= 40),
    enabled    boolean NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid REFERENCES users(id) ON DELETE SET NULL
);
