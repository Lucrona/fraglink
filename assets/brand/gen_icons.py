#!/usr/bin/env python3
"""Fraglink app icon v2 (Xfire-style): light silver tile, thin darker edge, charcoal Split Orbit mark.
16 px is hand-drawn pixel art, 24-48 px use crisp 1x tiles and a thickened mark, 64 px and up are
rendered from the SVG. Needs rsvg-convert and Pillow. Writes apps/desktop/src-tauri/icons/, including
the tray icons tray-16/20/24/32.png (same silver design; the app picks the size Windows asks for) and
the ui favicon."""
import subprocess, io, os
from PIL import Image, ImageDraw, ImageFilter
HERE = os.path.dirname(os.path.abspath(__file__))
B = HERE
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
ICONS = os.path.join(ROOT, 'apps/desktop/src-tauri/icons')
INK = (38, 41, 46)
def mark_png(w, color='#26292E'):
    s = open(f'{B}/v28-split-orbit-mark.svg').read().replace('#FFFFFF', color)
    png = subprocess.run(['rsvg-convert', '-w', str(w)], input=s.encode(), check=True, capture_output=True).stdout
    im = Image.open(io.BytesIO(png)).convert('RGBA')
    return im.crop(im.getchannel('A').getbbox())
def big_tile(n, ss=8):
    s = n * ss
    bg = Image.new('RGBA', (s, s)); d = ImageDraw.Draw(bg)
    for y in range(s):
        t = y / (s - 1)
        # bright silver top, a soft metallic dip at 55%, lighter again at the bottom
        k = 247 - 38 * t + (12 * max(0, t - .55) / .45 if t > .55 else 0)
        d.line([(0, y), (s, y)], fill=(int(k), int(k + 1), int(k + 4), 255))
    hl = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(hl).ellipse([-s * .3, -s * .75, s * 1.3, s * .42], fill=(255, 255, 255, 90))
    bg = Image.alpha_composite(bg, hl.filter(ImageFilter.GaussianBlur(s * .04)))
    r = s * (.18 if n >= 48 else .16)
    mask = Image.new('L', (s, s), 0); ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], r, fill=255)
    edge = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    ew = max(ss, round(s / 64 * 1.2))  # ~1px at small sizes
    ImageDraw.Draw(edge).rounded_rectangle([ew / 2, ew / 2, s - 1 - ew / 2, s - 1 - ew / 2], r - ew / 2, outline=(112, 118, 128, 255), width=ew)
    bg = Image.alpha_composite(bg, edge)
    out = Image.new('RGBA', (s, s), (0, 0, 0, 0)); out.paste(bg, (0, 0), mask)
    return out.resize((n, n), Image.LANCZOS)
def icon(n, frac, dy=0, harden=1.0):
    base = tile(n)
    mw = round(n * frac)
    big = mark_png(2048)
    m = big.resize((mw * 4, round(mw * 4 * big.height / big.width)), Image.LANCZOS).resize((mw, round(mw * big.height / big.width)), Image.LANCZOS)
    if harden != 1.0:
        a = m.getchannel('A').point(lambda v: 0 if v < 50 else min(255, int(v * harden)))
        m.putalpha(a)
    base.alpha_composite(m, ((n - m.width) // 2, (n - m.height) // 2 + dy))
    return base
def mark_thick(w, sw):
    s = open(f'{B}/v28-split-orbit-mark.svg').read()
    s = s.replace('fill="#FFFFFF" fill-rule="evenodd"/>', f'fill="#26292E" fill-rule="evenodd" stroke="#26292E" stroke-width="{sw}" stroke-linejoin="round"/>').replace('#FFFFFF', '#26292E')
    png = subprocess.run(['rsvg-convert', '-w', str(w)], input=s.encode(), check=True, capture_output=True).stdout
    im = Image.open(io.BytesIO(png)).convert('RGBA'); return im.crop(im.getchannel('A').getbbox())
def icon2(n, frac, sw, harden, dy=0, dx=0):
    base = tile(n); mw = round(n * frac); big = mark_thick(2048, sw)
    m = big.resize((mw, round(mw * big.height / big.width)), Image.LANCZOS)
    a = m.getchannel('A').point(lambda v: 0 if v < 60 else min(255, int(v * harden))); m.putalpha(a)
    base.alpha_composite(m, ((n - m.width) // 2 + dx, (n - m.height) // 2 + dy)); return base
P16 = [
"................",
"................",
"......########..",
".....########...",
".....#######....",
".....###........",
"...o.######.o...",
"...o.#####..o...",
"..o.###.....o...",
"..o.###....o....",
"..o.###...o.....",
"...###..oo......",
"...###ooo.......",
"...###..........",
"................",
"................"]
def icon16():
    base = tile(16); px = base.load()
    for y, row in enumerate(P16):
        for x, c in enumerate(row):
            if c == '#': px[x, y] = INK + (255,)
            elif c == 'o': px[x, y] = (62, 66, 73, 255)
    return base
def small_tile(n):
    im = Image.new('RGBA', (n, n), (0, 0, 0, 0)); d = ImageDraw.Draw(im)
    r = 2 if n <= 16 else 4 if n <= 32 else 6
    fill = Image.new('RGBA', (n, n)); fd = ImageDraw.Draw(fill)
    for y in range(n):
        t = y / (n - 1); k = 248 - 44 * t + (14 * (t - .6) / .4 if t > .6 else 0)
        fd.line([(0, y), (n, y)], fill=(int(k), int(k + 1), int(k + 4), 255))
    mask = Image.new('L', (n, n), 0); ImageDraw.Draw(mask).rounded_rectangle([0, 0, n - 1, n - 1], r, fill=255)
    im.paste(fill, (0, 0), mask)
    d.rounded_rectangle([0, 0, n - 1, n - 1], r, outline=(108, 114, 124, 255), width=1)
    d.line([(r, 1), (n - 1 - r, 1)], fill=(255, 255, 255, 255))  # top inner highlight
    return im
def _unused():
    imgs = {16: mk2.icon16(), 32: mk2.icon2(32, .84, 5, 1.6), 48: mk2.icon2(48, .8, 3.5, 1.3), 256: icon(256, .76)}
    return imgs

def tile(n, ss=8):
    return small_tile(n) if n <= 48 else big_tile(n, ss)

def build():
    return {16: icon16(), 20: icon2(20, .9, 6.5, 1.7), 24: icon2(24, .9, 6, 1.6), 32: icon2(32, .84, 5, 1.6), 48: icon2(48, .8, 3.5, 1.3),
            64: icon(64, .78), 128: icon(128, .76), 256: icon(256, .76), 512: icon(512, .76)}

if __name__ == '__main__':
    imgs = build()
    for n, f in {32: '32x32.png', 64: '64x64.png', 128: '128x128.png', 256: '128x128@2x.png', 512: 'icon.png'}.items():
        imgs[n].save(os.path.join(ICONS, f))
    for n in (16, 20, 24, 32):
        imgs[n].save(os.path.join(ICONS, f'tray-{n}.png'))
    imgs[32].save(os.path.join(ROOT, 'apps/desktop/ui/favicon-32.png'))
    order = (16, 20, 24, 32, 48, 64, 128, 256)
    imgs[256].save(os.path.join(ICONS, 'icon.ico'), sizes=[(n, n) for n in order], append_images=[imgs[n] for n in order[:-1]])
    print('icons written')
