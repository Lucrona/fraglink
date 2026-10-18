# Fraglink website (private preview)

The real website: home, sign up / sign in / sign out, forgot password (emails pending), profiles
(`/u/<username>`), friends, account settings, download, privacy and terms. Plain HTML + one CSS + one
JS file, no framework and no build dependencies. It talks to the Fraglink server's JSON API with a
bearer token kept in the browser after sign-in (`/api/v1/auth/login`).

```
pages/*.html     page bodies (first line: <!-- title: ... | page: <key> -->)
layout.html      shared header/footer; the <!--seo--> block holds title, description, canonical, robots and og tags
static/assets/   site.css, app.js, logo, icons
build.py         python3 build.py  ->  dist/  (asserts no banned words, per-page SEO tags, copies robots.txt)
robots.txt       playfraglink.com's robots.txt (beta answers its own Disallow: / in Caddy)
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
in; everyone else gets a plain 404. Every response carries `X-Robots-Tag: noindex, nofollow, noarchive`
and `/robots.txt` disallows everything.

## Search engines (playfraglink.com)

Production is open to search engines. `robots.txt` allows crawling except the private and signed-in
pages, and points at `/sitemap.xml`. The server builds the sitemap: an index, `sitemaps/pages.xml`,
and `sitemaps/profiles-N.xml` (5,000 profiles per file). It lists only public profiles, never test
accounts, private profiles, or accounts that are leaving or under age review. `/u/<name>` is also sent by the server
(`apps/server/src/seo.rs`): `u.html` with that profile's canonical and preview tags. Non-indexable
profiles get `noindex`, and hidden ones (test accounts, leaving accounts) a 404 that looks exactly
like an unknown name. Only the indexable pages (home, download, privacy, terms) have a canonical link
to `https://playfraglink.com/...`. Sign-in, settings and other private pages carry a noindex tag, and Caddy
also sends `X-Robots-Tag: noindex` on them. `*.html` addresses redirect to the clean URL.

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

## Admin section (`/admin`)

Staff tools for owner/admin/moderator accounts (`users.role`; see `apps/server/src/admin.rs`).
Caddy serves `/admin` with HTTP status 404 for everyone:

```
	@admin path /admin /admin/ /admin.html
	handle @admin {
		root * /var/www/fraglink-beta
		rewrite * /admin.html
		header Cache-Control "no-store"
		file_server {
			status 404
		}
	}
```

The page shows the normal "Not found" panel; `app.js` asks `GET /api/v1/admin/me` and only for staff
loads `assets/admin.js`. All checks happen on the server: every `/api/v1/admin/*` endpoint returns
404 to non-staff (and to cookie sessions), 403 when the role is too low, and writes `admin_audit`.
The site banner comes from `GET /api/v1/announcement` (public).
