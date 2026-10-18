-- Under-18 accounts from date of birth, and the under-13 age-review lock.
-- is_minor: set from DOB at sign-up (13–17) or when a missing DOB is added later.
-- Existing accounts without a DOB count as adults. Minors always have a private profile.
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_minor boolean NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN IF NOT EXISTS minor_set_at timestamptz;
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_minor_private;
ALTER TABLE users ADD CONSTRAINT users_minor_private CHECK (NOT (is_minor AND public_profile));
-- Set when a birthday showing an age under 13 was entered on an existing account:
-- the account can't sign in until staff clear it in Admin > Users. The under-13
-- date itself is never stored.
ALTER TABLE users ADD COLUMN IF NOT EXISTS age_review_at timestamptz;
CREATE INDEX IF NOT EXISTS users_age_review ON users (age_review_at) WHERE age_review_at IS NOT NULL;
