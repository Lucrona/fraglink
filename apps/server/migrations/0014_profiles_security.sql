-- 0.2.0-dev.19 website v2: avatars, profile extras, privacy, two-step sign-in (TOTP),
-- email verification, sign-in history, account deletion grace period.

-- Avatars: 'upload' (files under FRAGLINK_DATA_DIR/avatars/<user>/<hash>-<px>.webp),
-- 'preset' (avatar_preset 0..11) or NULL (default F mark coloured by user id).
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_kind text CHECK (avatar_kind IS NULL OR avatar_kind IN ('upload', 'preset'));
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_hash text;        -- hash of the processed 184px file (cache key)
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_src_hash text;    -- sha256 of the uploaded bytes (blocklist key)
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_preset smallint CHECK (avatar_preset IS NULL OR avatar_preset BETWEEN 0 AND 11);
CREATE TABLE IF NOT EXISTS avatar_changes (
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    changed_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS avatar_changes_user ON avatar_changes (user_id, changed_at);
CREATE TABLE IF NOT EXISTS avatar_blocklist (
    src_hash   text PRIMARY KEY,
    blocked_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS avatar_reports (
    id          bigserial PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reporter_id uuid REFERENCES users(id) ON DELETE SET NULL,
    avatar_hash text NOT NULL,
    reason      text CHECK (char_length(reason) <= 200),
    created_at  timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    resolved_by uuid REFERENCES users(id) ON DELETE SET NULL,
    resolution  text CHECK (resolution IS NULL OR resolution IN ('removed', 'dismissed'))
);
CREATE INDEX IF NOT EXISTS avatar_reports_open ON avatar_reports (created_at) WHERE resolved_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS avatar_reports_once ON avatar_reports (user_id, reporter_id, avatar_hash);

-- Profile extras and privacy.
ALTER TABLE users ADD COLUMN IF NOT EXISTS rig jsonb NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE users ADD COLUMN IF NOT EXISTS about text CHECK (about IS NULL OR char_length(about) <= 300);
ALTER TABLE users ADD COLUMN IF NOT EXISTS location text CHECK (location IS NULL OR char_length(location) <= 40);
ALTER TABLE users ADD COLUMN IF NOT EXISTS gaming_style text CHECK (gaming_style IS NULL OR char_length(gaming_style) <= 30);
-- Off by default: profile link viewable without signing in, listed on the public home page,
-- signature image enabled.
ALTER TABLE users ADD COLUMN IF NOT EXISTS public_profile boolean NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN IF NOT EXISTS show_friends boolean NOT NULL DEFAULT true;
ALTER TABLE users ADD COLUMN IF NOT EXISTS notify_prefs jsonb NOT NULL DEFAULT '{}'::jsonb;
CREATE TABLE IF NOT EXISTS hidden_games (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id text NOT NULL,
    PRIMARY KEY (user_id, game_id)
);

-- Email verification (links are logged until an email provider is switched on).
CREATE TABLE IF NOT EXISTS email_tokens (
    token_hash bytea PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose    text NOT NULL CHECK (purpose IN ('verify', 'reset')),
    email      text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    used_at    timestamptz
);

-- Two-step sign-in (TOTP, RFC 6238) and one-time backup codes (SHA-256 only).
CREATE TABLE IF NOT EXISTS user_mfa (
    user_id    uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret     bytea NOT NULL,
    enabled_at timestamptz,
    last_step  bigint NOT NULL DEFAULT 0,   -- last accepted 30 s step (no code reuse)
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS mfa_backup_codes (
    user_id   uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash bytea NOT NULL,
    used_at   timestamptz,
    PRIMARY KEY (user_id, code_hash)
);
-- Pending second step after a correct password (5 minutes).
CREATE TABLE IF NOT EXISTS mfa_tickets (
    ticket_hash bytea PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        text NOT NULL CHECK (kind IN ('web', 'device')),
    device_name text,
    attempts    integer NOT NULL DEFAULT 0,
    ip          text,
    user_agent  text,
    expires_at  timestamptz NOT NULL
);

-- Where each sign-in came from (Security tab, new sign-in emails).
ALTER TABLE auth_tokens ADD COLUMN IF NOT EXISTS ip text;
ALTER TABLE auth_tokens ADD COLUMN IF NOT EXISTS user_agent text;
ALTER TABLE auth_tokens ADD COLUMN IF NOT EXISTS location text;

-- Account deletion: 7-day grace period, then purged. Signing in again cancels it.
ALTER TABLE users ADD COLUMN IF NOT EXISTS delete_after timestamptz;
