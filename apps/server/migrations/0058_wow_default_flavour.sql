-- Profile WoW display. default_flavour null means Retail opens first.
-- profile_show null means Both. Existing preference rows stay null.
-- Character rows, snapshots and hours are not touched.

ALTER TABLE wow_prefs ADD COLUMN IF NOT EXISTS default_flavour text;
-- Null means Both: Retail and Forever, with the toggle. Existing rows stay null.
ALTER TABLE wow_prefs ADD COLUMN IF NOT EXISTS profile_show text;

ALTER TABLE wow_prefs DROP CONSTRAINT IF EXISTS wow_prefs_default_flavour_check;
ALTER TABLE wow_prefs ADD CONSTRAINT wow_prefs_default_flavour_check
    CHECK (default_flavour IS NULL OR default_flavour IN ('wow', 'wow_forever'));

ALTER TABLE wow_prefs DROP CONSTRAINT IF EXISTS wow_prefs_profile_show_check;
ALTER TABLE wow_prefs ADD CONSTRAINT wow_prefs_profile_show_check
    CHECK (profile_show IS NULL OR profile_show IN ('wow', 'wow_forever', 'both'));
