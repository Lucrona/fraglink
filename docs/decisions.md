# Product and technical decisions

Decisions the spec leaves open, with the default picked and why. Each can be revisited;
the entries say what changing them would cost.

## D1. Server stack: Rust axum + Postgres (sqlx), not Supabase (yet)
- **Spec:** Supabase (Sydney) with RLS.
- **Picked:** a small Rust server (`apps/server`, axum 0.8 + sqlx 0.9 + Postgres 17), run locally with `docker compose up db` or any Postgres.
- **Why:** no external accounts or paid hosting were allowed for M2. Rust matches the desktop/core code, and the server can reuse `fraglink-core` types and game signatures.
- **Changing later:** the schema follows spec §8.1 (users, sessions, linked_accounts, steam_playtime, hours_entries, and a per-game stats view), so it can move to Supabase/any managed Postgres. On Supabase, auth would move to Supabase Auth and the API checks to RLS policies. **Hosting is still your call** (see "Open questions").

## D2. Desktop sign-in: device-code pairing, not a deep link
- **Spec:** email/Discord login "via deep link".
- **Picked:** OAuth-style device flow. The tray shows a code like `KXRT-7QPM` and opens `<server>/pair?code=…`. You sign in on the website and approve, and the tray receives a device token.
- **Why:** no custom URL scheme to register or hijack, and it works with any browser, password manager or 2FA the website adds later. The password never goes through the tray app.

## D3. Tokens: opaque random tokens, stored hashed
- 32 random bytes (URL-safe base64). The server stores only SHA-256 hashes.
- Web sessions expire after 30 days. Device tokens last until revoked (website → Paired apps, or Sign out in the tray).
- No JWTs, so revoking takes effect immediately.
- Passwords use Argon2id (argon2 crate defaults: 19 MiB, t=2). Logins are limited to 10 failures per email per 15 min.

## D4. Device token storage on the PC
- Stored in the local settings table. On Windows it is **encrypted with DPAPI** (current user scope, app-specific entropy), so a copied `fraglink.db` is useless elsewhere. On other OSes it is only base64-encoded (dev builds).
- The token is never sent to the webview.

## D5. What "Tracked" means per game
- `tracked = max(sum of Fraglink sessions, Steam's playtime_forever)`, **not the sum**. Steam already counts the sessions Fraglink saw for Steam games, so adding them would double count.
- `display = max(tracked, latest Imported counter)`. The tier marker shows which one won.
- Estimated is always shown separately (`~`) and **never** added to any total.

## D6. Rankings use Tracked only
- Leaderboards (7 days: Fraglink sessions; all time: tracked as in D5) ignore Imported and Estimated hours completely.
- Until M4 adds privacy settings, the leaderboard API needs a signed-in caller and shows handles only.

## D7. One estimate per game
- Saving the history estimator again for the same game **replaces** the old estimate instead of stacking another.
- Imported counters are kept as a history, and the latest one counts.
- Limits: 1–10 eras, years 1970 to now, at most 80 h/week. Warnings for more than 40 h/week over several years and for overlapping eras.

## D8. Manual games are private per user
- A game you tag yourself (`manual:my-mod`) becomes `manual:<your user id>:my-mod` on the server.
- Two people's "my-mod" never merge, and nobody can attach hours to someone else's manual game.
- Known games (signatures / Steam app ids) share one id for everyone.

## D9. Excluding a game stops future uploads only
- Excluding a game in the tray keeps its sessions from being uploaded from then on. Sessions that were already uploaded stay on the server until you delete them there (or delete the account).
- A per-game server delete can come with the M4 website.

## D10. Upload validation
- The server rejects a session for good when it:
  - lasts more than 24 h
  - ends in the future (5 min clock skew allowed)
  - starts before 2000
  - has more active time than its length
  - overlaps another of your sessions (60 s tolerance)
- Rejected sessions stay in the local database, marked with the reason, and aren't retried.
- Uploads are idempotent by session UUID, so re-sending after a network error never duplicates.

## D11. Account deletion is immediate and hard
- `DELETE /api/v1/me` (with password) removes the user and every row that belongs to them, straight away.
- There is no 30-day grace period (the spec allows either). Local data on the PC is untouched.

## D12. Steam link: typed profile is "unverified", Steam sign-in is "verified"
- Pasting a profile URL, custom URL or SteamID64 links it right away, so import works for any public profile.
- "Sign in through Steam" (OpenID 2.0, checked server-side with `check_authentication`) marks the link **verified**. Only one Fraglink account can hold a verified link to a given Steam account.
- Later features that need proof (public profiles, ranks) can require verified links.

## D13. Steam import frequency
- Manual "Import Steam playtime": at most once per 10 minutes per account.
- Automatic daily refresh: the server re-imports links older than 24 h, with 1.5 s between users (Steam Web API limit is about 100k calls/day).
- Games with 0 minutes are skipped.

## D14. Sync is opt-in and offline-first
- The tray app makes **no network calls** until you press "Sign in to sync".
- After sign-in it uploads closed sessions every 5 minutes and right after a session ends. When the server is unreachable it just retries later, and tracking is unaffected.
- Exe names and install paths are never uploaded. Only game id/name, start, end, active and idle-trimmed seconds, and the client version are sent.
- Signing out forgets the token and marks everything as not yet uploaded, so a later sign-in (even to another server) re-sends it all. Uploads are idempotent (see D10).

