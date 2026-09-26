//! Sync outbox: which closed sessions still need uploading (M2).
//!
//! No networking here. The desktop app's sync thread (crate `fraglink-sync`)
//! builds a batch with [`Store::pending_upload`], sends it, then records the
//! server's answer with [`Store::mark_uploaded`] / [`Store::mark_rejected`].
//! A session that changes after upload (a relaunch merge) is re-queued
//! automatically because closing/reopening clears `synced_at`.
//!
//! Only what the spec allows leaves the machine: game id + name + Steam app id,
//! session start/end, active and idle-trimmed seconds, client version. Never exe
//! names, paths or the process list.

use crate::store::Store;
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadGame {
    pub id: String,
    pub name: String,
    pub steam_appid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSession {
    /// Client-generated UUID: uploads are idempotent.
    pub id: String,
    pub game_id: String,
    /// Unix seconds (UTC).
    pub started_at: i64,
    pub ended_at: i64,
    /// Active (non-idle) seconds.
    pub duration_s: i64,
    pub idle_trimmed_s: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadBatch {
    pub client_version: String,
    pub games: Vec<UploadGame>,
    pub sessions: Vec<UploadSession>,
}

impl UploadBatch {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

impl Store {
    /// Closed, not-yet-uploaded sessions of games that are not excluded (oldest first).
    pub fn pending_upload(&self, limit: usize) -> Result<UploadBatch> {
        let mut st = self.conn().prepare(
            "SELECT s.uuid, s.game_id, s.started_at, s.ended_at, s.active_seconds, s.idle_trimmed_seconds,
                    COALESCE(g.name, s.game_id), g.steam_appid
             FROM sessions s JOIN games g ON g.id = s.game_id
             WHERE s.status = 'closed' AND s.ended_at IS NOT NULL
               AND s.synced_at IS NULL AND s.sync_error IS NULL AND g.excluded = 0
             ORDER BY s.started_at, s.id LIMIT ?1",
        )?;
        let mut games: Vec<UploadGame> = Vec::new();
        let rows = st.query_map(params![limit as i64], |r| {
            Ok((
                UploadSession {
                    id: r.get(0)?,
                    game_id: r.get(1)?,
                    started_at: r.get(2)?,
                    ended_at: r.get(3)?,
                    duration_s: r.get(4)?,
                    idle_trimmed_s: r.get(5)?,
                },
                UploadGame {
                    id: r.get(1)?,
                    name: r.get(6)?,
                    steam_appid: r.get::<_, Option<u32>>(7)?,
                },
            ))
        })?;
        let mut sessions = Vec::new();
        for row in rows {
            let (s, g) = row?;
            if !games.iter().any(|x| x.id == g.id) {
                games.push(g);
            }
            sessions.push(s);
        }
        Ok(UploadBatch {
            client_version: env!("CARGO_PKG_VERSION").into(),
            games,
            sessions,
        })
    }

    pub fn pending_upload_count(&self) -> Result<i64> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM sessions s JOIN games g ON g.id = s.game_id
             WHERE s.status = 'closed' AND s.synced_at IS NULL AND s.sync_error IS NULL AND g.excluded = 0",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn mark_uploaded(&self, uuids: &[String], at: i64) -> Result<()> {
        let mut st = self
            .conn()
            .prepare("UPDATE sessions SET synced_at = ?2, sync_error = NULL WHERE uuid = ?1")?;
        for u in uuids {
            st.execute(params![u, at])?;
        }
        Ok(())
    }

    /// The server refused this session for good (e.g. validation). Keep it locally, stop retrying.
    pub fn mark_rejected(&self, uuid: &str, reason: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE sessions SET sync_error = ?2 WHERE uuid = ?1",
            params![uuid, reason],
        )?;
        Ok(())
    }

    /// After signing out or switching accounts: everything is pending again.
    pub fn reset_sync_state(&self) -> Result<()> {
        self.conn().execute(
            "UPDATE sessions SET synced_at = NULL, sync_error = NULL",
            [],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::store::Store;
    use std::collections::BTreeMap;

    fn store_with_sessions() -> Store {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_game("cs2", "Counter-Strike 2", "signature", Some(730), 0)
            .unwrap();
        s.upsert_game("wow", "World of Warcraft", "signature", None, 0)
            .unwrap();
        let d = BTreeMap::new();
        let a = s
            .open_session("u-a", "cs2", 1000, "signature", Some("cs2.exe"), "0.2.0")
            .unwrap();
        s.close_session(a, "cs2", 2000, 900, 100, &d).unwrap();
        let b = s
            .open_session("u-b", "wow", 3000, "signature", Some("Wow.exe"), "0.2.0")
            .unwrap();
        s.close_session(b, "wow", 5000, 2000, 0, &d).unwrap();
        // still open: never uploaded
        s.open_session("u-c", "wow", 6000, "signature", None, "0.2.0")
            .unwrap();
        s
    }

    #[test]
    fn pending_contains_closed_sessions_without_exe_names() {
        let s = store_with_sessions();
        let b = s.pending_upload(100).unwrap();
        assert_eq!(b.sessions.len(), 2);
        assert_eq!(b.games.len(), 2);
        assert_eq!(b.sessions[0].id, "u-a");
        assert_eq!(b.sessions[0].duration_s, 900);
        assert_eq!(b.games[0].steam_appid, Some(730));
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains(".exe"), "exe names must never be uploaded");
        assert_eq!(s.pending_upload_count().unwrap(), 2);
    }

    #[test]
    fn mark_uploaded_and_requeue_on_change() {
        let mut s = store_with_sessions();
        s.mark_uploaded(&["u-a".into(), "u-b".into()], 7000)
            .unwrap();
        assert!(s.pending_upload(100).unwrap().is_empty());
        // relaunch merge: reopen + close again -> re-queued with the same uuid
        let id = s
            .all_sessions()
            .unwrap()
            .into_iter()
            .find(|x| x.uuid == "u-b")
            .unwrap()
            .id;
        s.reopen_session(id).unwrap();
        assert!(s.pending_upload(100).unwrap().is_empty(), "open again");
        s.close_session(id, "wow", 5600, 2500, 0, &BTreeMap::new())
            .unwrap();
        let b = s.pending_upload(100).unwrap();
        assert_eq!(b.sessions.len(), 1);
        assert_eq!(b.sessions[0].id, "u-b");
        assert_eq!(b.sessions[0].duration_s, 2500);
    }

    #[test]
    fn rejected_and_excluded_are_not_retried() {
        let s = store_with_sessions();
        s.mark_rejected("u-a", "too long").unwrap();
        s.set_game_excluded("wow", true).unwrap();
        assert!(s.pending_upload(100).unwrap().is_empty());
        s.reset_sync_state().unwrap();
        s.set_game_excluded("wow", false).unwrap();
        assert_eq!(s.pending_upload(100).unwrap().sessions.len(), 2);
    }

    #[test]
    fn migration_adds_columns_to_v1_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE sessions(id INTEGER PRIMARY KEY, uuid TEXT NOT NULL UNIQUE, game_id TEXT NOT NULL,
                  started_at INTEGER NOT NULL, ended_at INTEGER, last_checkpoint_at INTEGER NOT NULL,
                  active_seconds INTEGER NOT NULL DEFAULT 0, idle_trimmed_seconds INTEGER NOT NULL DEFAULT 0,
                  source TEXT NOT NULL DEFAULT 'tracked', detect_source TEXT, exe_name TEXT,
                  status TEXT NOT NULL, client_version TEXT);",
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.pending_upload_count().unwrap(), 0);
        // opening twice is fine (idempotent)
        drop(s);
        Store::open(&path).unwrap();
    }
}
