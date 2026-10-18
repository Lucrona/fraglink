-- Test accounts (staff background testing, e.g. fraglinktest): the account works normally,
-- but real users never see it: no search, profile, leaderboards, home page lists or totals,
-- signature card, or friends grids/counts on other profiles. Only the account itself and
-- people it is already friends with can find it (their own private friends list keeps it).
-- Set with SQL: UPDATE users SET is_test = true WHERE handle = '...';
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_test boolean NOT NULL DEFAULT false;
