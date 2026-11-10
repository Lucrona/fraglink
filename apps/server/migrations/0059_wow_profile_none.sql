-- Let a profile hide its WoW section. Existing rows are not updated.
-- Null still means Both. 'wow', 'wow_forever' and 'both' stay valid.

ALTER TABLE wow_prefs DROP CONSTRAINT IF EXISTS wow_prefs_profile_show_check;
ALTER TABLE wow_prefs ADD CONSTRAINT wow_prefs_profile_show_check
    CHECK (profile_show IS NULL OR profile_show IN ('wow', 'wow_forever', 'both', 'none'));
