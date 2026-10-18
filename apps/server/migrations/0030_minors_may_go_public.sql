-- Under-18 accounts stay private by default at sign-up, but may switch to a public
-- profile themselves. Drop the CHECK that forbade is_minor AND public_profile.
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_minor_private;
