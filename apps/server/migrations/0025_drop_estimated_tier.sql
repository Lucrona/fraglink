-- The Estimated tier (history calculator) is gone (Oct 2026). Rows with tier 'estimated'
-- stay in hours_entries untouched, but the stats view no longer reads them, so they are
-- excluded from every total, ranking and display. Same columns as before minus
-- estimated_min; display_tier is 'tracked' or 'imported'.
DROP VIEW IF EXISTS user_game_stats;
CREATE VIEW user_game_stats AS
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
), keys AS NOT MATERIALIZED (
    SELECT user_id, game_id FROM s UNION SELECT user_id, game_id FROM st
    UNION SELECT user_id, game_id FROM imp
)
SELECT k.user_id, k.game_id, g.name,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0))::bigint AS tracked_min,
       coalesce(imp.imported_min, 0)::bigint AS imported_min,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0), coalesce(imp.imported_min, 0))::bigint AS display_min,
       CASE
         WHEN coalesce(imp.imported_min, 0) > GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min, 0)) THEN 'imported'
         ELSE 'tracked'
       END AS display_tier,
       coalesce(s.min_7d, 0)::bigint AS minutes_7d,
       s.last_played_at
FROM keys k
JOIN games g ON g.id = k.game_id
LEFT JOIN s   ON s.user_id = k.user_id AND s.game_id = k.game_id
LEFT JOIN st  ON st.user_id = k.user_id AND st.game_id = k.game_id
LEFT JOIN imp ON imp.user_id = k.user_id AND imp.game_id = k.game_id;
