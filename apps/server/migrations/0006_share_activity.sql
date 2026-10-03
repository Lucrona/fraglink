-- Whether friends may see this user's recent play history (Activity Report, hours this week).
-- Live presence (what you're playing right now) is unaffected and follows the status/invisible rules.
ALTER TABLE users ADD COLUMN share_activity boolean NOT NULL DEFAULT true;
