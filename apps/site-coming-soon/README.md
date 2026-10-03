# Fraglink "coming soon" page

Temporary landing page for https://playfraglink.com/ (the site root only). Static files, no scripts,
trackers or external fonts.

- `index.html` is the page. Assets are under `/site/` (`logo.svg` = brand v6 concept 28 Split Orbit
  lockup, chrome; `favicon.svg`/`favicon-32.png`; `og.jpg` = the social preview).
- Served by Caddy from `/var/www/fraglink-site` for the `playfraglink.com` host: exact path `/`
  (without a query string) and `/site/*`. Every other path still goes to the Fraglink server.

Deploy: copy `index.html` and `site/` to `/var/www/fraglink-site/` on the server.
