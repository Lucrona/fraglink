#!/usr/bin/env python3
"""NSIS installer art (dark, understated), 24-bit BMP: the one-click installer splash (splash-100/125/150/200.bmp,
same look as the app's startup window) and the uninstaller header 150x57. (The old wizard sidebar is no longer used.)
Near-black charcoal gradient, flat silver Split Orbit mark and Saira Expanded wordmark. Needs rsvg-convert + Pillow."""
import io, os, subprocess
from PIL import Image, ImageDraw, ImageFont
HERE = os.path.dirname(os.path.abspath(__file__))
INST = os.path.join(HERE, '..', '..', 'apps/desktop/src-tauri/installer')
SILVER = '#D9DDE3'

def svg(name, w, color=SILVER):
    s = open(os.path.join(HERE, name)).read().replace('#FFFFFF', color)
    im = Image.open(io.BytesIO(subprocess.run(['rsvg-convert', '-w', str(w)], input=s.encode(), check=True, capture_output=True).stdout)).convert('RGBA')
    return im.crop(im.getchannel('A').getbbox())

def wordmark(w):
    """The lockup without its mark (the mark is the left 128 of 508.6 viewBox units)."""
    big = svg('v28-split-orbit-lockup.svg', 2034)
    word = big.crop((int(big.width * 132 / 508.6), 0, big.width, big.height))
    word = word.crop(word.getchannel('A').getbbox())
    return word.resize((w, round(w * word.height / word.width)), Image.LANCZOS)

SLOGAN = 'PLAY. CONNECT. TRACK.'
FONT = os.path.join(HERE, 'RussoOne-Regular.ttf')

