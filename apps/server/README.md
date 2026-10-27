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

Then point the client at it: on the Login screen open **Server** and enter `http://localhost:8787`
(or use Options > Account). Since dev.11 the client defaults to the hosted Fraglink server at
`https://playfraglink.com` (falling back to `http://119.42.54.86` when HTTPS fails). Log in or use **Create Account**; "Sign in with your browser instead" uses
the pairing page (sign in, check the code, approve).

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
- Manual import at most every 10 minutes. No automatic refresh: imports are one-off and tracked time adds on top (see docs/decisions.md D13).
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
| POST | `/sessions/upload` | - | removed: 410 Gone (hours come from live presence) |
| GET | `/stats` | token | per-game tracked / imported / estimated / display + tier |
| GET | `/leaderboard/{game_id}?period=7d\|all` | token | like profiles: all = `display_min`, 7d = sessions or Imported growth (D6) |
| POST | `/hours/estimate` | - | removed: 410 Gone `estimates_removed` (the history estimator is gone) |
| POST | `/hours/import` | - | disabled: 410 Gone `imports_disabled`, "Manual hour imports are disabled." Nothing is stored. Rows saved before stay, still count, and can be deleted. The old `/web/import` form answers the same. |
| DELETE | `/hours/{id}` | token | delete one of your Imported entries |
| POST | `/steam/link` `{profile}` · `/steam/sync` | token | |
| GET | `/export` | token | everything about you as JSON (incl. friends) |
| GET | `/export.zip` | token | README + JSON + CSVs (sessions, games per tier, Steam playtime); once per hour → 429 `export_cooldown` |
| GET | `/friends` | token | `{friends:[{handle,display_name,since,presence}],incoming,outgoing,blocked}` |
| POST | `/friends/request` `{handle}` | token | by exact username (case-insensitive, `@` ok) → `pending` / `accepted` (mutual); 30/hour |
| POST | `/friends/respond` `{handle,accept}` | token | decline deletes silently |
| DELETE | `/friends/{handle}` | token | remove a friend or cancel your request |
| POST / DELETE | `/blocks` `{handle}`, `/blocks/{handle}` | token | block (removes friendship, silent) / unblock |
| GET | `/ws` | token (Bearer or cookie) | presence WebSocket, see below |
| GET | `/games/icons?ids=a,b` | token | `{id:{steam_appid,icon_url\|null}}` (Steam's public icon, hash recorded from `GetOwnedGames`); max 50 ids |

### Presence WebSocket

Client → server: `{"type":"presence","status":"online|away|invisible","game":{"id","name","started_at"}|null}`, `{"type":"ping"}` (every 60 s).
Server → client: `hello{friends}` (snapshot), `presence{handle,started,status,game_id,game_name,session_started_at}` (friends only, on change; `started` = a new game), `friend_request`, `friend_accepted`, `friend_removed`, `pong`, `error`.
Invisible is stored but sent to friends as `offline`. No message for 150 s (`presence_idle_secs`) closes the socket; the last socket closing sets you offline.
To act as a friend's tray locally: `cargo run -p fraglink-sync --example presence -- http://localhost:8787 TOKEN online steam:570 "Dota 2"`.

Web pages (no JavaScript): `/`, `/signup`, `/login`, `/pair`, `/auth/steam/start`. The dashboard has hours, Steam, friends, devices, export links and Delete account.

Docker: `docker compose up -d db` for Postgres, or `docker compose --profile server up` for Postgres + this server (image from `apps/server/Dockerfile`).

## Tests

```bash
export DATABASE_URL=postgres://fraglink:fraglink@127.0.0.1:5432/fraglink   # user must be allowed to CREATE DATABASE
cargo test -p fraglink-server
```

Each test gets a fresh database (`#[sqlx::test]`). Steam is mocked by a local axum server, so no key is needed.
`tests/presence.rs` opens several real WebSocket clients; `tests/tray_presence.rs` drives the tray's own `fraglink-sync` client.
`tests/e2e.rs` runs the real desktop path: pairing through the `fraglink-sync` client against this server.
