-- Game detail next to the game name, e.g. the World of Warcraft realm (0.2.0-dev.12).
-- Nullable and optional: older clients never send it.
ALTER TABLE presence ADD COLUMN game_detail text;
