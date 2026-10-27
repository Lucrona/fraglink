-- Keep menu time (Oct 2026). WoW /played doesn't count menus or loading screens, but the app
-- does. When a higher WoW upload is stored, the app-tracked minutes since the previous upload
-- beyond what /played gained are carried forward: carry_min is the running extra (it resets
-- on a recount, an unlink, or a lower stored total). Shown total = latest import + carry_min
-- + tracked play after it. board_min = display_min.
ALTER TABLE hours_entries ADD COLUMN carry_min integer NOT NULL DEFAULT 0;

-- Backfill existing WoW uploads with the same rule, computed from the stored data. One upload
-- used to store a row per character within seconds, so rows less than 2 minutes apart are
-- one upload and only its final total is compared with the previous upload's.
WITH e AS (
    SELECT id, user_id, game_id, minutes, captured_at, created_at,
           coalesce(inputs->>'reset', '') = 'true' AS is_reset,
           (lag(captured_at) OVER w IS NULL
            OR captured_at - lag(captured_at) OVER w > interval '2 minutes') AS starts
    FROM hours_entries WHERE tier = 'imported' AND game_id LIKE 'wow%'  -- every WoW flavour
    WINDOW w AS (PARTITION BY user_id, game_id ORDER BY captured_at, created_at)
), eb AS (
    SELECT *, sum(starts::int) OVER (PARTITION BY user_id, game_id ORDER BY captured_at, created_at) AS burst
    FROM e
), b AS (
    SELECT DISTINCT ON (user_id, game_id, burst) user_id, game_id, burst, minutes, captured_at,
           bool_or(is_reset) OVER (PARTITION BY user_id, game_id, burst) AS any_reset
    FROM eb ORDER BY user_id, game_id, burst, captured_at DESC, created_at DESC
), bl AS (
    SELECT *, lag(minutes) OVER w AS pm, lag(captured_at) OVER w AS pt
    FROM b WINDOW w AS (PARTITION BY user_id, game_id ORDER BY burst)
), f AS (
    SELECT bl.*, (pm IS NULL OR minutes < pm OR any_reset) AS rs,
           coalesce((SELECT (sum(GREATEST(0, LEAST(x.duration_s::numeric,
                        extract(epoch FROM LEAST(x.ended_at, bl.captured_at) - GREATEST(x.started_at, bl.pt))))) / 60)::bigint
                     FROM sessions x WHERE x.user_id = bl.user_id AND x.game_id = bl.game_id
                       AND x.ended_at > bl.pt AND x.started_at < bl.captured_at), 0) AS tracked
    FROM bl
), g AS (
    SELECT user_id, game_id, burst,
           CASE WHEN rs OR minutes = pm THEN 0 ELSE GREATEST(0, tracked - (minutes - pm)) END AS inc,
           sum(rs::int) OVER (PARTITION BY user_id, game_id ORDER BY burst) AS grp
    FROM f
), c AS (
    SELECT user_id, game_id, burst,
           sum(inc) OVER (PARTITION BY user_id, game_id, grp ORDER BY burst) AS carry
    FROM g
)
UPDATE hours_entries h SET carry_min = LEAST(c.carry, 2147483647)::integer
FROM eb JOIN c USING (user_id, game_id, burst)
WHERE h.id = eb.id AND c.carry > 0;

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
    SELECT DISTINCT ON (user_id, game_id) user_id, game_id, minutes AS imported_min, carry_min, captured_at
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
                     ELSE imp.imported_min + imp.carry_min + coalesce(a.after_min, 0) END)::bigint AS display_min,
       CASE
         WHEN coalesce(imp.imported_min, 0) > GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min + coalesce(sa.after_min, 0), 0)) THEN 'imported'
         ELSE 'tracked'
       END AS display_tier,
       coalesce(s.min_7d, 0)::bigint AS minutes_7d,
       s.last_played_at,
       GREATEST(coalesce(s.session_min, 0), coalesce(st.steam_min + coalesce(sa.after_min, 0), 0),
                CASE WHEN imp.imported_min IS NULL THEN 0
                     ELSE imp.imported_min + imp.carry_min + coalesce(a.after_min, 0) END)::bigint AS board_min
FROM keys k
JOIN games g ON g.id = k.game_id
LEFT JOIN s     ON s.user_id = k.user_id AND s.game_id = k.game_id
LEFT JOIN st    ON st.user_id = k.user_id AND st.game_id = k.game_id
LEFT JOIN imp   ON imp.user_id = k.user_id AND imp.game_id = k.game_id
LEFT JOIN st_after sa ON sa.user_id = k.user_id AND sa.game_id = k.game_id
LEFT JOIN after a ON a.user_id = k.user_id AND a.game_id = k.game_id;
