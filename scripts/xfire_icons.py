#!/usr/bin/env python3
"""Extract the classic Xfire game icons (private testing only; the images are NOT committed).

  python scripts/xfire_icons.py <icons.dll> <xfire_games.ini> <out-dir>

icons.dll has a custom "ICONS" resource type whose entries are whole .ico files named
XF_<SHORTNAME>.ICO; xfire_games.ini maps sections ([id] or [id_n]) to LongName/ShortName and
the exes (LauncherExe / DetectExe). Writes <out>/<key>-32.png (+ -16.png when the .ico has a
native 16 px image) and <out>/manifest.json. Needs pefile + Pillow."""
import io, json, os, re, sys, collections
import pefile
from PIL import Image

def norm(s):
    s = s.lower().replace('&', ' and ')
    s = re.sub(r"[™®©:'’.,!?()\[\]\-_/]+", ' ', s)
    return re.sub(r'\s+', ' ', s).strip()

def slug(s):
    return re.sub(r'[^a-z0-9]+', '-', norm(s)).strip('-')[:60]

def read_ini(path):
    games, cur = collections.OrderedDict(), None
    for line in open(path, encoding='latin-1'):
        line = line.strip()
        m = re.match(r'^\[(-?\d+)(?:_\d+)?\]$', line)
        if m:
            cur = games.setdefault(int(m.group(1)), {'id': int(m.group(1)), 'exes': []}); continue
        if line.startswith('[') or cur is None or '=' not in line:
            if line.startswith('['): cur = None
            continue
        k, v = line.split('=', 1)
        if k in ('LongName', 'ShortName') and v and k.lower() not in cur:
            cur[k.lower()] = v.strip()
        elif k in ('LauncherExe', 'DetectExe') and v:
            exe = os.path.basename(v.strip().replace('\\', '/')).lower()
            if exe.endswith('.exe') and exe not in cur['exes']:
                cur['exes'].append(exe)
    return [g for g in games.values() if g.get('shortname') and g['id'] >= 0]

def icons(dll):
    pe = pefile.PE(dll, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_RESOURCE']])
    for t in pe.DIRECTORY_ENTRY_RESOURCE.entries:
        if not (t.name and t.name.string == b'ICONS'):
            continue
        for e in t.directory.entries:
            name = e.name.string.decode()
            d = e.directory.entries[0].data.struct
            yield name, pe.get_memory_mapped_image()[d.OffsetToData:d.OffsetToData + d.Size]

def frames(ico_bytes):
    """{size: RGBA image} for the square frames stored in the .ico (largest bit depth wins)."""
    out = {}
    im = Image.open(io.BytesIO(ico_bytes))
    for sz in sorted(im.info.get('sizes', {im.size})):
        try:
            im.size = sz; f = im.copy().convert('RGBA')
        except Exception:
            continue
        if sz[0] == sz[1]:
            out[sz[0]] = f
    return out

def main(dll, ini, out):
    os.makedirs(out, exist_ok=True)
    by_short = collections.defaultdict(list)
    for g in read_ini(ini):
        by_short[g['shortname'].upper()].append(g)
    manifest, total, mapped = [], 0, 0
    for name, data in icons(dll):
        total += 1
        short = re.sub(r'^XF_|\.ICO$', '', name, flags=re.I).upper()
        try:
            fr = frames(data)
        except Exception as e:
            print('skip', name, e, file=sys.stderr); continue
        if not fr:
            continue
        g = by_short.get(short, [None])[0]
        if g: mapped += 1
        key = f"{slug(g['longname']) if g else 'unmapped'}--{short.lower()}"
        big = fr.get(32) or max(fr.items())[1].resize((32, 32), Image.LANCZOS)
        big.save(os.path.join(out, key + '-32.png'), optimize=True)
        entry = {'key': key, 'shortname': short.lower(), 'file32': key + '-32.png'}
        if 16 in fr:
            fr[16].save(os.path.join(out, key + '-16.png'), optimize=True); entry['file16'] = key + '-16.png'
        if g:
            ids = sorted({x['id'] for x in by_short[short]})
            exes = sorted({e for x in by_short[short] for e in x['exes']})
            entry.update(name=g['longname'], norm=norm(g['longname']), xfire_ids=ids, exes=exes)
        manifest.append(entry)
    json.dump({'source': 'Xfire 2013 icons.dll', 'total_icons': total, 'mapped': mapped, 'icons': manifest},
              open(os.path.join(out, 'manifest.json'), 'w'), indent=1)
    print(f'icons: {total}, written: {len(manifest)}, mapped to a game: {mapped}')

if __name__ == '__main__':
    main(*sys.argv[1:4])
