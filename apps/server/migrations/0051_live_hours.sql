-- Live hours (Oct 2026). A game that is running counts in every total right away instead of
-- when it ends. play_sessions = the stored sessions plus each open session in presence, from
-- session_started_at to LEAST(now(), heartbeat_at + 45 s) (the same end presence.rs records),
-- while it is live: last heartbeat within 150 s (IDLE_TIMEOUT_SECS), at least 30 s long
-- (MIN_LIVE_SECS) and not hidden ("Show my game to my friends" off: those hours appear when
-- the session ends, as before, so nobody can watch them tick). An id staff mapped in Game
-- approvals counts as its target, as record_live_session stores it. presence.rs writes a
-- session row and clears session_started_at in one transaction, so nothing counts twice.
CREATE VIEW play_sessions AS
SELECT s.user_id, s.game_id, s.started_at, s.ended_at, s.duration_s, s.source, false AS open
FROM sessions s
UNION ALL
SELECT p.user_id, coalesce(m.mapped_to, p.game_id), p.session_started_at, e.ended_at,
       extract(epoch FROM e.ended_at - p.session_started_at)::integer, 'live'::text, true
FROM presence p
CROSS JOIN LATERAL (SELECT LEAST(now(), p.heartbeat_at + interval '45 seconds') AS ended_at) e
LEFT JOIN unknown_games m ON m.game_id = p.game_id AND m.status = 'mapped' AND m.mapped_to IS NOT NULL
WHERE p.game_id IS NOT NULL AND p.session_started_at IS NOT NULL AND NOT p.game_hidden
  AND p.status <> 'offline' AND p.heartbeat_at > now() - interval '150 seconds'
  AND e.ended_at - p.session_started_at >= interval '30 seconds';

CREATE OR REPLACE VIEW user_game_stats AS
WITH s AS NOT MATERIALIZED (
    SELECT user_id, game_id,
           sum(duration_s) / 60 AS session_min,
           sum(duration_s) FILTER (WHERE started_at >= now() - interval '7 days') / 60 AS min_7d,
           max(ended_at) AS last_played_at
    FROM play_sessions GROUP BY user_id, game_id
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
    JOIN play_sessions x ON x.user_id = t.user_id AND x.game_id = t.game_id AND x.ended_at > t.fetched_at
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
    JOIN play_sessions x ON x.user_id = i.user_id AND x.game_id = i.game_id AND x.ended_at > i.captured_at
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
