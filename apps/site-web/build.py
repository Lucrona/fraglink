#!/usr/bin/env python3
"""Build the website into dist/: wrap pages/*.html in layout.html and copy static/.
Usage: python3 build.py   (no dependencies)"""
import hashlib, os, re, shutil
HERE = os.path.dirname(os.path.abspath(__file__))
DIST = os.path.join(HERE, "dist")
shutil.rmtree(DIST, ignore_errors=True)
shutil.copytree(os.path.join(HERE, "static"), DIST)
h = hashlib.sha256()
for f in ("site.css", "app.js"):
    h.update(open(os.path.join(HERE, "static", "assets", f), "rb").read())
v = h.hexdigest()[:10]
layout = open(os.path.join(HERE, "layout.html")).read()
for name in sorted(os.listdir(os.path.join(HERE, "pages"))):
    src = open(os.path.join(HERE, "pages", name)).read()
    m = re.match(r"<!--\s*(.*?)\s*-->\n", src)
    meta = dict(kv.split(":", 1) for kv in m.group(1).split("|")) if m else {}
    meta = {k.strip(): v_.strip() for k, v_ in meta.items()}
    body = src[m.end():] if m else src
    out = (layout.replace("{{title}}", meta.get("title", "Fraglink")).replace("{{page}}", meta.get("page", ""))
                 .replace("{{v}}", v).replace("{{body}}", body.strip()))
    assert "xfire" not in out.lower(), name
    open(os.path.join(DIST, name), "w").write(out)
open(os.path.join(DIST, "robots.txt"), "w").write("User-agent: *\nDisallow: /\n")
for root, _, files in os.walk(DIST):
    for f in files:
        if f.endswith((".html", ".js", ".css")):
            assert "xfire" not in open(os.path.join(root, f), encoding="utf-8").read().lower(), f
print("built", DIST, "v=" + v)
