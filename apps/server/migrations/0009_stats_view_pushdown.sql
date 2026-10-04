-- Capacity fix: the CTEs in user_game_stats were materialized, so every
-- `WHERE user_id = $1` lookup aggregated the whole sessions table (~280 ms at 400k
-- sessions). NOT MATERIALIZED lets Postgres push the user filter into each CTE
-- (~1 ms). Same columns and results.
CREATE OR REPLACE VIEW user_game_stats AS
WITH s AS NOT MATERIALIZED (
    SELECT user_id, game_id,
           sum(duration_s) / 60 AS session_min,
           sum(duration_s) FILTER (WHERE started_at >= now() - interval '7 days') / 60 AS min_7d,
           max(ended_at) AS last_played_at
    FROM sessions GROUP BY user_id, game_id
), st AS NOT MATERIALIZED (
    SELECT user_id, game_id, max(minutes_forever) AS steam_min, max(minutes_2weeks) AS steam_2w
    FROM steam_playtime GROUP BY user_id, game_id
), imp AS NOT MATERIALIZED (
    SELECT DISTINCT ON (user_id, game_id) user_id, game_id, minutes AS imported_min
    FROM hours_entries WHERE tier = 'imported'
    ORDER BY user_id, game_id, captured_at DESC, created_at DESC
), est AS NOT MATERIALIZED (
    SELECT user_id, game_id, sum(minutes) AS estimated_min
    FROM hours_entries WHERE tier = 'estimated' GROUP BY user_id, game_id
), keys AS NOT MATERIALIZED (
    SELECT user_id, game_id FROM s UNION SELECT user_id, game_id FROM st
    UNION SELECT user_id, game_id FROM imp UNION SELECT user_id, game_id FROM est
)
SELECT k.user_id, k.game_id, g.name,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0))::bigint AS tracked_min,
       coalesce(imp.imported_min, 0)::bigint AS imported_min,
       coalesce(est.estimated_min, 0)::bigint AS estimated_min,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0), coalesce(imp.imported_min, 0))::bigint AS display_min,
       CASE
         WHEN coalesce(imp.imported_min, 0) > GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0)) THEN 'imported'
         WHEN GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0)) > 0 THEN 'tracked'
         WHEN coalesce(est.estimated_min, 0) > 0 THEN 'estimated'
         ELSE 'tracked'
       END AS display_tier,
       coalesce(s.min_7d, 0)::bigint AS minutes_7d,
       s.last_played_at
FROM keys k
JOIN games g ON g.id = k.game_id
LEFT JOIN s   ON s.user_id = k.user_id AND s.game_id = k.game_id
LEFT JOIN st  ON st.user_id = k.user_id AND st.game_id = k.game_id
LEFT JOIN imp ON imp.user_id = k.user_id AND imp.game_id = k.game_id
LEFT JOIN est ON est.user_id = k.user_id AND est.game_id = k.game_id;

-- FK cascade on account delete scanned friendships for requested_by.
CREATE INDEX IF NOT EXISTS friendships_requested_by ON friendships (requested_by);
CREATE INDEX IF NOT EXISTS device_codes_approved_by ON device_codes (approved_by);
