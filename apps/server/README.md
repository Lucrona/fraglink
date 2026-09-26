# fraglink-server (M2, local development)

Accounts, desktop pairing, session sync, trust-tier hours and Steam import for Fraglink.
Rust (axum 0.8, sqlx 0.9) + Postgres. **Local only: nothing is deployed.** See
[`docs/decisions.md`](../../docs/decisions.md) for the design choices.

## Run it

```bash
# 1. Postgres: either Docker...
docker compose up -d db
# ...or any local Postgres 14+ with a database + user fraglink/fraglink.

# 2. The server (runs migrations on start)
export DATABASE_URL=postgres://fraglink:fraglink@127.0.0.1:5432/fraglink
export STEAM_API_KEY=...        # optional, see below
cargo run -p fraglink-server
# -> http://localhost:8787  (sign up, then pair the tray app)
```

Or everything in containers: `docker compose --profile server up --build`.

Then in the tray app: **Tools → Options → Sync (optional) → Sign in to sync**. Keep the default
`http://localhost:8787`. Your browser opens the pairing page: sign in, check the code, approve.

## Configuration (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | (required) | Postgres connection string |
| `FRAGLINK_BIND` | `127.0.0.1:8787` | Listen address |
| `FRAGLINK_PUBLIC_URL` | `http://localhost:8787` | How browsers reach the server (pairing links, OpenID return URL, cookie `Secure` flag when https) |
| `STEAM_API_KEY` | unset | Steam Web API key. Without it, linking works but importing is disabled |
| `STEAM_API_BASE` / `STEAM_OPENID_URL` | Steam's | Only for tests/mocks |
| `RUST_LOG` | `info,sqlx=warn` | Log level |

See `.env.example`. Keep real values out of git (`.env` is ignored).

## Steam import

- Uses `IPlayerService/GetOwnedGames` (with `include_appinfo` and `include_played_free_games`) and `ISteamUser/ResolveVanityURL`.
- Get a key at <https://steamcommunity.com/dev/apikey>. **Users must set Steam → Profile → Edit Profile → Privacy Settings → "Game details" to Public** (and not tick "Always keep my total playtime private"). Otherwise Steam returns an empty list; Fraglink reports that with instructions and marks the link `private`.
- Imported minutes are the **Tracked** tier (Steam's own counter), matched to Fraglink's game ids (e.g. app 730 → `cs2`), so they merge with desktop sessions as `max(sessions, steam)`.
- Manual import at most every 10 minutes. The server also refreshes every linked account once a day.
- "Sign in through Steam" (OpenID) verifies ownership of the Steam account.

## API (JSON, `/api/v1`)

| Method | Path | Auth | |
|---|---|---|---|
| GET | `/health` | none | |
| POST | `/auth/signup` `{email,password,handle}` | none | returns `{token,user}` (web token, 30 days) |
| POST | `/auth/login` `{email,password}` | none | 10 failures / 15 min / email → 429 |
| POST | `/auth/logout` | token | revokes the calling token |
| GET / DELETE | `/me` (DELETE body `{password}`) | token | DELETE = immediate hard delete of everything |
| POST | `/device/start` `{device_name}` | none | device-code pairing: `{device_code,user_code,verification_uri(_complete),expires_in,interval}` |
| POST | `/device/poll` `{device_code}` | none | `pending` / `approved` + device token (single use); 410 expired |
| POST | `/device/approve` `{user_code}` | web token | |
| GET / DELETE | `/devices`, `/devices/{id}` | token | list / revoke paired apps |
| POST | `/sessions/upload` `{client_version,games,sessions}` | token | idempotent; `{accepted:[ids],rejected:[{id,reason}]}` |
| GET | `/stats` | token | per-game tracked / imported / estimated / display + tier |
| GET | `/leaderboard/{game_id}?period=7d\|all` | token | **Tracked only** |
| POST | `/hours/estimate` `{game_id\|game_name, eras:[{from_year,to_year,hours_per_week}], note}` | token | Estimated tier; one per game (replaces) |
| POST | `/hours/import` `{game_id\|game_name, minutes, source: manual\|wow_manual\|xbox_recap, note}` | token | Imported tier (self-reported) |
| DELETE | `/hours/{id}` | token | |
| POST | `/steam/link` `{profile}` · `/steam/sync` | token | |
| GET | `/export` | token | everything about you as JSON |

Web pages (no JavaScript): `/`, `/signup`, `/login`, `/pair`, `/auth/steam/start`.

## Tests

```bash
export DATABASE_URL=postgres://fraglink:fraglink@127.0.0.1:5432/fraglink   # user must be allowed to CREATE DATABASE
cargo test -p fraglink-server
```

Each test gets a fresh database (`#[sqlx::test]`). Steam is mocked by a local axum server, so no key is needed.
`tests/e2e.rs` runs the real desktop path: local SQLite store → outbox → `fraglink-sync` client → this server.
