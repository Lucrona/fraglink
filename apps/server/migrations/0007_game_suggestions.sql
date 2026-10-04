-- "Track it as a game?" answers from the desktop app (0.2.0-dev.12): exe + name only,
-- no user id. Reviewed by hand; nothing here is published automatically.
CREATE TABLE game_suggestions (
    id        bigserial PRIMARY KEY,
    exe       text NOT NULL CHECK (length(exe) BETWEEN 1 AND 120),
    name      text NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    reports   integer NOT NULL DEFAULT 1,
    status    text NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'accepted', 'rejected')),
    first_at  timestamptz NOT NULL DEFAULT now(),
    last_at   timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX game_suggestions_exe_name ON game_suggestions ((lower(exe)), (lower(name)));
