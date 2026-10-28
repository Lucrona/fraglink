-- Welcome email: sent once, when a new account first confirms its email. Every account that
-- exists before this migration counts as already sent, so nobody existing gets it. Additive only.
ALTER TABLE users ADD COLUMN IF NOT EXISTS welcome_sent_at timestamptz;
UPDATE users SET welcome_sent_at = now() WHERE welcome_sent_at IS NULL;
