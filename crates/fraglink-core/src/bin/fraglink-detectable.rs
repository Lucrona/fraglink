//! Build the compact detectable list from Discord's raw JSON (the same code the
//! server runs daily). Used to refresh the snapshot bundled into the app.
//!
//! ```text
//! fraglink-detectable <discord.json> <out.json.gz> [slugs.json]
//! ```
//! Ids already in the bundled snapshot are kept. `slugs.json` (Discord id ->
//! Fraglink id) is read too if present and rewritten with any new slugs.

use fraglink_core::detectable::build_from_discord;
use fraglink_core::signatures::SignatureDb;
use std::collections::BTreeMap;
use std::io::Write;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(src), Some(out)) = (args.first(), args.get(1)) else {
        anyhow::bail!("usage: fraglink-detectable <discord.json> <out.json.gz> [slugs.json]");
    };
    let raw = std::fs::read(src)?;
    // keep the ids of the current bundled snapshot, then any from slugs.json
    let mut frozen = fraglink_core::detectable::bundled_slugs();
    if let Some(p) = args.get(2).filter(|p| std::path::Path::new(p).exists()) {
        frozen.extend(serde_json::from_slice::<BTreeMap<String, String>>(
            &std::fs::read(p)?,
        )?);
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let (file, slugs) = build_from_discord(&raw, &SignatureDb::builtin(), &frozen, &now)?;
    let json = serde_json::to_vec(&file)?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&json)?;
    std::fs::write(out, gz.finish()?)?;
    if let Some(p) = args.get(2) {
        std::fs::write(p, serde_json::to_vec_pretty(&slugs)?)?;
    }
    eprintln!(
        "{} games, {} KB json, written to {out}",
        file.games.len(),
        json.len() / 1024
    );
    Ok(())
}
