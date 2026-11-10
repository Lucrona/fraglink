-- WoW Forever /played imports. Additive only.
--
-- New nullable columns on wow_characters. Existing rows are not updated:
-- flavour, project_id, client_build, first_name and surname stay null on
-- every character saved before this migration.
--
-- wow_played_quarantine holds uploads that are not stored as characters:
-- a beta or unconfirmed client, a Forever name without a surname, a
-- decreasing total, or a flavour that does not match the chosen game.
-- Nothing in this file deletes or rewrites sessions, hours_entries,
-- steam_playtime, or existing wow_characters rows.

ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS flavour text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS project_id integer;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS client_build text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS first_name text;
ALTER TABLE wow_characters ADD COLUMN IF NOT EXISTS surname text;

CREATE TABLE IF NOT EXISTS wow_played_quarantine (
    id           uuid PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    claimed_game text NOT NULL,
    reason       text NOT NULL,
    detail       text NOT NULL DEFAULT '',
    payload      jsonb NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS wow_played_quarantine_user
    ON wow_played_quarantine (user_id, created_at DESC);
