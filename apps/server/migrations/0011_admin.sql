-- Staff tools (admin section of the website).
--
-- users.role: NULL for everyone except staff. Exactly one owner (the master account,
-- can't be removed, demoted or banned); admins; moderators.
ALTER TABLE users ADD COLUMN IF NOT EXISTS role text
    CHECK (role IS NULL OR role IN ('owner', 'admin', 'moderator'));
CREATE UNIQUE INDEX IF NOT EXISTS users_single_owner ON users ((true)) WHERE role = 'owner';
UPDATE users SET role = 'owner'
 WHERE lower(handle) = 'lucrona' AND deleted_at IS NULL
   AND NOT EXISTS (SELECT 1 FROM users WHERE role = 'owner');

-- Banned accounts can't sign in and their profile is hidden.
ALTER TABLE users ADD COLUMN IF NOT EXISTS banned_at timestamptz;
ALTER TABLE users ADD COLUMN IF NOT EXISTS ban_reason text CHECK (char_length(ban_reason) <= 200);

-- Every staff action: who, what, when, before/after.
CREATE TABLE IF NOT EXISTS admin_audit (
    id           bigserial PRIMARY KEY,
    actor_id     uuid REFERENCES users(id) ON DELETE SET NULL,
    actor_handle text NOT NULL,
    action       text NOT NULL,
    target       text,
    before       jsonb,
    after        jsonb,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS admin_audit_time ON admin_audit (created_at DESC);

-- Staff edits to a game, applied on top of the daily games list (so they survive
-- the refresh): display name, hidden from public, extra exe names, uploaded icon.
CREATE TABLE IF NOT EXISTS game_overrides (
    game_id    text PRIMARY KEY,
    name       text CHECK (name IS NULL OR char_length(name) BETWEEN 1 AND 80),
    hidden     boolean NOT NULL DEFAULT false,
    extra_exes text[] NOT NULL DEFAULT '{}',
    has_icon   boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid REFERENCES users(id) ON DELETE SET NULL
);

-- Site banner (and later in the app). At most one is shown: the newest active one.
CREATE TABLE IF NOT EXISTS announcements (
    id         bigserial PRIMARY KEY,
    message    text NOT NULL CHECK (char_length(message) BETWEEN 1 AND 300),
    style      text NOT NULL DEFAULT 'info' CHECK (style IN ('info', 'warning')),
    link       text CHECK (link IS NULL OR char_length(link) <= 300),
    ends_at    timestamptz,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    cleared_at timestamptz
);
