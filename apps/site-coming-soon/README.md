# Fraglink "coming soon" page (retired)

> **Retired.** playfraglink.com now serves the real website ([`apps/site-web`](../site-web/)). These files are kept only so old cached pages and links don't break.

This was the temporary landing page for https://playfraglink.com/ (the site root only). Static files, no scripts,
trackers or external fonts.

- `index.html` is the page. Assets are under `/site/` (`logo.svg` = brand v6 concept 28 Split Orbit
  lockup, chrome; `favicon.svg`/`favicon-32.png`; `og.jpg` = the social preview).

Extras (no build step, plain JS in index.html):
- "Join the Discord" button -> https://discord.gg/QbaYRtvADj, official white Discord symbol (`site/discord-symbol-white.svg`, unmodified from discord.com/branding). Member/online count comes client-side from the Discord invite API (CORS allows this origin) and is hidden if that fails.
- Status dot: same-origin `GET /api/v1/health` (`ok: true`) + `/updates/latest.json`. Both OK = "All systems operational"; API OK, manifest bad = "Degraded"; API error = "Down"; both fetches fail at the network level = grey "Status unavailable". Links to the Discord invite (#status posts live there).
