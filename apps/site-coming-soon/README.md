# Fraglink "coming soon" page

Temporary landing page for https://playfraglink.com/ (the site root only). Static files, no scripts,
trackers or external fonts.

- `index.html` is the page. Assets are under `/site/` (`logo.svg` = brand v6 concept 28 Split Orbit
  lockup, chrome; `favicon.svg`/`favicon-32.png`; `og.jpg` = the social preview).
- Served by Caddy from `/var/www/fraglink-site` for the `playfraglink.com` host: exact path `/`
  (without a query string) and `/site/*`. Every other path still goes to the Fraglink server.

Extras (no build step, plain JS in index.html):
- "Join the Discord" button -> https://discord.gg/QbaYRtvADj, official white Discord symbol (`site/discord-symbol-white.svg`, unmodified from discord.com/branding). Member/online count comes client-side from the Discord invite API (CORS allows this origin) and is hidden if that fails.
- Status dot: same-origin `GET /api/v1/health` (`ok: true`) + `/updates/latest.json`. Both OK = "All systems operational"; API OK, manifest bad = "Degraded"; API error = "Down"; both fetches fail at the network level = grey "Status unavailable". Links to the Discord invite (#status posts live there).

Deploy: copy `index.html` and `site/` to `/var/www/fraglink-site/` on the server.
