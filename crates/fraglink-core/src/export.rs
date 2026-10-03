//! "Export my data": everything in the local database as one zip with JSON + CSVs.
//! The server's account export uses the same layout (`sessions.csv`, `games.csv`, JSON).

use crate::store::Store;
use anyhow::Result;
use chrono::{Local, TimeZone, Utc};
use serde_json::json;
use std::path::{Path, PathBuf};

pub const EXPORT_SCHEMA: u32 = 1;

/// Settings that hold credentials and are never exported.
pub const SECRET_SETTINGS: &[&str] = &["sync_token"];

fn iso_utc(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|t| t.to_rfc3339())
        .unwrap_or_default()
}

fn iso_local(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|t| t.to_rfc3339())
        .unwrap_or_default()
}

pub fn export_json(store: &Store, now: i64) -> Result<serde_json::Value> {
    let sessions: Vec<_> = store
        .all_sessions()?
        .into_iter()
        .map(|s| {
            json!({
                "uuid": s.uuid,
                "game_id": s.game_id,
                "game_name": s.game_name,
                "started_at": iso_utc(s.started_at),
                "ended_at": s.ended_at.map(iso_utc),
                "active_seconds": s.active_seconds,
                "idle_trimmed_seconds": s.idle_trimmed_seconds,
                "source": s.source,
                "detect_source": s.detect_source,
                "exe_name": s.exe_name,
                "status": s.status,
            })
        })
        .collect();
    let mut settings = store.all_settings()?;
    // Credentials never go into exports.
    settings.retain(|k, _| !SECRET_SETTINGS.contains(&k.as_str()));
    Ok(json!({
        "schema": EXPORT_SCHEMA,
        "app": "Fraglink",
        "app_version": env!("CARGO_PKG_VERSION"),
        "generated_at": iso_utc(now),
        "notes": "All times UTC (RFC 3339). active_seconds is the time the game ran (sessions before 0.2.0-dev.9 may have idle time trimmed). tier 'tracked' = logged by Fraglink on this PC.",
        "games": store.list_games()?,
        "sessions": sessions,
        "session_days": store.all_session_days()?,
        "activity": store.all_activity()?,
        "settings": settings,
        "manual_tags": store.manual_tags()?.into_iter().map(|(exe, (id, name))| json!({"exe_name": exe, "game_id": id, "game_name": name})).collect::<Vec<_>>(),
        "excluded_exes": store.excluded_exes()?,
    }))
}

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn sessions_csv(store: &Store) -> Result<String> {
    let mut out = String::from(
        "session_uuid,game_id,game_name,started_at_utc,ended_at_utc,started_at_local,active_seconds,active_hours,idle_trimmed_seconds,source,tier,detect_source,exe_name,status\n",
    );
    for s in store.all_sessions()? {
        let row = [
            s.uuid.clone(),
            s.game_id.clone(),
            s.game_name.clone(),
            iso_utc(s.started_at),
            s.ended_at.map(iso_utc).unwrap_or_default(),
            iso_local(s.started_at),
            s.active_seconds.to_string(),
            format!("{:.2}", s.active_seconds as f64 / 3600.0),
            s.idle_trimmed_seconds.to_string(),
            s.source.clone(),
            "tracked".into(),
            s.detect_source.clone().unwrap_or_default(),
            s.exe_name.clone().unwrap_or_default(),
            s.status.clone(),
        ];
        out.push_str(
            &row.iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    Ok(out)
}

pub fn games_csv(store: &Store) -> Result<String> {
    let mut out = String::from(
        "game_id,game_name,source,steam_appid,excluded,tier,tracked_seconds,tracked_hours,sessions,last_played_utc\n",
    );
    for g in store.list_games()? {
        let row = [
            g.id.clone(),
            g.name.clone(),
            g.source.clone(),
            g.steam_appid.map(|a| a.to_string()).unwrap_or_default(),
            g.excluded.to_string(),
            "tracked".into(),
            g.total_seconds.to_string(),
            format!("{:.2}", g.total_seconds as f64 / 3600.0),
            g.sessions.to_string(),
            g.last_played.map(iso_utc).unwrap_or_default(),
        ];
        out.push_str(
            &row.iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    Ok(out)
}

/// Build a zip archive in memory from `(name, bytes)` entries (deflate).
pub fn zip_bytes(entries: &[(&str, Vec<u8>)]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(*name, opts)?;
        w.write_all(data)?;
    }
    Ok(w.finish()?.into_inner())
}

const LOCAL_README: &str = "Fraglink export (this PC)\n\n\
fraglink-export.json  everything in the local database (games, sessions, daily totals, activity, settings, tags)\n\
sessions.csv          one row per play session (times UTC + local, active and idle-trimmed seconds)\n\
games.csv             per-game totals (tier: tracked = logged by Fraglink on this PC)\n\n\
All times are RFC 3339. Credentials (the sync sign-in token) are never included.\n";

/// Writes `fraglink-export-<stamp>.zip` into `dir` and returns its path.
pub fn write_export(store: &Store, dir: &Path, now: i64) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = Local
        .timestamp_opt(now, 0)
        .single()
        .map(|t| t.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| now.to_string());
    let zp = dir.join(format!("fraglink-export-{stamp}.zip"));
    let bytes = zip_bytes(&[
        (
            "fraglink-export.json",
            serde_json::to_vec_pretty(&export_json(store, now)?)?,
        ),
        ("sessions.csv", sessions_csv(store)?.into_bytes()),
        ("games.csv", games_csv(store)?.into_bytes()),
        ("README.txt", LOCAL_README.as_bytes().to_vec()),
    ])?;
    std::fs::write(&zp, bytes)?;
    Ok(zp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn exports_json_and_csv() {
        let mut s = Store::open_in_memory().unwrap();
        s.add_manual_tag("Odd,Name.exe", "manual:odd", "Odd \"Game\", Deluxe", 1)
            .unwrap();
        let id = s
            .open_session(
                "uuid-1",
                "manual:odd",
                1_790_000_000,
                "manual",
                Some("Odd,Name.exe"),
                "t",
            )
            .unwrap();
        let mut days = BTreeMap::new();
        days.insert("2026-09-22".to_string(), 3600);
        s.close_session(id, "manual:odd", 1_790_003_700, 3600, 100, &days)
            .unwrap();

        let j = export_json(&s, 1_790_010_000).unwrap();
        assert_eq!(j["schema"], 1);
        assert_eq!(j["sessions"][0]["active_seconds"], 3600);
        assert_eq!(j["sessions"][0]["started_at"], "2026-09-21T14:13:20+00:00");
        assert_eq!(j["session_days"][0]["day"], "2026-09-22");
        assert_eq!(j["manual_tags"][0]["exe_name"], "odd,name.exe");

        let csv = sessions_csv(&s).unwrap();
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with(
            "uuid-1,manual:odd,\"Odd \"\"Game\"\", Deluxe\",2026-09-21T14:13:20+00:00,"
        ));
        assert!(lines[1].contains(",3600,1.00,100,tracked,tracked,manual,\"Odd,Name.exe\",closed"));

        let games = games_csv(&s).unwrap();
        assert!(
            games.lines().nth(1).unwrap().starts_with(
                "manual:odd,\"Odd \"\"Game\"\", Deluxe\",manual,,false,tracked,3600,1.00,1,"
            ),
            "{games}"
        );

        s.set_setting("sync_token", "plain:c2VjcmV0").unwrap();
        s.set_setting("skin", "phoenix").unwrap();
        let j = export_json(&s, 1_790_010_000).unwrap();
        assert!(j["settings"].get("sync_token").is_none());
        assert_eq!(j["settings"]["skin"], "phoenix");

        let tmp = tempfile::tempdir().unwrap();
        let zp = write_export(&s, tmp.path(), 1_790_010_000).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&zp).unwrap()).unwrap();
        let mut names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "README.txt",
                "fraglink-export.json",
                "games.csv",
                "sessions.csv"
            ]
        );
        let mut csv2 = String::new();
        std::io::Read::read_to_string(&mut z.by_name("sessions.csv").unwrap(), &mut csv2).unwrap();
        assert_eq!(csv2, csv);
        let raw = std::fs::read(&zp).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("c2VjcmV0"));
    }
}
