//! Local SQLite store (WAL). Everything Fraglink records lives here, in the
//! app data folder. Sessions are never uploaded: the server records hours from live
//! presence while the app is signed in.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS games(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source TEXT NOT NULL,               -- signature | steam | manual
  steam_appid INTEGER,
  excluded INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions(
  id INTEGER PRIMARY KEY,
  uuid TEXT NOT NULL UNIQUE,          -- client-generated, idempotent for later upload (M2)
  game_id TEXT NOT NULL REFERENCES games(id),
  started_at INTEGER NOT NULL,        -- unix seconds (UTC)
  ended_at INTEGER,                   -- NULL while open
  last_checkpoint_at INTEGER NOT NULL,
  active_seconds INTEGER NOT NULL DEFAULT 0,
  idle_trimmed_seconds INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'tracked',
  detect_source TEXT,
  exe_name TEXT,
  status TEXT NOT NULL CHECK(status IN ('open','closed')),
  client_version TEXT
);
CREATE INDEX IF NOT EXISTS sessions_game ON sessions(game_id, ended_at);
CREATE TABLE IF NOT EXISTS session_days(
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  day TEXT NOT NULL,                  -- local date YYYY-MM-DD
  game_id TEXT NOT NULL,
  active_seconds INTEGER NOT NULL,
  PRIMARY KEY(session_id, day)
);
CREATE INDEX IF NOT EXISTS session_days_day ON session_days(day);
CREATE VIEW IF NOT EXISTS daily_totals AS
  SELECT day, game_id, SUM(active_seconds) AS active_seconds, COUNT(DISTINCT session_id) AS sessions
  FROM session_days GROUP BY day, game_id;
