#!/usr/bin/env python3
"""Build the Fraglink app icons from the "Split Orbit" mark (concept 28).
Needs rsvg-convert and Pillow. Writes into apps/desktop/src-tauri/icons/ and installer/."""
import subprocess, io, os
from PIL import Image, ImageDraw, ImageFilter
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
ICONS = os.path.join(ROOT, 'apps/desktop/src-tauri/icons')
INST = os.path.join(ROOT, 'apps/desktop/src-tauri/installer')

def svg(name, w):
    png = subprocess.run(['rsvg-convert', '-w', str(w), os.path.join(HERE, name)], check=True, capture_output=True).stdout
    return Image.open(io.BytesIO(png)).convert('RGBA')

def tile(n):
    """Dark rounded square with a faint blue glow: readable on light and dark taskbars."""
    s = n * 4
    im = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    bg = Image.new('RGBA', (s, s))
    d = ImageDraw.Draw(bg)
    for y in range(s):
        t = y / (s - 1)
        d.line([(0, y), (s, y)], fill=(int(40 - 26 * t), int(46 - 30 * t), int(58 - 36 * t), 255))
    glow = Image.new('RGBA', (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([s * .1, s * .25, s * .9, s * 1.1], fill=(40, 100, 210, 110))
    bg = Image.alpha_composite(bg, glow.filter(ImageFilter.GaussianBlur(s * .12)))
    mask = Image.new('L', (s, s), 0)
    r = s * (.2 if n >= 48 else .14)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], r, fill=255)
    im.paste(bg, (0, 0), mask)
    return im.resize((n, n), Image.LANCZOS)

def icon(n):
    base = tile(n)
    # small sizes: flat white mark, bigger share of the tile, for crisp pixels
    flat = n <= 32
    frac = 1.0 if n <= 16 else .9 if n <= 32 else .78
    mw = round(n * frac)
    big = svg('v28-split-orbit-mark.svg' if flat else 'v28-split-orbit-mark-chrome.svg', 1024)
    big = big.crop(big.getchannel('A').getbbox())  # tight to the artwork
    m = big.resize((mw, round(mw * big.height / big.width)), Image.LANCZOS)
    if flat:  # harden the anti-aliasing a little so the F reads at 16 px
        a = m.getchannel('A').point(lambda v: 0 if v < 40 else min(255, int(v * 1.35)))
        m.putalpha(a)
    base.alpha_composite(m, ((n - m.width) // 2, (n - m.height) // 2 + (1 if n <= 16 else 0)))
    return base

if __name__ == '__main__':
    sizes = {16: None, 24: None, 32: '32x32.png', 48: None, 64: '64x64.png', 128: '128x128.png', 256: '128x128@2x.png', 512: 'icon.png'}
    imgs = {n: icon(n) for n in sizes}
    for n, f in sizes.items():
        if f:
            imgs[n].save(os.path.join(ICONS, f))
    imgs[256].save(os.path.join(ICONS, 'icon.ico'), sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
                   append_images=[imgs[n] for n in (16, 24, 32, 48, 64, 128)])
    # hand-check sheet (not shipped)
    sheet = Image.new('RGBA', (16 * 8 + 32 * 8 + 24, 256), (230, 230, 230, 255))
    sheet.paste(imgs[16].resize((128, 128), Image.NEAREST), (0, 0))
    sheet.paste(imgs[32].resize((256, 256), Image.NEAREST), (140, 0))
    sheet.save('/tmp/icon-check.png')
    # NSIS branding (24-bit BMP): sidebar 164x314, header 150x57
    def grad(w, h):
        g = Image.new('RGB', (w, h)); d = ImageDraw.Draw(g)
        for y in range(h):
            t = y / (h - 1); d.line([(0, y), (w, y)], fill=(int(30 - 18 * t), int(34 - 20 * t), int(44 - 26 * t)))
        return g
    sb = grad(164, 314)
    m = svg('v28-split-orbit-mark-chrome.svg', 120); sb.paste(m, (22, 70), m)
    lk = svg('v28-split-orbit-lockup.svg', 148); sb.paste(lk, (8, 200), lk)
    sb.save(os.path.join(INST, 'sidebar.bmp'))
    hd = grad(150, 57); m = svg('v28-split-orbit-mark-chrome.svg', 56); hd.paste(m, (88, 5), m)
    hd.save(os.path.join(INST, 'header.bmp'))
    print('icons written')
