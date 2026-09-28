-- Email is optional (no email provider yet): accounts made in the desktop client may have ''.
-- Only non-empty emails have to be unique.
DROP INDEX IF EXISTS users_email_key;
CREATE UNIQUE INDEX users_email_key ON users (lower(email)) WHERE email <> '';
