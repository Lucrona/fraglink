# Contributing

Fraglink is in early, private development by a solo developer, so outside contributions aren't being accepted yet. Bug reports and game-detection fixes from testers are very welcome through the issue templates.

If you are working in this repo:

- **Build:** see the README ("Build from source").
- **Before pushing:** run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. CI runs the same checks and builds the Windows installer.
- **Game signatures:** edit the table in `packages/signatures/tools/build_games_json.py` and regenerate `games.json`. Only list exe names, path fragments and Steam app ids. Never copy other products' detection lists or bundle artwork. Mark anything unverified as `uncertain`.
- **Hard rules:** no game-memory reads, injection, hooks, overlays or keyloggers. No network calls or telemetry without an explicit, opt-in milestone for it.
- **Commits:** small, descriptive commits. Update `CHANGELOG.md` under *Unreleased*.

Unless agreed otherwise in writing, contributions are made under the repository's current terms (all rights reserved).