CREATE TABLE IF NOT EXISTS manual_tags(
  exe_name TEXT PRIMARY KEY,          -- lower-case
  game_id TEXT NOT NULL REFERENCES games(id),
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS excluded_exes(exe_name TEXT PRIMARY KEY, created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS activity(
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,                 -- started | stopped | first_time | milestone | tagged | excluded
  game_id TEXT,
  session_id INTEGER,
  payload TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS activity_ts ON activity(ts);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GameRow {
    pub id: String,
    pub name: String,
    pub source: String,
    pub steam_appid: Option<u32>,
    pub excluded: bool,
    pub total_seconds: i64,
    pub sessions: i64,
    pub last_played: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionRow {
    pub id: i64,
    pub uuid: String,
    pub game_id: String,
    pub game_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub last_checkpoint_at: i64,
    pub active_seconds: i64,
    pub idle_trimmed_seconds: i64,
    pub source: String,
    pub detect_source: Option<String>,
    pub exe_name: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActivityRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub session_id: Option<i64>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DayRow {
    pub session_id: i64,
    pub day: String,
    pub game_id: String,
    pub active_seconds: i64,
}

pub struct Store {
    conn: Connection,
}

const SESSION_COLS: &str = "s.id, s.uuid, s.game_id, COALESCE(g.name, s.game_id), s.started_at, s.ended_at, s.last_checkpoint_at, s.active_seconds, s.idle_trimmed_seconds, s.source, s.detect_source, s.exe_name, s.status";

fn session_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: r.get(0)?,
        uuid: r.get(1)?,
        game_id: r.get(2)?,
        game_name: r.get(3)?,
        started_at: r.get(4)?,
        ended_at: r.get(5)?,
        last_checkpoint_at: r.get(6)?,
        active_seconds: r.get(7)?,
        idle_trimmed_seconds: r.get(8)?,
        source: r.get(9)?,
        detect_source: r.get(10)?,
        exe_name: r.get(11)?,
        status: r.get(12)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn)?;
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(Store { conn })
    }

    /// Additive, idempotent migrations for databases created by older versions.
    fn migrate(conn: &Connection) -> Result<()> {
        let has_col = |table: &str, col: &str| -> Result<bool> {
            let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let names = st
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(names.iter().any(|n| n == col))
        };
        // v2 (0.2.0): sync outbox state per session. The upload path is gone (0.3.0-beta.3);
        // the two columns stay so existing databases need no migration, but nothing uses them.
        if !has_col("sessions", "synced_at")? {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN synced_at INTEGER;
                 ALTER TABLE sessions ADD COLUMN sync_error TEXT;",
            )?;
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ------------------------------------------------------------ games
    pub fn upsert_game(
        &self,
        id: &str,
        name: &str,
        source: &str,
        steam_appid: Option<u32>,
        now: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO games(id, name, source, steam_appid, created_at) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, steam_appid = COALESCE(excluded.steam_appid, games.steam_appid)
             WHERE games.source != 'manual'",
            params![id, name, source, steam_appid, now],
        )?;
        Ok(())
    }

    /// Rename a known game (not one the user named by hand); no row is created.
    pub fn rename_game(&self, id: &str, name: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE games SET name = ?2 WHERE id = ?1 AND source != 'manual' AND name != ?2",
            params![id, name],
        )?;
        Ok(n > 0)
    }

    pub fn game_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT name FROM games WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_game_excluded(&self, id: &str, excluded: bool) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE games SET excluded = ?2 WHERE id = ?1",
            params![id, excluded as i64],
        )?;
        Ok(n > 0)
    }

    pub fn excluded_game_ids(&self) -> Result<HashSet<String>> {
        let mut st = self
            .conn
            .prepare("SELECT id FROM games WHERE excluded = 1")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_games(&self) -> Result<Vec<GameRow>> {
        let mut st = self.conn.prepare(
            "SELECT g.id, g.name, g.source, g.steam_appid, g.excluded,
                    COALESCE(SUM(s.active_seconds), 0), COUNT(s.id), MAX(COALESCE(s.ended_at, s.last_checkpoint_at))
             FROM games g LEFT JOIN sessions s ON s.game_id = g.id
             GROUP BY g.id ORDER BY 6 DESC, g.name",
        )?;
        let rows = st.query_map([], |r| {
            Ok(GameRow {
                id: r.get(0)?,
                name: r.get(1)?,
                source: r.get(2)?,
                steam_appid: r.get(3)?,
                excluded: r.get::<_, i64>(4)? != 0,
                total_seconds: r.get(5)?,
                sessions: r.get(6)?,
                last_played: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ------------------------------------------------------------ tags / exclusions
    pub fn add_manual_tag(&self, exe: &str, game_id: &str, name: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO games(id, name, source, created_at) VALUES(?1, ?2, 'manual', ?3)
             ON CONFLICT(id) DO UPDATE SET excluded = 0,
                 source = CASE WHEN games.source = 'removed' THEN 'manual' ELSE games.source END",
            params![game_id, name, now],
        )?;
        self.conn.execute(
            "INSERT INTO manual_tags(exe_name, game_id, created_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(exe_name) DO UPDATE SET game_id = excluded.game_id",
            params![exe.to_lowercase(), game_id, now],
        )?;
        self.conn.execute(
            "DELETE FROM excluded_exes WHERE exe_name = ?1",
            params![exe.to_lowercase()],
        )?;
        Ok(())
    }

    pub fn remove_manual_tag(&self, exe: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM manual_tags WHERE exe_name = ?1",
            params![exe.to_lowercase()],
        )?;
        Ok(())
    }

    /// beta.5: "Remove" on a game the user added: stop detecting it and hide it from
    /// Options > Games. The row and its sessions stay (hours live on the server anyway).
    pub fn remove_manual_game(&self, game_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM manual_tags WHERE game_id = ?1",
            params![game_id],
        )?;
        self.conn.execute(
            "UPDATE games SET source = 'removed' WHERE id = ?1 AND source = 'manual'",
            params![game_id],
        )?;
        Ok(())
    }

    pub fn manual_tags(&self) -> Result<HashMap<String, (String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT t.exe_name, t.game_id, COALESCE(g.name, t.game_id) FROM manual_tags t LEFT JOIN games g ON g.id = t.game_id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn exclude_exe(&self, exe: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO excluded_exes(exe_name, created_at) VALUES(?1, ?2) ON CONFLICT DO NOTHING",
            params![exe.to_lowercase(), now],
        )?;
        Ok(())
    }

    pub fn include_exe(&self, exe: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM excluded_exes WHERE exe_name = ?1",
            params![exe.to_lowercase()],
        )?;
        Ok(())
    }

    pub fn excluded_exes(&self) -> Result<HashSet<String>> {
        let mut st = self.conn.prepare("SELECT exe_name FROM excluded_exes")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ------------------------------------------------------------ sessions
    #[allow(clippy::too_many_arguments)]
    pub fn open_session(
        &self,
        uuid: &str,
        game_id: &str,
        started_at: i64,
        detect_source: &str,
        exe: Option<&str>,
        client_version: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions(uuid, game_id, started_at, last_checkpoint_at, detect_source, exe_name, status, client_version)
             VALUES(?1, ?2, ?3, ?3, ?4, ?5, 'open', ?6)",
            params![uuid, game_id, started_at, detect_source, exe, client_version],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn write_days(
        tx: &rusqlite::Transaction<'_>,
        id: i64,
        game_id: &str,
        days: &BTreeMap<String, i64>,
    ) -> Result<()> {
        tx.execute(
            "DELETE FROM session_days WHERE session_id = ?1",
            params![id],
        )?;
        let mut st = tx.prepare("INSERT INTO session_days(session_id, day, game_id, active_seconds) VALUES(?1, ?2, ?3, ?4)")?;
        for (day, secs) in days {
            if *secs > 0 {
                st.execute(params![id, day, game_id, secs])?;
            }
        }
        Ok(())
    }

    /// Persist the running totals of an open session (crash safety).
    pub fn checkpoint_session(
        &mut self,
        id: i64,
        game_id: &str,
        now: i64,
        active: i64,
        trimmed: i64,
        days: &BTreeMap<String, i64>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE sessions SET last_checkpoint_at = ?2, active_seconds = ?3, idle_trimmed_seconds = ?4 WHERE id = ?1",
            params![id, now, active, trimmed],
        )?;
        Self::write_days(&tx, id, game_id, days)?;
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn close_session(
        &mut self,
        id: i64,
        game_id: &str,
        ended_at: i64,
        active: i64,
        trimmed: i64,
        days: &BTreeMap<String, i64>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE sessions SET ended_at = ?2, last_checkpoint_at = ?2, active_seconds = ?3, idle_trimmed_seconds = ?4, status = 'closed' WHERE id = ?1",
            params![id, ended_at, active, trimmed],
        )?;
        Self::write_days(&tx, id, game_id, days)?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM activity WHERE session_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// The most recent session overall, if it is a closed session of `game_id`
    /// that ended at or after `since` (i.e. no other game was played in between).
    pub fn recent_closed_session(&self, game_id: &str, since: i64) -> Result<Option<SessionRow>> {
        let sql = format!(
            "SELECT {SESSION_COLS} FROM sessions s LEFT JOIN games g ON g.id = s.game_id
             ORDER BY s.started_at DESC, s.id DESC LIMIT 1"
        );
        let last = self.conn.query_row(&sql, [], session_from_row).optional()?;
        Ok(last.filter(|s| {
            s.game_id == game_id && s.status == "closed" && s.ended_at.is_some_and(|e| e >= since)
        }))
    }

    pub fn reopen_session(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET status = 'open', ended_at = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn session(&self, id: i64) -> Result<Option<SessionRow>> {
        let sql = format!("SELECT {SESSION_COLS} FROM sessions s LEFT JOIN games g ON g.id = s.game_id WHERE s.id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![id], session_from_row)
            .optional()?)
    }

    pub fn session_days(&self, id: i64) -> Result<BTreeMap<String, i64>> {
        let mut st = self
            .conn
            .prepare("SELECT day, active_seconds FROM session_days WHERE session_id = ?1")?;
        let rows = st.query_map(params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn open_sessions(&self) -> Result<Vec<SessionRow>> {
        let sql = format!("SELECT {SESSION_COLS} FROM sessions s LEFT JOIN games g ON g.id = s.game_id WHERE s.status = 'open' ORDER BY s.id");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn all_sessions(&self) -> Result<Vec<SessionRow>> {
        let sql = format!("SELECT {SESSION_COLS} FROM sessions s LEFT JOIN games g ON g.id = s.game_id ORDER BY s.started_at");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn all_session_days(&self) -> Result<Vec<DayRow>> {
        let mut st = self.conn.prepare("SELECT session_id, day, game_id, active_seconds FROM session_days ORDER BY day, session_id")?;
        let rows = st.query_map([], |r| {
            Ok(DayRow {
                session_id: r.get(0)?,
                day: r.get(1)?,
                game_id: r.get(2)?,
                active_seconds: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Total active seconds for a game over all sessions (optionally excluding one).
    pub fn game_total_seconds(&self, game_id: &str, exclude_session: Option<i64>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(active_seconds), 0) FROM sessions WHERE game_id = ?1 AND id != COALESCE(?2, -1)",
            params![game_id, exclude_session],
            |r| r.get(0),
        )?)
    }

    pub fn game_session_count(&self, game_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE game_id = ?1",
            params![game_id],
            |r| r.get(0),
        )?)
    }

    /// Per-day, per-game seconds in `[from, to]` (inclusive, YYYY-MM-DD), excluded games removed.
    pub fn day_totals(&self, from: &str, to: &str) -> Result<Vec<(String, String, i64)>> {
        let mut st = self.conn.prepare(
            "SELECT d.day, d.game_id, d.active_seconds FROM daily_totals d
             LEFT JOIN games g ON g.id = d.game_id
             WHERE d.day >= ?1 AND d.day <= ?2 AND COALESCE(g.excluded, 0) = 0",
        )?;
        let rows = st.query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Distinct days with at least `min_seconds` of play, newest first (excluded games removed).
    pub fn played_days_desc(&self, min_seconds: i64, limit: i64) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT d.day FROM daily_totals d LEFT JOIN games g ON g.id = d.game_id
             WHERE COALESCE(g.excluded, 0) = 0 GROUP BY d.day HAVING SUM(d.active_seconds) >= ?1
             ORDER BY d.day DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![min_seconds, limit], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ------------------------------------------------------------ activity
    pub fn add_activity(
        &self,
        ts: i64,
        kind: &str,
        game_id: Option<&str>,
        session_id: Option<i64>,
        payload: &serde_json::Value,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO activity(ts, kind, game_id, session_id, payload) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![ts, kind, game_id, session_id, payload.to_string()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn activity_query(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<ActivityRow>> {
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map(p, |r| {
            let payload: String = r.get(6)?;
            Ok(ActivityRow {
                id: r.get(0)?,
                ts: r.get(1)?,
                kind: r.get(2)?,
                game_id: r.get(3)?,
                game_name: r.get(4)?,
                session_id: r.get(5)?,
                payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_activity(&self, session_id: i64, kind: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM activity WHERE session_id = ?1 AND kind = ?2",
            params![session_id, kind],
        )?;
        Ok(())
    }

    pub fn recent_activity(&self, limit: i64) -> Result<Vec<ActivityRow>> {
        self.activity_query(
            "SELECT a.id, a.ts, a.kind, a.game_id, g.name, a.session_id, a.payload FROM activity a
             LEFT JOIN games g ON g.id = a.game_id WHERE COALESCE(g.excluded, 0) = 0
             ORDER BY a.ts DESC, a.id DESC LIMIT ?1",
            params![limit],
        )
    }

    pub fn all_activity(&self) -> Result<Vec<ActivityRow>> {
        self.activity_query(
            "SELECT a.id, a.ts, a.kind, a.game_id, g.name, a.session_id, a.payload FROM activity a
             LEFT JOIN games g ON g.id = a.game_id ORDER BY a.ts, a.id",
            [],
        )
    }

    // ------------------------------------------------------------ settings
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<BTreeMap<String, String>> {
        let mut st = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_idempotent_and_file_backed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub/fraglink.db");
        {
            let s = Store::open(&path).unwrap();
            s.upsert_game("wow", "World of Warcraft", "signature", None, 1)
                .unwrap();
            s.set_setting("idle_threshold_min", "15").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(
            s.game_name("wow").unwrap().as_deref(),
            Some("World of Warcraft")
        );
        assert_eq!(
            s.get_setting("idle_threshold_min").unwrap().as_deref(),
            Some("15")
        );
        let mode: String = s
            .conn()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn removed_manual_game_hides_and_can_come_back() {
        let s = Store::open_in_memory().unwrap();
        s.add_manual_tag("GrokBot.exe", "manual:grok-bot", "Grok Bot", 1)
            .unwrap();
        s.remove_manual_game("manual:grok-bot").unwrap();
        assert!(s.manual_tags().unwrap().is_empty());
        let sigs = crate::signatures::SignatureDb::builtin();
        let c = crate::catalog::build(
            &sigs,
            &Default::default(),
            &Default::default(),
            &s.list_games().unwrap(),
            &Default::default(),
        );
        assert!(!c.iter().any(|e| e.id == "manual:grok-bot"));
        s.add_manual_tag("GrokBot.exe", "manual:grok-bot", "Grok Bot", 2)
            .unwrap();
        assert_eq!(s.manual_tags().unwrap().len(), 1);
        assert!(s
            .list_games()
            .unwrap()
            .iter()
            .any(|g| g.id == "manual:grok-bot" && g.source == "manual"));
    }

    #[test]
    fn manual_game_names_are_not_overwritten() {
        let s = Store::open_in_memory().unwrap();
        s.add_manual_tag("MyGame.exe", "manual:mygame", "My Game", 1)
            .unwrap();
        s.upsert_game("manual:mygame", "Other", "signature", None, 2)
            .unwrap();
        assert_eq!(
            s.game_name("manual:mygame").unwrap().as_deref(),
            Some("My Game")
        );
        let tags = s.manual_tags().unwrap();
        assert_eq!(tags.get("mygame.exe").unwrap().1, "My Game");
    }
}
