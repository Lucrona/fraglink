-- 0.3.0-beta.4: staff fix-ups for game detection, edited in Admin > Detection and
-- sent to every app with the override rules (GET /api/v1/games/overrides).
--   steam: pattern = Steam app id; that app counts as game_id (app + Steam sync)
--   id:    pattern = a game id; it counts as game_id
--   path:  pattern = folder + exe, e.g. 'world of warcraft/_retail_/wow.exe'
CREATE TABLE detection_aliases (
    id          bigserial PRIMARY KEY,
    kind        text NOT NULL CHECK (kind IN ('steam', 'id', 'path')),
    pattern     text NOT NULL,
    game_id     text NOT NULL,
    note        text,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX detection_aliases_kind_pattern ON detection_aliases (kind, lower(pattern));

INSERT INTO detection_aliases(kind, pattern, game_id, note)
VALUES ('steam', '793460', '112_operator', '112 Operator: the Steam copy is the same game');
