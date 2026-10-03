#!/usr/bin/env python3
"""NSIS installer art (dark, understated): sidebar 164x314 and header 150x57, 24-bit BMP.
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

if __name__ == '__main__':
    sb, hd = build()
    sb.save(os.path.join(INST, 'sidebar.bmp')); hd.save(os.path.join(INST, 'header.bmp'))
    prev = Image.new('RGB', (164 * 2 + 150 * 3 + 60, 314 * 2 + 20), (240, 240, 240))
    prev.paste(sb.resize((328, 628), Image.NEAREST), (10, 10))
    prev.paste(hd.resize((450, 171), Image.NEAREST), (358, 10))
    prev.paste(sb, (358, 200)); prev.paste(hd, (540, 200))
    prev.save('/workspace/fraglink-brand/installer-preview.png')
    print('installer art written')
