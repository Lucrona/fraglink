-- Usernames (users.handle, permanent; staff can change them) and nicknames (users.display_name,
-- what people see; the owner changes it). Existing display names are the nicknames.
UPDATE users SET display_name = handle WHERE display_name IS NULL OR btrim(display_name) = '';

-- Earlier nicknames, newest last; profiles show the last few ("Previously known as").
CREATE TABLE IF NOT EXISTS name_history (
    id          bigserial PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        text NOT NULL,
    changed_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS name_history_user ON name_history (user_id, changed_at DESC);

-- Old usernames after a staff rename: old profile links redirect, and nobody else can take them.
CREATE TABLE IF NOT EXISTS handle_redirects (
    old_handle  text PRIMARY KEY,          -- lower-case
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now()
);
