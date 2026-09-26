-- M3: friends, blocks and presence (spec §8.1 friendships/blocks/presence).

-- One row per pair, user_a < user_b. Pending until the other side accepts.
CREATE TABLE friendships (
    user_a       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_b       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status       text NOT NULL CHECK (status IN ('pending', 'accepted')),
    requested_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    accepted_at  timestamptz,
    PRIMARY KEY (user_a, user_b),
    CHECK (user_a < user_b),
    CHECK (requested_by = user_a OR requested_by = user_b)
);
CREATE INDEX friendships_user_b ON friendships (user_b);

-- One-directional blocks. Blocking also removes any friendship/request.
CREATE TABLE blocks (
    blocker    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (blocker, blocked),
    CHECK (blocker <> blocked)
);
CREATE INDEX blocks_blocked ON blocks (blocked);

-- Last known presence. `invisible` is stored as chosen but shown to others as offline.
CREATE TABLE presence (
    user_id            uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    status             text NOT NULL CHECK (status IN ('online', 'away', 'invisible', 'offline')),
    game_id            text,
    game_name          text,
    session_started_at timestamptz,
    heartbeat_at       timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);
