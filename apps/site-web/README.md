# Fraglink website

The live website at [playfraglink.com](https://playfraglink.com): home, news, sign up / sign in with
email verification, password reset and optional two-step sign-in, profiles (`/u/<username>`), friends
and friends' activity, leaderboards, linked accounts (Steam, Battle.net, Discord), WoW characters and
`/played` upload, account settings, download, changelog, status, FAQ, contact, privacy and terms.
Plain HTML + one CSS + one JS file, no framework and no build dependencies. It talks to the Fraglink
server's JSON API with a bearer token kept in the browser after sign-in (`/api/v1/auth/login`).

```
pages/*.html     page bodies (first line: <!-- title: ... | page: <key> -->)
layout.html      shared header/footer; the <!--seo--> block holds title, description, canonical, robots and og tags
static/assets/   site.css, app.js, logo, icons
build.py         python3 build.py  ->  dist/  (asserts no banned words, per-page SEO tags, copies robots.txt)
robots.txt       playfraglink.com's robots.txt (beta answers its own Disallow: / in Caddy)
```

The pages call the server's JSON API under `/api/v1/` (see [`apps/server/README.md`](../server/README.md)
and `apps/server/src/`). The download button reads `/updates/latest.json`.

## Hosting

Build with `python3 build.py`; Caddy serves `dist/`, with `/api/*` proxied to the server. A key-gated
copy at `beta.playfraglink.com` is used for testing before release and is never indexed. Server
setup and operating details are kept in the private operator runbook, not in this repo.

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

## Admin section (`/admin`)

Staff tools for owner/admin/moderator accounts (`users.role`; see `apps/server/src/admin.rs`).
`/admin` looks like a normal "Not found" page to everyone who isn't staff.

`app.js` asks `GET /api/v1/admin/me` and only for staff
loads `assets/admin.js`. All checks happen on the server: every `/api/v1/admin/*` endpoint returns
404 to non-staff (and to cookie sessions), 403 when the role is too low, and writes `admin_audit`.
The site banner comes from `GET /api/v1/announcement` (public).
