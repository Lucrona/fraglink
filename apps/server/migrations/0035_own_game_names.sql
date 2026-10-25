-- Player-chosen names for programs they added themselves (0.3.0-beta.6). Until beta.5 these
-- lived only in server memory, so after a restart the player's own views said "A game" until
-- the app reported the game again. Shown only to that player. Additive only.
CREATE TABLE IF NOT EXISTS own_game_names (
    user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    game_id    text NOT NULL,
    name       text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, game_id)
);
