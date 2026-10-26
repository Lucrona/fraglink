#!/usr/bin/env python3
"""Generate the website's /media files (logos, wallpapers, press kit zip) from the brand SVGs.
Usage: python3 assets/brand/gen_media.py   (needs CairoSVG + Pillow). Output: apps/site-web/static/assets/media/"""
import io, os, shutil, zipfile, math
import cairosvg
from PIL import Image, ImageDraw, ImageFilter
B = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(B, "..", "..", "apps", "site-web", "static", "assets", "media")
os.makedirs(OUT, exist_ok=True)
def png(svg, w):
    return Image.open(io.BytesIO(cairosvg.svg2png(url=os.path.join(B, svg), output_width=w))).convert("RGBA")
logos = {"fraglink-mark-white": "v28-split-orbit-mark.svg", "fraglink-mark-chrome": "v28-split-orbit-mark-chrome.svg",
         "fraglink-logo-white": "v28-split-orbit-lockup.svg", "fraglink-logo-chrome": "v28-split-orbit-lockup-chrome.svg"}
for name, svg in logos.items():
    shutil.copy(os.path.join(B, svg), os.path.join(OUT, name + ".svg"))
    png(svg, 1024 if "mark" in name else 2000).save(os.path.join(OUT, name + ".png"), optimize=True)
def wallpaper(w, h, name):
    img = Image.new("RGB", (w, h), (12, 14, 17))
    d = ImageDraw.Draw(img)
    for y in range(h):  # dark blue-grey vertical gradient
        t = y / h
        d.line([(0, y), (w, y)], fill=(int(14 + 8 * t), int(18 + 10 * t), int(24 + 16 * t)))
    glow = Image.new("L", (w, h), 0)
    r = int(min(w, h) * 0.55)
    ImageDraw.Draw(glow).ellipse([w // 2 - r, h // 2 - r, w // 2 + r, h // 2 + r], fill=38)
    glow = glow.filter(ImageFilter.GaussianBlur(r // 2))
    img = Image.composite(Image.new("RGB", (w, h), (224, 138, 32)), img, glow)
    portrait = h > w
    logo = png("v28-split-orbit-mark-chrome.svg" if portrait else "v28-split-orbit-lockup-chrome.svg",
               int(w * 0.55) if portrait else int(min(w * 0.42, h * 1.1)))
    img.paste(logo, ((w - logo.width) // 2, (h - logo.height) // 2), logo)
    img.save(os.path.join(OUT, name), quality=92, optimize=True)
walls = [(1920, 1080), (2560, 1440), (3440, 1440), (1080, 2340)]
names = []
for w, h in walls:
    n = f"fraglink-wallpaper-{w}x{h}.jpg"; wallpaper(w, h, n); names.append(n)
blurb = open(os.path.join(B, "press-blurb.txt")).read()
with zipfile.ZipFile(os.path.join(OUT, "fraglink-press-kit.zip"), "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("README.txt", blurb)
    for name in logos:
        z.write(os.path.join(OUT, name + ".svg"), "logos/" + name + ".svg")
        z.write(os.path.join(OUT, name + ".png"), "logos/" + name + ".png")
    for n in ("shot-friends.png", "shot-games.png", "shot-toast.png"):
        z.write(os.path.join(OUT, "..", n), "screenshots/" + n)
    z.write(os.path.join(OUT, names[0]), "wallpapers/" + names[0])
print("media ->", os.path.abspath(OUT))