## D15. Server web pages are a stop-gap
- `apps/server` serves minimal no-JavaScript pages: sign up/in, approve a pairing code, link/import Steam, add Estimated/Imported hours, paired devices, export.
- They exist so M2 can be used end to end locally. The real website is M4 (Astro in the spec).
- The cookie is HttpOnly, SameSite=Lax (plus Secure on https). Every cookie-authenticated write also checks Origin/Referer.

## D16. Exports: JSON any time, zip once per hour
- `GET /api/v1/export` (JSON) has no limit. `GET /api/v1/export.zip` (JSON + CSVs) is limited to once per hour per account, because it's heavier to build. The limit is kept in memory, so it resets when the server restarts.
- The tray zip has the same shape (JSON + `sessions.csv` + `games.csv` + README) and never includes the sync token.

## D17. Friend requests by exact username only
- There's no user directory or search in M3, so nobody can scrape a user list. Matching ignores case and a leading `@`.
- Max 30 requests an hour per user. Unknown usernames return 404 (usernames are public anyway).
- If both people send a request, it's accepted automatically.

## D18. Declines and blocks are silent
- Declining deletes the request without telling the sender. A request to someone who blocked you returns `pending` and stores nothing.
- Blocking removes the friendship. The other person's tray only gets `friend_removed`, the same as a normal removal.
- Someone you've blocked can't send you requests, and you can't send them any until you unblock.

## D19. Invisible is stored, shown as offline
- The server keeps `invisible` so your devices agree, but friends only ever see `offline` with no game. The same applies to stale heartbeats (over 150 s).

## D20. One presence per account, last writer wins
- Presence is one row per user. If two PCs are signed in, the latest update wins. You go offline only when your last socket closes.
- Only the latest state is kept. There's no presence history.

## D21. Presence hub is in-process
- Fan-out uses an in-memory map from each user to their open sockets inside the single server process. It's simple and has no extra infrastructure.
- **Limit:** you can't run more than one server instance. Scaling out needs Postgres `LISTEN/NOTIFY` or Redis pub/sub (M7). Presence is reset to offline at startup.

## D22. WebSocket with a ping every 60 s and a 150 s timeout
- The tray sends `ping` every 60 s and republishes when its status or game changes (checked every 3 s, and nudged immediately by tracker events and status changes).
- The server closes sockets that stay silent for 150 s. The tray reconnects with backoff (2 s doubling to 60 s) and stops when signed out or when its token is refused.
- Only friends get updates, and only when what they can see changes (status, game). `started=true` means a new game id appeared.

## D23. Friend toast rules live in the tray
- A toast only appears when a friend *starts* a game. At most one per friend per 30 min.
- Settings (all on this PC, not synced): friend pop-ups on/off, "not while I'm playing" (**on by default**), mute per friend, and the global pop-ups switch.
- Friend requests also pop up (unless muted or pop-ups are off).

## D24. Away is manual in M3
- Away is only set from the status drop-down. Automatic away based on idle time is deferred: idle during a game already means "still playing" for friends.

## D25. Tray account totals, with offline fallback
- After each upload pass the tray fetches `/api/v1/stats` and shows a "My Account (all devices)" section on the Stats tab with tier markers. If that fetch fails, the section says the totals are unavailable, and the local numbers stay as they are.
- A game's Tracked total never shows lower than what this PC already recorded (uploads lag a few minutes). Estimates use the server's rounding (nearest 100 h above 1,000 h).

## D26. Account deletion from the web dashboard
- It needs your password plus a confirmation box, then hard-deletes everything at once (D11). It uses the same-origin check as other forms.

## D27. Docker validation on the dev box
- Docker 26 installs and runs on the dev box. The `apps/server/Dockerfile` image builds and runs (migrations plus signup against the compose `postgres:17` service).
- The dev sandbox can't route traffic between containers, so the full `docker compose --profile server up` was checked with the server container on the host network. On a normal Docker host the compose file works as written. Nothing is published to a registry.

## Open questions for you
1. **Hosting:** Supabase (as in the spec) vs a small VPS/managed Postgres running `fraglink-server`, and the region (spec: Sydney). Nothing is deployed.
2. **Steam Web API key:** needed for real imports. Create one at <https://steamcommunity.com/dev/apikey> (needs a Steam account with a purchase). Set it as `STEAM_API_KEY` on the server only. Never commit it.
3. **Discord login** (spec M2): not built yet. It needs a Discord application (client id/secret), which means creating an external account.
4. **Email verification and password reset:** they need an email provider (external service), so they are not built. Accounts work without verification for now.
5. **Grace period** for account deletion (D11) and whether excluded games should also be deleted server-side (D9).
6. **Privacy policy:** `docs/privacy-policy.md` is a DRAFT. It needs an operator name, contact, hosting location, a minimum age and legal review before any public launch.
7. **Presence scaling (D21):** fine for a single server. Pick Postgres LISTEN/NOTIFY or Redis when hosting is decided.
