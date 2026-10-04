# Fraglink website (private preview)

The real website: home, sign up / sign in / sign out, forgot password (emails pending), profiles
(`/u/<username>`), friends, account settings, download, privacy and terms. Plain HTML + one CSS + one
JS file, no framework and no build dependencies. It talks to the Fraglink server's JSON API with a
bearer token kept in the browser after sign-in (`/api/v1/auth/login`).

```
pages/*.html     page bodies (first line: <!-- title: ... | page: <key> -->)
layout.html      shared header/footer (noindex meta)
static/assets/   site.css, app.js, logo, icons
build.py         python3 build.py  ->  dist/  (asserts no banned words, writes robots.txt)
```

Server endpoints used: `auth/login`, `auth/signup`, `auth/logout`, `me`, `me/privacy`, `me/password`,
`me/email`, `me` (DELETE), `devices`, `export.zip`, `stats`, `friends*`, `blocks*`, `games/icons`, and the
newer `users/{handle}/profile` + `me/display_name` (`apps/server/src/profile.rs`). Until a server with
those is deployed, the profile page falls back to `me` + `stats` (own profile) or the friend summary +
activity feed (friends), and changing the display name shows "needs the next server update".
The download button reads `/updates/latest.json`.

## Hosting (VPS)

Served by Caddy at `https://beta.playfraglink.com` from `/var/www/fraglink-beta`, with `/api/*` proxied
to the server, `/updates/*` read from the update folder, and `/icons/games/*` (official game icons) proxied to the server.
Only people with the secret link (`/?key=<key>`, sets an HttpOnly `fl_preview` cookie for a year) get
in; everyone else gets a plain 404. Every response carries `X-Robots-Tag: noindex, nofollow, noarchive`,
pages have a noindex meta tag, and `/robots.txt` disallows everything.

The key lives only on the server, in `/etc/caddy/beta-gate.caddy` (root:caddy 0640), imported at the top
of the Caddyfile:

```
(beta_gate) {
	@betakey query key=<KEY>
	handle @betakey {
		header Set-Cookie "fl_preview=<KEY>; Path=/; Max-Age=31536000; HttpOnly; Secure; SameSite=Lax"
		redir {path} 302
	}
	@betanokey not header Cookie *fl_preview=<KEY>*
	handle @betanokey {
		respond "Not found" 404
	}
}
```

To rotate the key, edit that file and `systemctl reload caddy` (old links and cookies stop working).
The site block itself is in `/etc/caddy/Caddyfile` (`beta.playfraglink.com { ... import beta_gate ... }`).

Deploy: `python3 build.py`, then copy `dist/` into `/var/www/fraglink-beta/`.
