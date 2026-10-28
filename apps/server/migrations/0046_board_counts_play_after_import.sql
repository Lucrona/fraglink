-- App-tracked play is legit (Oct 2026): the all-time leaderboard now uses the same rule as the
-- shown total, the latest import + tracked minutes after it. board_min stays as a column but
-- equals display_min. Estimated hours were dropped in 0025 and never count.
CREATE OR REPLACE VIEW user_game_stats AS
WITH s AS NOT MATERIALIZED (
    SELECT user_id, game_id,
           sum(duration_s) / 60 AS session_min,
           sum(duration_s) FILTER (WHERE started_at >= now() - interval '7 days') / 60 AS min_7d,
           max(ended_at) AS last_played_at
    FROM sessions GROUP BY user_id, game_id
), st AS NOT MATERIALIZED (
    SELECT user_id, game_id, max(minutes_forever) AS steam_min, max(minutes_2weeks) AS steam_2w,
           max(fetched_at) AS fetched_at
    FROM steam_playtime GROUP BY user_id, game_id
), st_after AS NOT MATERIALIZED (
    SELECT t.user_id, t.game_id,
           (sum(LEAST(x.duration_s::numeric,
                      extract(epoch FROM x.ended_at - GREATEST(x.started_at, t.fetched_at))))
            / 60)::bigint AS after_min
    FROM st t
    JOIN sessions x ON x.user_id = t.user_id AND x.game_id = t.game_id AND x.ended_at > t.fetched_at
    GROUP BY t.user_id, t.game_id
), imp AS NOT MATERIALIZED (
    SELECT DISTINCT ON (user_id, game_id) user_id, game_id, minutes AS imported_min, captured_at
    FROM hours_entries WHERE tier = 'imported'
    ORDER BY user_id, game_id, captured_at DESC, created_at DESC
), after AS NOT MATERIALIZED (
    SELECT i.user_id, i.game_id,
           (sum(LEAST(x.duration_s::numeric,
                      extract(epoch FROM x.ended_at - GREATEST(x.started_at, i.captured_at))))
            / 60)::bigint AS after_min
    FROM imp i
    JOIN sessions x ON x.user_id = i.user_id AND x.game_id = i.game_id AND x.ended_at > i.captured_at
    GROUP BY i.user_id, i.game_id
), keys AS NOT MATERIALIZED (
    SELECT user_id, game_id FROM s UNION SELECT user_id, game_id FROM st
    UNION SELECT user_id, game_id FROM imp
)
SELECT k.user_id, k.game_id, g.name,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min + coalesce(sa.after_min, 0), 0))::bigint AS tracked_min,
       coalesce(imp.imported_min, 0)::bigint AS imported_min,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min + coalesce(sa.after_min, 0), 0),
                CASE WHEN imp.imported_min IS NULL THEN 0
                     ELSE imp.imported_min + coalesce(a.after_min, 0) END)::bigint AS display_min,
       CASE
         WHEN coalesce(imp.imported_min, 0) > GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min + coalesce(sa.after_min, 0), 0)) THEN 'imported'
         ELSE 'tracked'
       END AS display_tier,
       coalesce(s.min_7d, 0)::bigint AS minutes_7d,
       s.last_played_at,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min + coalesce(sa.after_min, 0), 0),
                CASE WHEN imp.imported_min IS NULL THEN 0
                     ELSE imp.imported_min + coalesce(a.after_min, 0) END)::bigint AS board_min
FROM keys k
JOIN games g ON g.id = k.game_id
LEFT JOIN s     ON s.user_id = k.user_id AND s.game_id = k.game_id
LEFT JOIN st    ON st.user_id = k.user_id AND st.game_id = k.game_id
LEFT JOIN imp   ON imp.user_id = k.user_id AND imp.game_id = k.game_id
LEFT JOIN st_after sa ON sa.user_id = k.user_id AND sa.game_id = k.game_id
LEFT JOIN after a ON a.user_id = k.user_id AND a.game_id = k.game_id;