def slogan(img, y, size=7, spacing=1.6, color=(150, 156, 166, 255)):
    """The slogan in small, letter-spaced caps, centred at height y."""
    f = ImageFont.truetype(FONT, size * 4)
    widths = [f.getlength(c) for c in SLOGAN]
    total = sum(widths) + spacing * 4 * (len(SLOGAN) - 1)
    big = Image.new('RGBA', (int(total) + 8, size * 6), (0, 0, 0, 0)); d = ImageDraw.Draw(big); x = 4
    for c, cw in zip(SLOGAN, widths):
        d.text((x, size), c, font=f, fill=color); x += cw + spacing * 4
    big = big.crop(big.getchannel('A').getbbox())
    small = big.resize((max(1, big.width // 4), max(1, big.height // 4)), Image.LANCZOS)
    img.alpha_composite(small, ((img.width - small.width) // 2, y))

def grad(w, h, top=(26, 28, 32), bottom=(9, 10, 12)):
    g = Image.new('RGBA', (w, h)); d = ImageDraw.Draw(g)
    for y in range(h):
        t = y / (h - 1)
        d.line([(0, y), (w, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,))
    return g

def build():
    sb = grad(164, 314)
    m = svg('v28-split-orbit-mark.svg', 92); sb.alpha_composite(m, ((164 - m.width) // 2, 78))
    wm = wordmark(124); sb.alpha_composite(wm, ((164 - wm.width) // 2, 78 + m.height + 22))
    slogan(sb, 78 + m.height + 22 + wm.height + 10)
    ImageDraw.Draw(sb).line([(0, 0), (0, 313)], fill=(40, 43, 49, 255))  # hairline where it meets the page
    hd = grad(150, 57, (24, 26, 30), (12, 13, 15))
    m2 = svg('v28-split-orbit-mark.svg', 34); hd.alpha_composite(m2, (12, (57 - m2.height) // 2))
    w2 = wordmark(86); hd.alpha_composite(w2, (12 + m2.width + 8, (57 - w2.height) // 2))
    return sb.convert('RGB'), hd.convert('RGB')

# ---- one-click installer splash (0.2.0-dev.16): same look as the app's startup window (ui/splash.html)
SPLASH_W, SPLASH_H = 300, 330          # logical px, matches SPLASH_W/H in windows.rs
SPLASH_BOTTOM = (16, 17, 20)           # flat colour behind the status text (NSIS label background)
BAR = (50, 280, 200, 6)                # x, y, w, h of the progress bar (the NSIS template uses the same numbers)
STATUS = (20, 258, 260, 15)            # x, y, w, h of the status text
SPLASH_SCALES = (100, 125, 150, 200)

def splash(scale):
    k = scale / 100
    W, H = round(SPLASH_W * k), round(SPLASH_H * k)
    S = 3  # supersample
    big = Image.new('RGBA', (W * S, H * S)); px = big.load()
    cx, cy, rx, ry = W * S * .5, H * S * .3, W * S * .6, H * S * .4  # ellipse 120% x 80% at 50% 30%
    stops = [(0, (35, 38, 44)), (.7, (18, 19, 23)), (1, (12, 13, 16))]
    def col(t):
        t = min(t, 1)
        for (a, ca), (b, cb) in zip(stops, stops[1:]):
            if t <= b:
                u = (t - a) / (b - a); return tuple(round(x + (y - x) * u) for x, y in zip(ca, cb))
        return stops[-1][1]
    band0, band1 = (STATUS[1] - 30) * k * S, (STATUS[1] - 4) * k * S
    row = Image.new('RGBA', (W * S, 1))
    for y in range(H * S):
        for x in range(W * S):
            c = col((((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2) ** .5)
            if y > band0:  # fade into the flat bottom band
                u = min(1, (y - band0) / (band1 - band0)); c = tuple(round(a + (b - a) * u) for a, b in zip(c, SPLASH_BOTTOM))
            px[x, y] = c + (255,)
    img = big.resize((W, H), Image.LANCZOS)
    lock = svg_raw('../../apps/desktop/ui/brand/v28-lockup-chrome.svg', round(220 * k))
    img.alpha_composite(lock, ((W - lock.width) // 2, round(74 * k)))
    slogan_k(img, round(74 * k) + lock.height + round(14 * k), k)
    d = ImageDraw.Draw(img)
    bx, by, bw, bh = [round(v * k) for v in BAR]
    d.rounded_rectangle([bx - 1, by - 1, bx + bw, by + bh], radius=max(2, round(3 * k)), outline=(40, 43, 50, 255), fill=(7, 8, 10, 255))
    d.rectangle([0, 0, W - 1, H - 1], outline=(43, 46, 53, 255))  # 1px border like the app window
    return img.convert('RGB')

def svg_raw(rel, w):
    s = open(os.path.join(HERE, rel)).read()
    im = Image.open(io.BytesIO(subprocess.run(['rsvg-convert', '-w', str(w)], input=s.encode(), check=True, capture_output=True).stdout)).convert('RGBA')
    return im.crop(im.getchannel('A').getbbox())

def slogan_k(img, y, k):
    f = ImageFont.truetype(FONT, round(10.5 * k * 4))
    sp = 4.6 * k * 4
    widths = [f.getlength(c) for c in SLOGAN]
    total = sum(widths) + sp * (len(SLOGAN) - 1)
    big = Image.new('RGBA', (int(total) + 16, round(10.5 * k * 4 * 2)), (0, 0, 0, 0)); d = ImageDraw.Draw(big); x = 8
    for c, cw in zip(SLOGAN, widths):
        d.text((x, 4), c, font=f, fill=(169, 177, 188, 255)); x += cw + sp
    big = big.crop(big.getchannel('A').getbbox())
    small = big.resize((max(1, big.width // 4), max(1, big.height // 4)), Image.LANCZOS)
    img.alpha_composite(small, ((img.width - small.width) // 2, y))

def build_splash():
    for sc in SPLASH_SCALES:
        splash(sc).save(os.path.join(INST, f'splash-{sc}.bmp'))

if __name__ == '__main__':
    sb, hd = build()
    hd.save(os.path.join(INST, 'header.bmp'))
    prev = Image.new('RGB', (164 * 2 + 150 * 3 + 60, 314 * 2 + 20), (240, 240, 240))
    prev.paste(sb.resize((328, 628), Image.NEAREST), (10, 10))
    prev.paste(hd.resize((450, 171), Image.NEAREST), (358, 10))
    prev.paste(sb, (358, 200)); prev.paste(hd, (540, 200))
    prev.save('/workspace/fraglink-brand/installer-preview.png')
    build_splash()
    print('installer art written')
