-- Email changes are stored unverified until there's an email provider to confirm them.
ALTER TABLE users ADD COLUMN email_verified boolean NOT NULL DEFAULT false;
