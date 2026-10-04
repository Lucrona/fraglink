-- 0.2.0-dev.14: hours come only from live presence, and game names are the server's.
--
-- games.verified: the name may be shown to other people. True for names from Steam
-- (Web API import or store lookup) and for "Track this as a game?" programs an admin
-- approved. Names that came from a client (old session uploads, typed estimates) stay
-- unverified and are shown as "Unlisted game" / "a game".
ALTER TABLE games ADD COLUMN IF NOT EXISTS verified boolean NOT NULL DEFAULT false;
UPDATE games SET verified = true WHERE steam_icon_hash IS NOT NULL;

-- "Show my game to my friends" off: the game is still recorded for the user's own
-- hours but hidden from everyone else.
ALTER TABLE presence ADD COLUMN IF NOT EXISTS game_hidden boolean NOT NULL DEFAULT false;
