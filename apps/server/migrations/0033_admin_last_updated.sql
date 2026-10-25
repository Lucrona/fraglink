-- Admin > Games / Detection "last updated" (0.3.0-beta.5): when background jobs last ran,
-- and who last changed a detection fix-up. Additive only, so the previous binary keeps working.
CREATE TABLE IF NOT EXISTS job_runs (
    job        text PRIMARY KEY,
    last_run_at timestamptz NOT NULL DEFAULT now(),
    last_ok_at  timestamptz,
    detail      text
);
ALTER TABLE detection_aliases ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE detection_aliases ADD COLUMN IF NOT EXISTS updated_by uuid REFERENCES users(id) ON DELETE SET NULL;
UPDATE detection_aliases SET updated_at = created_at;
