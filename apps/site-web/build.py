#!/usr/bin/env python3
"""Build the website into dist/: wrap pages/*.html in layout.html and copy static/.
Usage: python3 build.py   (no dependencies)"""
import hashlib, os, re, shutil
HERE = os.path.dirname(os.path.abspath(__file__))
DIST = os.path.join(HERE, "dist")
shutil.rmtree(DIST, ignore_errors=True)
shutil.copytree(os.path.join(HERE, "static"), DIST)
h = hashlib.sha256()
for f in ("site.css", "app.js", "admin.js"):
    h.update(open(os.path.join(HERE, "static", "assets", f), "rb").read())
v = h.hexdigest()[:10]
layout = open(os.path.join(HERE, "layout.html")).read()
# Cloudflare Turnstile site key (public). Widget "Fraglink website": playfraglink.com + beta.
SITEKEY = os.environ.get("FRAGLINK_TURNSTILE_SITEKEY", "0x4AAAAAAFUF0sKGuaUYzQ8H")
# Search engines. Production is playfraglink.com; beta sends noindex headers and Disallow: / (Caddy).
CANON = "https://playfraglink.com"
DESC = "Fraglink: a lightweight Windows tray app that tracks your gaming hours and shows what your friends are playing."
# pages search engines may index -> their one canonical address (also in the server's sitemap)
INDEXABLE = {"home": "/", "download": "/download", "news": None, "privacy": "/privacy", "terms": "/terms", "faq": "/faq", "changelog": "/changelog", "contact": "/contact", "status": "/status", "media": "/media"}
OG_IMG = CANON + "/assets/og-card.png"
def seo_tags(page, title, desc):
    og = (f'<meta property="og:type" content="website">\n<meta property="og:site_name" content="Fraglink">\n'
          f'<meta property="og:title" content="{title}">\n<meta property="og:description" content="{desc}">\n'
          f'<meta property="og:image" content="{OG_IMG}">\n<meta property="og:image:width" content="1200">\n'
          f'<meta property="og:image:height" content="630">\n<meta name="twitter:card" content="summary_large_image">')
    if page not in INDEXABLE:  # sign-in, settings, friends, reset, admin, 404, profile template...
        return '<meta name="robots" content="noindex">\n' + og
    path = INDEXABLE[page]
    if path is None:  # news.html also serves /news/<post>: no fixed canonical
        return og
    return f'<link rel="canonical" href="{CANON}{path}">\n<meta property="og:url" content="{CANON}{path}">\n' + og
for name in sorted(os.listdir(os.path.join(HERE, "pages"))):
    src = open(os.path.join(HERE, "pages", name)).read()
    m = re.match(r"<!--\s*(.*?)\s*-->\n", src)
    meta = dict(kv.split(":", 1) for kv in m.group(1).split("|")) if m else {}
    meta = {k.strip(): v_.strip() for k, v_ in meta.items()}
    body = src[m.end():] if m else src
    title, desc = meta.get("title", "Fraglink"), meta.get("desc", DESC)
    assert '"' not in title + desc and "<" not in title + desc, name
    out = (layout.replace("{{seo}}", seo_tags(meta.get("page", ""), title, desc)).replace("{{desc}}", desc)
                 .replace("{{title}}", title).replace("{{page}}", meta.get("page", ""))
                 .replace("{{v}}", v).replace("{{body}}", body.strip())).replace("__SITEKEY__", SITEKEY)
    if meta.get("page") in ("privacy", "terms"):  # published legal pages: no "Private preview" strip, even on beta
        out = re.sub(r'<div class="preview-flag">.*?</div></div>\n?', "", out)
    assert "xfire" not in out.lower(), name
    open(os.path.join(DIST, name), "w").write(out)
# playfraglink.com's robots.txt (beta.playfraglink.com answers its own "Disallow: /" in Caddy)
open(os.path.join(DIST, "robots.txt"), "w").write(open(os.path.join(HERE, "robots.txt")).read())
for root, _, files in os.walk(DIST):
    for f in files:
        if f.endswith((".html", ".js", ".css")):
            assert "xfire" not in open(os.path.join(root, f), encoding="utf-8").read().lower(), f
print("built", DIST, "v=" + v)
