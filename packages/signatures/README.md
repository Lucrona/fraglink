# Game signatures

`games.json` lists the games Fraglink recognises by process name. It is compiled into the app (`include_str!`) and versioned by its `version` field.

- **Source:** hand-curated in `tools/build_games_json.py`. It was **not** scraped or derived from Discord's detectable-games list. It contains only exe names, optional path fragments and Steam app ids. No artwork or other copyrighted assets.
- **Steam games need no entry at all:** anything running from `steamapps/common/<installdir>` of an installed Steam game is detected from the local Steam manifests. Entries here cover non-Steam launchers (Battle.net, Riot, Epic, EA, Ubisoft, Xbox/MS Store, standalone) and give nicer names.
- **Entry fields:** `id`, `name`, `exes` (case-insensitive exe names, matched anywhere), `path_exes` + `path_hints` (generic exe names such as `javaw.exe` that only count when the path contains a hint), `steam_appids`, `uncertain` (the exe name is from memory or community sources and not yet verified on a real install; please confirm while testing).
- `ignore` lists launchers and helpers that must never count as games. `non_game` lists common apps shown last in the "This is a game…" picker.

Regenerate after editing:

```bash
python3 packages/signatures/tools/build_games_json.py
cargo test -p fraglink-core signatures
```

## Detectable list and overrides (0.2.0-dev.12)

- `detectable.json.gz` is a compact, Windows-only snapshot of Discord's public detectable-applications list, built by `crates/fraglink-core/src/bin/fraglink-detectable.rs` (refresh with `scripts/update-detectable.sh`). It is the offline fallback only: the server re-downloads the list daily and the app fetches it from the server. It is kept separate from `games.json`.
- `overrides.json` holds Fraglink's folder rules (WoW versions, Star Citizen channels), checked before every list. The server can serve a newer copy from `/var/lib/fraglink/detect-overrides.json`.
