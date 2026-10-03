/* Fraglink tray client UI (from the classic client prototype).
   Stats and Activity show real local data from the Rust core via window.FL (backend.js).
   Friends are sample data while signed out; signed in to sync they come from the server (M3). */
(() => {
  'use strict';
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const params = new URLSearchParams(location.search);
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const FLB = window.FL; const TAURI = !!(FLB && FLB.tauri);
  document.body.classList.add('app-mode'); if (TAURI) document.body.classList.add('tauri');
  // `win`: the page IS the window (Tauri, or ?win=1 for browser previews/screenshots)
  if (TAURI || new URLSearchParams(location.search).get('win') === '1') document.body.classList.add('win');
  // live local data from the tracker
  const LIVE = { glance: null, activity: [], fetchedAt: 0, settings: null, friends: null, server: null, friendAct: null, summaries: {} };


  /* ---------------- skinned scrollbars (drawn in DOM so they match the classic skin everywhere) ---------------- */
  function makeScroller(host, both) {
    host.classList.add('gs'); if (!both) host.classList.add('gs-vonly');
    host.innerHTML = '<div class="gs-vp"></div><div class="gs-v"><b class="gs-a up"></b><div class="gs-t"><i class="gs-th"></i></div><b class="gs-a dn"></b></div>' +
      (both ? '<div class="gs-h"><b class="gs-a lf"></b><div class="gs-t"><i class="gs-th"></i></div><b class="gs-a rt"></b></div><div class="gs-c"></div>' : '');
    const vp = host.querySelector('.gs-vp');
    const bars = { v: host.querySelector('.gs-v'), h: host.querySelector('.gs-h') };
    function update() {
      for (const ax of ['v', 'h']) {
        const b = bars[ax]; if (!b) continue;
        const t = b.querySelector('.gs-t'), th = b.querySelector('.gs-th');
        const view = ax === 'v' ? vp.clientHeight : vp.clientWidth, full = ax === 'v' ? vp.scrollHeight : vp.scrollWidth;
        const pos = ax === 'v' ? vp.scrollTop : vp.scrollLeft, len = ax === 'v' ? t.clientHeight : t.clientWidth;
        const size = Math.max(14, Math.round(len * Math.min(1, view / Math.max(1, full))));
        const off = full > view ? Math.round((len - size) * pos / (full - view)) : 0;
        th.style[ax === 'v' ? 'height' : 'width'] = size + 'px'; th.style[ax === 'v' ? 'top' : 'left'] = off + 'px';
        b.classList.toggle('idle', full <= view);
      }
    }
    const by = (dx, dy) => { vp.scrollLeft += dx; vp.scrollTop += dy; update(); };
    host.addEventListener('wheel', e => { e.preventDefault(); by(e.shiftKey ? e.deltaY : e.deltaX, e.shiftKey ? 0 : e.deltaY); }, { passive: false });
    for (const ax of ['v', 'h']) {
      const b = bars[ax]; if (!b) continue;
      const step = 15, sgn = ax === 'v';
      b.querySelectorAll('.gs-a').forEach((a, i) => a.addEventListener('mousedown', e => {
        e.preventDefault(); const d = (i ? 1 : -1) * step; const go = () => sgn ? by(0, d) : by(d, 0); go();
        const iv = setInterval(go, 70); const up = () => { clearInterval(iv); document.removeEventListener('mouseup', up); }; document.addEventListener('mouseup', up);
      }));
      const t = b.querySelector('.gs-t'), th = b.querySelector('.gs-th');
      t.addEventListener('mousedown', e => {
        e.preventDefault();
        if (e.target === th) {
          const z = parseFloat(getComputedStyle(host).zoom) || 1; const eff = (parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--z')) || 1);
          const s0 = sgn ? e.clientY : e.clientX, p0 = sgn ? vp.scrollTop : vp.scrollLeft;
          const ratio = sgn ? (vp.scrollHeight - vp.clientHeight) / Math.max(1, t.clientHeight - th.offsetHeight) : (vp.scrollWidth - vp.clientWidth) / Math.max(1, t.clientWidth - th.offsetWidth);
          const mv = ev => { const d = ((sgn ? ev.clientY : ev.clientX) - s0) / eff * ratio; if (sgn) vp.scrollTop = p0 + d; else vp.scrollLeft = p0 + d; update(); };
          const up = () => { document.removeEventListener('mousemove', mv); document.removeEventListener('mouseup', up); };
          document.addEventListener('mousemove', mv); document.addEventListener('mouseup', up);
        } else {
          const r = th.getBoundingClientRect(); const before = sgn ? e.clientY < r.top : e.clientX < r.left;
          if (sgn) by(0, (before ? -1 : 1) * vp.clientHeight); else by((before ? -1 : 1) * vp.clientWidth, 0);
        }
      });
    }
    return { vp, update };
  }

  /* ---------------- mock data ---------------- */
  // Original 16px game badges (coloured tiles + glyph), not the real game art.
  const GAMES = {
    cs:   { name: 'Counter-Strike 1.6', c1: '#e8e8e8', c2: '#6d6d6d', fg: '#111', g: 'CS', genre: 'FPS' },
    wow:  { name: 'World of Warcraft', c1: '#ffe27a', c2: '#8a5a00', fg: '#3a1c00', g: 'W', genre: 'MMORPG' },
    coh:  { name: 'City of Heroes', c1: '#ff7a7a', c2: '#1e4fb8', fg: '#fff', g: 'H', genre: 'MMORPG' },
    eq2:  { name: 'EverQuest 2', c1: '#8fb4ff', c2: '#c89a20', fg: '#fff', g: 'EQ', genre: 'MMORPG' },
    aa:   { name: "America's Army: Special Forces", c1: '#ffffff', c2: '#b0172a', fg: '#0a2a7a', g: '★', genre: 'FPS' },
    bfv:  { name: 'Battlefield Vietnam', c1: '#f4c542', c2: '#7a1a1a', fg: '#fff', g: 'BV', genre: 'FPS' },
    hl2:  { name: 'Half-Life 2: Deathmatch', c1: '#ffb347', c2: '#b54a00', fg: '#fff', g: 'λ', genre: 'FPS' },
    bf2:  { name: 'Battlefield 2', c1: '#c9d3a0', c2: '#3d4a1c', fg: '#fff', g: 'B2', genre: 'FPS' },
    wc3:  { name: 'Warcraft III: The Frozen Throne', c1: '#9fd0ff', c2: '#1b3f8a', fg: '#fff', g: 'W3', genre: 'RTS' },
    d2:   { name: 'Diablo II: LoD', c1: '#ff9a60', c2: '#5a0a0a', fg: '#fff', g: 'D', genre: 'RPG' },
    ut:   { name: 'Unreal Tournament 2004', c1: '#d0d0ff', c2: '#3a2a8a', fg: '#fff', g: 'U', genre: 'FPS' },
    q3:   { name: 'Quake III Arena', c1: '#ff6a3a', c2: '#3a0a00', fg: '#fff', g: 'Q', genre: 'FPS' },
    rust: { name: 'Rust', c1: '#e3a07a', c2: '#6a2a10', fg: '#fff', g: 'R', genre: 'Survival' },
    hd2:  { name: 'Helldivers 2', c1: '#ffe04a', c2: '#3a3a3a', fg: '#111', g: 'H2', genre: 'Shooter' },
    // pseudo tiles for activity events
    _ach: { name: 'Achievement', c1: '#ffe9a0', c2: '#a8741a', fg: '#3a2200', g: '★', genre: '' },
    _rec: { name: 'Weekly recap', c1: '#9fb8ec', c2: '#2a4a9c', fg: '#fff', g: 'R', genre: '' },
    _new: { name: 'New friend', c1: '#b8f0b0', c2: '#2a7a2a', fg: '#0a2a0a', g: '+', genre: '' },
  };
  // Real games get an original lettered tile: reuse a prototype tile where one exists, else derive colours from the name.
  const TILE_ALIAS = { cs2: 'cs', cs16: 'cs', css: 'cs', quake3: 'q3', d2r: 'd2', diablo2: 'd2', ut2004: 'ut', helldivers2: 'hd2', bfvietnam: 'bfv', everquest2: 'eq2', hl2dm: 'hl2' };
  function ensureGame(id, name) {
    if (!id) return '_new';
    if (GAMES[id] && GAMES[id].real) return id;
    if (GAMES[id] && (!name || GAMES[id].name === name)) { GAMES[id] = { ...GAMES[id], real: true }; return id; }
    const base = GAMES[TILE_ALIAS[id]] || (GAMES[id] && !GAMES[id].real ? GAMES[id] : null);
    if (base) { GAMES[id] = { ...base, name: name || base.name, real: true }; return id; }
    const n = String(name || id);
    let hsh = 0; for (const ch of n) hsh = (hsh * 31 + ch.codePointAt(0)) >>> 0;
    const hue = hsh % 360;
    const words = n.replace(/[^A-Za-z0-9 ]+/g, ' ').trim().split(/\s+/).filter(w => !/^(the|of|a)$/i.test(w));
    const g = ((words[0] || '?')[0] + (words.length > 1 ? words[1][0] : '')).toUpperCase();
    GAMES[id] = { name: n, c1: `hsl(${hue} 70% 72%)`, c2: `hsl(${(hue + 40) % 360} 60% 28%)`, fg: '#fff', g, genre: '', real: true };
    return id;
  }
  /* Real game icons (Xfire style: the game's own 16x16 icon). The app resolves
     them locally (icon cache → the game's exe → Steam's library cache → Steam's
     public icon for friends' games); anything else gets a neutral generic icon. */
  const ICONS = {}; // game id -> data URL | null (none) ; undefined = not asked yet
  const iconWant = new Set(); let iconTimer = null;
  const GENERIC_ICON = '<svg class="gi-gen" viewBox="0 0 16 16"><rect x=".5" y=".5" width="15" height="15" rx="2" fill="#39414f" stroke="#15191f"/><path d="M4.2 6h7.6c1.3 0 2.2 1 2.2 2.3l-.4 2.4c-.2 1-1.4 1.3-2.1.6L10.3 10H5.7l-1.2 1.3c-.7.7-1.9.4-2.1-.6L2 8.3C2 7 2.9 6 4.2 6z" fill="#aab4c4"/><path d="M4 7.3v2M3 8.3h2" stroke="#39414f" stroke-width=".9"/><circle cx="11" cy="7.8" r=".6" fill="#39414f"/><circle cx="12" cy="8.9" r=".6" fill="#39414f"/></svg>';
  /* Fallback icons by genre (original 16x16 drawings) for games without an icon of their own. */
  const GENRE = {}; // game id -> genre, from the signature list (games_catalog)
  let CATALOG = null;
  const gtile = (bg, body) => `<svg class="gi-gen" viewBox="0 0 16 16"><rect x=".5" y=".5" width="15" height="15" rx="2" fill="${bg}" stroke="#0d1117"/>${body}</svg>`;
  const L = '#dde6f3';
  const GENRE_ICONS = {
    fps: gtile('#3a2a2a', `<circle cx="8" cy="8" r="4.3" fill="none" stroke="${L}" stroke-width="1.3"/><path d="M8 1.8v4M8 10.2v4M1.8 8h4M10.2 8h4" stroke="${L}" stroke-width="1.3"/><circle cx="8" cy="8" r=".9" fill="#ff5a4a"/>`),
    rpg: gtile('#2c2a44', `<path d="M13.2 2.2l.6.6-6.6 7.4-1.4-1.4z" fill="${L}"/><path d="M4.2 8.6l3.2 3.2" stroke="#e3b34a" stroke-width="1.5" stroke-linecap="round"/><path d="M5.4 10.6l-2.4 2.4" stroke="#9a6a3a" stroke-width="1.6" stroke-linecap="round"/>`),
    mmo: gtile('#1f3a4a', `<circle cx="8" cy="8" r="5.3" fill="#2f7fb8" stroke="${L}" stroke-width="1"/><path d="M2.8 8h10.4M8 2.7c-2.6 2.4-2.6 8.2 0 10.6M8 2.7c2.6 2.4 2.6 8.2 0 10.6" stroke="${L}" stroke-width=".8" fill="none"/>`),
    strategy: gtile('#3a3424', `<path d="M4.5 3h1.6v1.6h1.1V3h1.6v1.6h1.1V3h1.6v3l-1.2 1.2v4.3h1.4V13H4.3v-1.5h1.4V7.2L4.5 6z" fill="${L}"/>`),
    survival: gtile('#2e2a22', `<path d="M3 13l10-3M3 10l10 3" stroke="#9a6a3a" stroke-width="1.6" stroke-linecap="round"/><path d="M8 2.5c.4 2 2.6 3 2.6 5.5A2.6 2.6 0 0 1 8 10.6 2.6 2.6 0 0 1 5.4 8c0-1.3.7-2 1.2-2.7.2 1 .6 1.4 1 1.5C7.4 5.4 7.4 4 8 2.5z" fill="#ff8a2a"/><path d="M8 6.6c.3 1 1.2 1.3 1.2 2.2a1.2 1.2 0 0 1-2.4 0c0-.6.4-.9.6-1.2z" fill="#ffd34a"/>`),
    sandbox: gtile('#22301f', `<path d="M8 2.5l5 2.7L8 8 3 5.2z" fill="#7cc35a"/><path d="M3 5.2L8 8v5.5l-5-2.7z" fill="#8a5a32"/><path d="M13 5.2L8 8v5.5l5-2.7z" fill="#6a4426"/>`),
    action: gtile('#3a3020', `<path d="M9.6 1.8L4 9h3.4l-1.2 5.2L12 6.8H8.5z" fill="#ffd43a" stroke="#7a5a10" stroke-width=".5"/>`),
    battleroyale: gtile('#23303a', `<path d="M2.5 7.2C3 3.8 5.3 2.3 8 2.3s5 1.5 5.5 4.9C12 6.3 10.2 6.2 8 6.2s-4 .1-5.5 1z" fill="#e2573a"/><path d="M3 7l4.3 5M13 7l-4.3 5M8 6.2V12" stroke="${L}" stroke-width=".6"/><rect x="6.9" y="11.6" width="2.2" height="2.6" rx=".8" fill="${L}"/>`),
    puzzle: gtile('#2a2640', `<path d="M3 5h2.6a1.5 1.5 0 1 1 3 0H11v2.4a1.5 1.5 0 1 1 0 3V13H8.6a1.5 1.5 0 1 0-3 0H3v-2.6a1.5 1.5 0 1 0 0-3z" fill="#b48cff"/>`),
    moba: gtile('#1f2e24', `<rect x="3" y="3" width="10" height="10" fill="#2f5a3a"/><path d="M3.8 12.2L12.2 3.8M3.8 12.2V3.8h8.4M3.8 12.2h8.4V3.8" stroke="#e0cc92" stroke-width="1.1" fill="none"/><circle cx="3.8" cy="12.2" r="1.2" fill="#4aa0ff"/><circle cx="12.2" cy="3.8" r="1.2" fill="#ff5a4a"/>`),
    racing: gtile('#2a2a2a', `<path d="M4 2.5v11" stroke="${L}" stroke-width="1.2"/><path d="M4.6 3h8.4v6H4.6z" fill="#fff"/><path d="M4.6 3h2.1v1.5H4.6zM8.8 3h2.1v1.5H8.8zM6.7 4.5h2.1V6H6.7zM10.9 4.5H13V6h-2.1zM4.6 6h2.1v1.5H4.6zM8.8 6h2.1v1.5H8.8zM6.7 7.5h2.1V9H6.7zM10.9 7.5H13V9h-2.1z" fill="#111"/>`),
    horror: gtile('#2a1c22', `<path d="M8 2.3c3 0 5 2 5 4.7 0 1.6-.8 2.6-1.8 3.2v2.3H4.8v-2.3C3.8 9.6 3 8.6 3 7c0-2.7 2-4.7 5-4.7z" fill="${L}"/><circle cx="6" cy="7.2" r="1.3" fill="#2a1c22"/><circle cx="10" cy="7.2" r="1.3" fill="#2a1c22"/><path d="M7.3 10.2L8 9l.7 1.2zM6.4 11.2v1.3M8 11.2v1.3M9.6 11.2v1.3" stroke="#2a1c22" stroke-width=".7" fill="#2a1c22"/>`),
    card: gtile('#1f3326', `<rect x="2.8" y="3.6" width="6.4" height="9" rx="1" fill="#f4f1e8" stroke="#555" stroke-width=".5" transform="rotate(-12 6 8)"/><rect x="6.6" y="3.2" width="6.4" height="9" rx="1" fill="#fff" stroke="#555" stroke-width=".5" transform="rotate(10 9.8 7.7)"/><path d="M9.9 6.2c.8-.9 2 .1 1.3 1L9.9 8.6 8.6 7.2c-.7-.9.5-1.9 1.3-1z" fill="#d22"/>`),
    fighting: gtile('#3a2320', `<path d="M8 1.8l1.3 3.4 3.5-1.2-1.6 3.3 3.2 1.7-3.6.8.8 3.6-3.1-2-2 3.1-.9-3.6-3.6.7 2.1-3-3.1-1.9 3.6-.6-1-3.5 3.2 1.8z" fill="#ff8a3d"/><path d="M8 5.3l.7 1.8 1.9-.3-1.1 1.6 1.4 1.3-1.9.2-.2 1.9L8 10.6 6.7 11.8l-.2-1.9-1.9-.2 1.4-1.3-1.1-1.6 1.9.3z" fill="#ffe14a"/>`),
    sports: gtile('#1e3322', `<circle cx="8" cy="8" r="5.4" fill="#fff" stroke="#222" stroke-width=".6"/><path d="M8 5.6l2.2 1.6-.8 2.6H6.6l-.8-2.6z" fill="#222"/><path d="M8 5.6V2.7M10.2 7.2l2.8-.9M9.4 9.8l1.7 2.4M6.6 9.8l-1.7 2.4M5.8 7.2L3 6.3" stroke="#222" stroke-width=".7"/>`),
  };
  GENRE_ICONS.platformer = GENRE_ICONS.action; GENRE_ICONS.battle_royale = GENRE_ICONS.battleroyale;
  const fallbackIcon = id => GENRE_ICONS[GENRE[id]] || GENERIC_ICON;
  function appIcon(id) {
    const u = ICONS[id];
    if (u) return `<img class="gi-img" src="${esc(u)}" alt="" draggable="false">`;
    if (u === undefined && id) { iconWant.add(id); clearTimeout(iconTimer); iconTimer = setTimeout(loadIcons, 30); }
    return fallbackIcon(id);
  }
  async function loadIcons() {
    const want = [...iconWant].filter(id => ICONS[id] === undefined).slice(0, 60); iconWant.clear();
    if (!want.length || !FLB.gameIcons) return;
    want.forEach(id => { ICONS[id] = null; }); // don't ask twice while loading
    // "f:<id>" = a friend's game: the server icon wins over the local one
    const groups = [[want.filter(i => !i.startsWith('f:')), false], [want.filter(i => i.startsWith('f:')), true]];
    let changed = false;
    for (const [keys, fr] of groups) {
      if (!keys.length) continue;
      try {
        const got = await FLB.gameIcons(keys.map(k => fr ? k.slice(2) : k), fr);
        for (const k of keys) { const u = got && got[fr ? k.slice(2) : k]; if (u) { ICONS[k] = u; changed = true; } }
      } catch (e) { console.warn('icons', e); }
    }
    if (changed) render();
  }
  // Friends' games use the same icon priority as ours (Xfire, Steam, exe), see icons.rs.
  const friendGameIcon = gameIcon;
  function gameIcon(id) {
    const g = GAMES[id]; if (!g) return '';
    if (g.real) return appIcon(id);
    const gid = 'gi-' + String(id).replace(/[^A-Za-z0-9_-]/g, '_');
    return `<svg viewBox="0 0 16 15"><defs><linearGradient id="${gid}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${g.c1}"/><stop offset="1" stop-color="${g.c2}"/></linearGradient></defs>` +
      `<rect x=".5" y=".5" width="15" height="14" rx="1.5" fill="url(#${gid})" stroke="rgba(0,0,0,.6)"/>` +
      `<rect x="1.5" y="1.2" width="13" height="5" rx="1" fill="#fff" opacity=".28"/>` +
      `<text x="8" y="11" text-anchor="middle" font-family="Tahoma,Verdana,sans-serif" font-weight="bold" font-size="${g.g.length > 1 ? 7 : 9}" fill="${g.fg}">${esc(g.g)}</text></svg>`;
  }
  const SERVERS = ['66.150.14.9:27015', '[EU] Nordic Frag #2', 'Kel\u2019Thuzad (US-PvP)', 'Freedom (Hero)', 'Antonia Bayle', 'Official 24/7 Bridge', 'Operation Hastings', 'Dustbowl Night Owls'];
  let FRIENDS = [
    { id: 'curm', name: 'curmudgeon', kind: 'playing', game: 'cs', server: SERVERS[0], info: 'de_dust2 · 18/20', av: '#6d6d6d', week: 11.4, total: 812 },
    { id: 'dizz', name: 'diZZ_Spinner', kind: 'playing', game: 'wow', server: SERVERS[2], info: 'Lvl 60 Rogue · Stormwind', av: '#b8860b', week: 23.1, total: 503 },
    { id: 'shar', name: 'sharpe_', kind: 'playing', game: 'coh', server: SERVERS[3], info: 'Atlas Park · lvl 32', av: '#1e4fb8', week: 7.8, total: 390 },
    { id: 'tool', name: 'tooTALL', kind: 'playing', game: 'eq2', server: SERVERS[4], info: 'Qeynos Harbor', av: '#c89a20', week: 15.0, total: 720 },
    { id: 'rift', name: 'Riftwalker', kind: 'off', last: '20 minutes ago', status: 'Online', av: '#3a7a3a', week: 4.2, total: 260 },
    { id: 'nyx', name: 'nyx0', kind: 'off', last: '5 hours ago', status: 'Away', av: '#6a3a8a', week: 1.1, total: 88 },
    { id: 'abby', name: 'Abbey', kind: 'fof', game: 'aa', server: SERVERS[5], info: 'via tooTALL · Bridge', av: '#b0172a', week: 0, total: 0 },
    { id: 'jj', name: 'JJ_toasty', kind: 'fof', game: 'bfv', server: SERVERS[6], info: 'via curmudgeon · Hue', av: '#7a1a1a', week: 0, total: 0 },
    { id: 'kr', name: 'Krakatoa', kind: 'off', last: '2 hours ago', av: '#444', week: 6.3, total: 402 },
    { id: 'mo', name: 'mothball', kind: 'off', last: 'yesterday', av: '#444', week: 0.5, total: 97 },
    { id: 'pz', name: 'Pz_Kpfw', kind: 'off', last: '3 days ago', av: '#444', week: 0, total: 1210 },
    { id: 'lu', name: 'lumen', kind: 'off', last: '1 week ago', av: '#444', week: 0, total: 55 },
  ];
  const ME = { name: 'Lucrona', since: 'Mar 2005', game: 'hl2', started: Date.now() - (47 * 60 + 12) * 1000 };
  // tier: tracked (Fraglink/Steam official) | imported (game's own counter, e.g. WoW /played) | estimated (history calculator)
  const MY_HOURS = [
    { game: 'hl2', week: 9.6, today: 1.6, total: 311.4, tier: 'tracked', src: 'Steam playtime', last: 'today' },
    { game: 'wow', week: 6.8, today: 1.1, total: 11214, tier: 'imported', src: '/played via Fraglink addon · 6 characters', last: 'today', note: 'Raided every night through TBC and Wrath' },
    { game: 'wc3', week: 3.2, today: 0, total: 544.0, tier: 'tracked', src: 'Fraglink tracked', last: 'yesterday' },
    { game: 'bf2', week: 2.7, today: 0, total: 188.9, tier: 'tracked', src: 'Fraglink tracked', last: 'Thu' },
    { game: 'cs', week: 0.0, today: 0, total: 4400, tier: 'estimated', src: 'Estimated · 2003–2009', last: 'Sep 18' },
    { game: 'd2', week: 0.0, today: 0, total: 1200, tier: 'estimated', src: 'Estimated · 2001–2004', last: '2004' },
    { game: 'q3', week: 0.0, today: 0, total: 260.5, tier: 'tracked', src: 'Fraglink tracked', last: 'Jul 12' },
    { game: 'rust', week: 0.0, today: 0, total: 97.2, tier: 'tracked', src: 'Steam playtime', last: 'May 2' },
  ];
  const TIERS = { tracked: { label: 'Tracked', sym: '●' }, imported: { label: 'Imported', sym: '◆' }, estimated: { label: 'Estimated', sym: '~' } };
  const sumBy = (arr, f) => arr.reduce((a, b) => a + f(b), 0);
  const VERIFIED = sumBy(MY_HOURS.filter(r => r.tier !== 'estimated'), r => r.total);
  const ESTIMATED = sumBy(MY_HOURS.filter(r => r.tier === 'estimated'), r => r.total);
  const nf = n => Math.round(n).toLocaleString('en-AU');
  function tierMark(t) { const x = TIERS[t]; return `<i class="tm tm-${t}" title="${x.label}">${t === 'estimated' ? '' : x.sym}</i>`; }
  function fmtTotalShort(r) { return r.tier === 'estimated' ? `<span class="est">~${nf(r.total)}h</span>` : `${nf(r.total)}h${tierMark(r.tier)}`; }
  // friends' activity (day-grouped), newest first
  const ACTIVITY = [
    { day: 'Today', items: [
      { who: 'curmudgeon', game: 'rust', html: 'started playing <span class="g">Rust</span>', t: '12m' },
      { who: 'diZZ_Spinner', game: 'wow', html: 'hit <b class="hl">500 hours</b> in <span class="g">World of Warcraft</span>', t: '1h' },
      { who: 'sharpe_', game: '_ach', html: 'unlocked <span class="g">No Days Off · Gold</span> (30-day streak)', t: '3h' },
      { who: 'tooTALL', game: 'eq2', html: 'went <b class="live">LIVE</b> on Twitch with <span class="g">EverQuest 2</span>', t: '4h' },
    ] },
    { day: 'Yesterday', items: [
      { who: 'Riftwalker', game: 'bf2', html: 'played <span class="g">Battlefield 2</span> for the first time', t: '9:40 pm' },
      { who: 'nyx0', game: 'hd2', html: 'played 3 sessions of <span class="g">Helldivers 2</span> · 5.2 h', t: '7:15 pm' },
      { who: 'You', game: '_rec', html: 'weekly recap is ready: <b class="hl">22.3 h</b>, top game HL2:DM', t: '9:00 am', me: true },
    ] },
    { day: 'Wednesday', items: [
      { who: 'Krakatoa', game: 'wow', html: 'imported <span class="g">/played</span> for 11 WoW characters', t: '8:02 pm' },
      { who: 'Pz_Kpfw', game: 'q3', html: 'hit <b class="hl">1,000 hours</b> in <span class="g">Quake III Arena</span>', t: '6:30 pm' },
    ] },
    { day: 'Earlier', items: [
      { who: 'mothball', game: '_new', html: 'is now your friend on Fraglink', t: 'Sep 19' },
    ] },
  ];
  const REPLIES = ['lol yeah', 'one sec, finishing this round', 'join me! server is in my profile', 'gg', 'brb food', 'you on tonight?', 'haha nice', 'need a medic here'];

  /* ---------------- feature flags ---------------- */
  // Tabs hidden for now (code kept). Re-enable by flipping to true, or for
  // testing with ?features=stats,activity or localStorage 'fl-features'.
  const FEATURES = { stats: false, activity: false };
  (params.get('features') || localStorage.getItem('fl-features') || '')
    .split(',').filter(k => k in FEATURES).forEach(k => { FEATURES[k] = true; });
  const tabOn = t => t === 'friends' || FEATURES[t] === true;

  /* ---------------- state ---------------- */
  const state = {
    skin: params.get('skin') || localStorage.getItem('fl-skin') || 'dark',
    tab: ({ hours: 'stats', profile: 'stats' })[params.get('tab')] || params.get('tab') || 'friends',
    collapsed: { online: false, fof: false, off: false },
    sel: null, sort: { key: 'name', dir: 1 }, hoursSort: { key: 'week', dir: -1 },
    chatWith: null, logs: {},
  };
  if (params.get('off') === '1') state.collapsed.off = false;
  if (!tabOn(state.tab)) state.tab = 'friends';
  // Remove the buttons of disabled tabs so no empty tab slots are left.
  $$('[data-tab]').forEach(b => { if (!tabOn(b.dataset.tab)) b.remove(); });

  /* ---------------- skins ---------------- */
  // Dark (the 2013-style resizable window) is the default; Classic Silver is a
  // fixed-size alternative. Phoenix/StreamEngine CSS is kept but not offered.
  const SKINS = ['dark', 'classic', 'phoenix', 'stream'];
  function applySkin(s, persist = true) {
    if (!SKINS.includes(s)) s = 'dark';
    const changed = s !== state.skin;
    state.skin = s;
    document.body.classList.remove(...SKINS.map(k => 'skin-' + k));
    document.body.classList.add('skin-' + s);
    try { localStorage.setItem('fl-skin', s); } catch (e) {}
    // the OS window switches between resizable (dark) and fixed (chassis skins)
    if (TAURI) (persist ? FLB.set('skin', s) : Promise.resolve()).then(() => { if (changed || persist) FLB.scale(currentZoom(), $('#app').classList.contains('side-open')); }).catch(() => {});
    $$('.skin-opt').forEach(b => b.classList.toggle('checked', b.dataset.skin === s));
    $$('input[name=skin]').forEach(r => { r.checked = r.value === s; });
    render();
  }

  /* ---------------- list rendering ---------------- */
  const LS = makeScroller($('#list'), true); const list = LS.vp;
  const SS = makeScroller($('#side-content'), false);
  const CS = makeScroller($('#chat-log'), false);
  function fmtH(h) { return h.toFixed(1) + ' h'; }
  function fmtT(ms) { const s = Math.floor(ms / 1000), h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, x = s % 60; return `${h}:${String(m).padStart(2, '0')}:${String(x).padStart(2, '0')}`; }
  function statusText(f) {
    if (f.kind === 'playing' || f.kind === 'fof') return GAMES[f.game].name + (f.away ? ' (Away)' : '');
    if (f.kind === 'online' || f.kind === 'req' || f.kind === 'out') return f.status;
    return 'Offline';
  }
  function row(f, cls = '') {
    const icon = f.game && f.kind !== 'off' ? `<i class="gi">${friendGameIcon(f.game)}</i>` : '<i class="noicon"></i>';
    const info = f.kind === 'playing' || f.kind === 'fof' ? f.server : f.kind === 'off' ? (f.last ? 'Last online ' + f.last : '') : (f.info || '');
    return `<div class="xf-row ${cls} ${state.sel === f.id ? 'sel' : ''}" data-friend="${f.id}"><span class="c-name">${icon}${f.fav ? '<i class="fav" title="Favourite">★</i>' : ''}${esc(f.name)}</span><span class="c-status">${esc(statusText(f))}</span><span class="c-info">${esc(info)}</span></div>`;
  }
  function sec(key, label, cls, n) {
    const c = state.collapsed[key];
    return `<div class="xf-sec ${cls} ${c ? 'collapsed' : ''} ${state.sel === 'g:' + key ? 'sel' : ''}" data-sec="${esc(key)}"><i class="tri"></i>${label}${n != null ? ` <small>(${n})</small>` : ''}</div>`;
  }
  /* Settings the main window follows (written by Options and the menus). */
  const SET = () => (LIVE.settings && LIVE.settings.settings) || {};
  const setOn = (k, def = true) => { const v = SET()[k]; return v == null ? def : v === '1'; };
  const jsonList = k => { try { const v = JSON.parse(SET()[k] || '[]'); return Array.isArray(v) ? v.filter(x => typeof x === 'string') : []; } catch (e) { return []; } };
  async function saveSetting(k, v) {
    if (LIVE.settings) (LIVE.settings.settings = LIVE.settings.settings || {})[k] = String(v);
    try { await FLB.set(k, v); } catch (e) { toastErr(e); }
  }
  const GROUP_NAMES = { online: 'Friends Online', fof: 'Friends of Friends Playing', req: 'Friend Requests', off: 'Friends Offline' };
  const GROUP_HELP = {
    online: 'Friends who are signed in to Fraglink right now. Friends in a game are listed first, with the game in the Status column.',
    fof: 'Friends of your friends who are in a game right now. Click one to see their game and the friends you have in common.',
    req: 'Friend requests you have sent or received. Select one to accept, decline or cancel it.',
    off: 'Friends who are not signed in. Click a heading to fold or unfold it. Turn this list off with Tools > Friends > Show Offline Friends.',
    custom: 'A group you made. Moving friends into custom groups comes in a later update; for now it is an empty heading.',
  };
  // The grouped list shared by sample and real friends.
  const groupOf = () => { try { const o = JSON.parse(SET().friend_groups || '{}'); return o && typeof o === 'object' ? o : {}; } catch (e) { return {}; } };
  function groupsHtml(F, note) {
    const hidden = jsonList('hidden_groups');
    const groups = jsonList('custom_groups'), moved = groupOf();
    const inGroup = f => groups.includes(moved[f.handle || f.id]);
    const favs = jsonList('favorite_friends');
    F = F.map(f => favs.includes(f.handle || f.id) ? { ...f, fav: true } : f);
    const custom = Object.fromEntries(groups.map(g => [g, sorted(F.filter(f => moved[f.handle || f.id] === g))]));
    F = F.filter(f => !inGroup(f));
    const on = sorted(F.filter(f => f.kind === 'playing')).concat(sorted(F.filter(f => f.kind === 'online')));
    const fof = sorted(F.filter(f => f.kind === 'fof')), reqs = F.filter(f => f.kind === 'req' || f.kind === 'out'), off = sorted(F.filter(f => f.kind === 'off'));
    const block = (key, label, cls, rows, rowCls = '') => {
      if (hidden.includes(key)) return '';
      return sec(key, label, cls, rows.length) + (state.collapsed[key] ? '' : rows.map(f => row(f, rowCls)).join(''));
    };
    let h = note || '';
    h += block('online', GROUP_NAMES.online, 's-on', on);
    if (setOn('show_fof')) h += block('fof', 'Friends of Friends Playing', 's-fof', fof, 'fof');
    if (reqs.length) h += block('req', GROUP_NAMES.req, 's-fof', reqs, 'req');
    for (const g of groups) h += block('c:' + g, esc(g), 's-cus', custom[g]);
    if (setOn('show_offline')) h += block('off', GROUP_NAMES.off, 's-off', off, 'off');
    return h;
  }
  function sorted(arr) {
    const { key, dir } = state.sort;
    return [...arr].sort((a, b) => (key === 'name' ? a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) : statusText(a).localeCompare(statusText(b))) * dir);
  }
  /* Real friends (signed in to sync) mapped onto the same row shape as the samples. */
  const realMode = () => !!(LIVE.friends && LIVE.friends.signed_in);
  function sinceText(iso) {
    if (!iso) return '';
    const m = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
    return m < 1 ? 'just now' : m < 60 ? `for ${m} min` : `for ${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
  }
  function realFriends() {
    const l = LIVE.friends && LIVE.friends.list; if (!l) return [];
    const out = [];
    for (const f of l.friends || []) {
      const p = f.presence || {}, name = f.display_name || f.handle, base = { id: 'u:' + f.handle, handle: f.handle, name, real: true, since: f.since, presence: p, week: 0, total: 0 };
      if (p.status !== 'offline' && p.game_id) out.push({ ...base, kind: 'playing', game: ensureGame(p.game_id, p.game_name), server: sinceText(p.session_started_at), info: '', away: p.status === 'away' });
      else if (p.status === 'online' || p.status === 'away') out.push({ ...base, kind: 'online', status: p.status === 'away' ? 'Away' : 'Online' });
      else out.push({ ...base, kind: 'off', last: '' });
    }
    for (const q of l.incoming || []) out.push({ id: 'u:' + q.handle, handle: q.handle, name: q.display_name || q.handle, kind: 'req', status: 'Wants to add you', info: '', real: true, incoming: true });
    for (const q of l.outgoing || []) out.push({ id: 'u:' + q.handle, handle: q.handle, name: q.display_name || q.handle, kind: 'out', status: 'Request sent', info: '', real: true });
    return out;
  }
  const friendsNow = () => realMode() ? realFriends() : FRIENDS;
  /* Signed out: the login form sits in the main window (like the classic client). Fraglink signs
     in through the browser (device pairing), so there's no password box here. */
  const signedOut = () => params.get('login') === '1' || (TAURI && !!LIVE.friends && !LIVE.friends.signed_in);
  const srvVal = () => ($('#lg-server') && $('#lg-server').value.trim()) || (LIVE.sync && LIVE.sync.server) || '';
  function loginHtml() {
    const sy = LIVE.sync || {}, p = sy.pairing, err = sy.pairing_error || sy.last_error;
    const used = jsonList('login_history'), last = SET().login_last || used[0] || '';
    let h = `<div class="lg-wrap"><div class="lg">`;
    if (p) {
      const left = Math.max(0, Math.round((p.expires_at || 0) - Date.now() / 1000));
      h += `<div class="lg-prog"><div class="lg-status">Signing in... confirm in your browser</div><div class="det-track"><div class="det-bar lg-anim"></div></div>
        <div class="lg-dim" id="lg-tmo" data-exp="${p.expires_at || 0}">Time out in ${left} secs</div>
        <p>Code: <span class="lg-code">${esc(p.user_code)}</span></p><p class="lg-dim">Browser didn't open? Go to<br>${esc(p.verification_uri)}</p>
        <button class="hw" type="button" data-action="login-cancel">Cancel</button></div>`;
    }
    else if (state.loginMode === 'create') {
      h += `<b class="lg-lbl">Create Account</b>
        <label class="lg-lbl" for="lg-nuser">Username</label><input id="lg-nuser" maxlength="20" autocomplete="off" placeholder="3-20 letters, digits, _ or -" />
        <label class="lg-lbl" for="lg-npass">Password</label><input id="lg-npass" type="password" maxlength="128" placeholder="at least 10 characters" />
        <label class="lg-lbl" for="lg-npass2">Confirm Password</label><input id="lg-npass2" type="password" maxlength="128" />
        <label class="lg-lbl" for="lg-email">Email <span class="lg-opt">(optional)</span></label><input id="lg-email" maxlength="254" autocomplete="off" />
        <div class="af-btns"><button class="hw" type="button" data-action="login-back">Cancel</button><button class="hw" type="button" data-action="signup-go">Create Account</button></div>`;
    } else {
      h += `<label class="lg-lbl" for="lg-user">Username</label>
        <input id="lg-user" list="lg-users" maxlength="254" autocomplete="off" value="${esc(last)}" placeholder="&lt;New User&gt;" /><datalist id="lg-users">${used.map(u => `<option value="${esc(u)}">`).join('')}</datalist>
        <a class="lg-a" data-action="register">Create Account</a>
        <label class="lg-lbl" for="lg-pass">Password</label>
        <div class="lg-row"><input id="lg-pass" type="password" maxlength="128" /><button class="hw" type="button" data-action="login-go">Login</button></div>
        <a class="lg-a" data-action="forgot">Forgot Password</a>
        <label class="lg-tick"><input type="checkbox" id="lg-save" ${setOn('save_password') ? 'checked' : ''} /> Save Password</label>
        <label class="lg-tick"><input type="checkbox" id="lg-auto" ${setOn('auto_login') ? 'checked' : ''} /> Auto-Login</label>
        <a class="lg-a lg-alt" data-action="login-browser">Sign in with your browser instead</a>`;
    }
    if (!p) h += `<details class="lg-srv"><summary>Server</summary><input id="lg-server" maxlength="200" value="${esc(sy.server || '')}" /></details>`;
    if (state.loginBusy) h += `<div class="lg-status">${esc(state.loginBusy)}</div>`;
    if (state.loginErr) h += `<div class="bad">${esc(state.loginErr)}</div>`;
    if (err) h += `<div class="bad">${esc(err)}</div>`;
    return h + `</div></div>`;
  }
  function offlineSide() {
    return `<h4>Learn About Fraglink</h4><ul class="lg-learn"><li>${GENRE_ICONS.fps}<a data-action="about">See what friends are playing and join them</a></li><li>${GENRE_ICONS.strategy}<a data-action="options" data-page="games">Track your hours in every game</a></li></ul>
      <h4>New Users</h4><p class="side-help">Set up a free Fraglink account so you can log in and see your friends. Game tracking works without one.</p><ul class="lg-res"><li><a data-action="register">New User Registration</a></li></ul>
      <h4>Resources</h4><ul class="lg-res"><li><a data-action="web" data-page="faq">Frequently Asked Questions</a></li><li><a data-action="web" data-page="support">Technical Support Forum</a></li><li><a data-action="web" data-page="releases">Latest Release Notes</a></li></ul>`;
  }
  function addFriendHtml() {
    const A = state.addFriend;
    if (A.step === 'search') return `<div class="lg-wrap"><div class="lg af-in"><label for="add-handle">Search by name, email, or Fraglink username:</label>
      <input id="add-handle" maxlength="60" autocomplete="off" value="${esc(A.q)}" />
      <div class="af-ex">(Example: Jo, Jo Doe, or jo@doe.com)</div>
      <div class="af-btns"><button class="hw" type="button" data-action="add-cancel">Cancel</button><button class="hw" type="button" id="add-search" data-action="add-friend-go"${A.q ? '' : ' disabled'}>Search</button></div>
      <a class="lg-a" data-action="invite">Invite friends to join Fraglink</a></div></div>`;
    const us = A.users, sel = A.sel || '';
    const rows = us == null ? '<tr><td colspan="2">Searching…</td></tr>' : us.length ? us.map(u => `<tr class="${u.handle === sel ? 'sel' : ''}" data-action="add-pick" data-handle="${esc(u.handle)}"><td>${esc(u.handle)}</td><td>${esc(u.display_name || u.handle)}</td></tr>`).join('') : `<tr><td colspan="2">${esc(A.err || 'No users found. Check the spelling, or invite them to join.')}</td></tr>`;
    const n = us ? us.length : 0;
    return `<div class="lg-wrap af-wrap"><div class="lg af-in af-res"><div class="af-h"><span>Search Results (select one):</span><span>${us == null ? '' : `${n} User${n === 1 ? '' : 's'} Found`}</span></div>
      <table class="af-t"><tr><th>Username</th><th>Display Name</th></tr>${rows}</table>
      <label for="add-msg-text">Personal Message:</label><textarea id="add-msg-text" maxlength="200" rows="3">Add me to your friends list</textarea>
      <div class="af-btns"><button class="hw" type="button" data-action="add-back">Back</button><button class="hw" type="button" data-action="friend-profile" data-handle="${esc(sel)}"${sel ? '' : ' disabled'}>View Profile</button><button class="hw" type="button" data-action="add-friend-send" data-handle="${esc(sel)}"${sel ? '' : ' disabled'}>Add Friend</button></div>
      <div id="add-msg"></div><a class="lg-a" data-action="add-cancel">Back to friends list</a></div></div>`;
  }
  function renderFriends() {
    if (state.addFriend && !state.detectRun) { list.innerHTML = addFriendHtml(); return; }
    if (state.detectRun) { list.innerHTML = `<div class="lg"><div class="lg-h">Detecting installed games…</div><p>Looking for games from Steam, Battle.net, Epic, GOG, EA app, Ubisoft Connect and Xbox…</p><div class="det-track"><div class="det-bar" id="fd-bar" style="width:${state.detectPct}%"></div></div><button class="dk-btn" data-action="detect-skip">Skip</button></div>`; return; }
    const s = state.sort;
    let h = `<div class="xf-grid"><div class="xf-head"><span class="c-name sort ${s.key === 'name' && s.dir < 0 ? 'desc' : ''}" data-sort="name">Name</span><span class="c-status" data-sort="status">Status</span><span class="c-info">Game Info</span></div>`;
    if (realMode()) {
      const F = realFriends(), fs = LIVE.friends;
      let note = '';
      if (!fs.list) note = `<div class="fl-mock">${fs.connected ? 'Loading friends…' : 'Connecting to your Fraglink server…'}</div>`;
      else if (!fs.connected) note = `<div class="fl-mock">Offline: friends' status may be out of date. Reconnecting…</div>`;
      h += groupsHtml(F, note);
      if (fs.list && !(fs.list.friends || []).length && !F.some(f => f.kind === 'req' || f.kind === 'out')) h += `<div class="fl-empty">No friends yet. <a data-action="add-friend">Add a friend</a> by their Fraglink username.</div>`;
    } else {
      if (TAURI || params.get('login') === '1') { list.innerHTML = loginHtml(); return; }
      h += groupsHtml(FRIENDS, `<div class="fl-mock">Sample friends (browser preview).</div>`);
    }
    list.innerHTML = h + '</div>';
  }
  const hrs = sec => sec / 3600;
  function fmtHrs(sec) { const h = hrs(sec); return h >= 100 ? nf(h) : h.toFixed(1); }
  function fmtDur(sec) { const m = Math.round(sec / 60); return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m} min`; }
  function liveSeconds() {
    const np = LIVE.glance && LIVE.glance.now_playing; if (!np) return 0;
    return np.active_seconds + (np.idle ? 0 : Math.max(0, Math.floor((Date.now() - LIVE.fetchedAt) / 1000)));
  }
  function renderStats() {
    const g = LIVE.glance;
    let h = `<div class="st"><div class="xf-head st-head"><span class="c-name st-g">Game</span><span class="st-w">7 Days</span><span class="st-t">Total</span></div>`;
    if (!g) { list.innerHTML = h + `<div class="fl-empty">Loading your stats…</div></div>`; return; }
    h += `<div class="xf-sec s-now"><i class="tri"></i>Now Playing</div>`;
    const np = g.now_playing;
    if (np) { const id = ensureGame(np.game_id, np.name); h += `<div class="xf-row st-row" data-game="${esc(id)}" title="${esc(np.name)} · detected via ${esc(String(np.source).replace('_', ' '))}"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(np.name)}</span></span><span class="st-live timer" id="session">${fmtT(liveSeconds() * 1000)}</span></div>`; }
    else h += `<div class="xf-row st-row fl-muted"><span class="c-name st-g"><i class="noicon"></i><span class="nm">Not in a game · watching</span></span><span class="st-live"></span></div>`;
    const change = g.last_week_seconds > 0 ? Math.round((g.week_seconds - g.last_week_seconds) / g.last_week_seconds * 100) : null;
    const chg = change === null ? '' : `<em class="${change < 0 ? 'down' : ''}" title="vs all of last week">${change < 0 ? '&#9660;' : '&#9650;'}${Math.abs(change)}%</em>`;
    h += `<div class="st-sum"><div><small>Today</small><b>${fmtHrs(g.today_seconds)} h</b></div><div><small>This week</small><b>${fmtHrs(g.week_seconds)} h</b>${chg}</div><div><small>Streak</small><b>${g.streak_days} day${g.streak_days === 1 ? '' : 's'}</b></div></div>`;
    h += `<div class="xf-sec s-fof"><i class="tri"></i>Top Games This Week</div>`;
    const top = g.top || []; const max = Math.max(1, ...top.map(r => r.seven_day_seconds));
    if (!top.length) h += `<div class="fl-empty">No games tracked yet. Start a game, or use <a data-action="tag-game">Fraglink → This is a game…</a></div>`;
    h += top.map(r => { const id = ensureGame(r.game_id, r.name); return `<div class="xf-row st-row" data-game="${esc(id)}" title="${esc(r.name)} · ${fmtDur(r.total_seconds)} tracked on this PC"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(r.name.replace(/:.*/, ''))}</span></span><span class="st-w"><i class="st-bar"><b style="width:${Math.round(r.seven_day_seconds / max * 100)}%"></b></i>${hrs(r.seven_day_seconds).toFixed(1)}</span><span class="st-t">${fmtHrs(r.total_seconds)}h${tierMark(r.tier || 'tracked')}</span></div>`; }).join('');
    h += `<div class="st-legend">${tierMark('tracked')}Tracked on this PC · ${g.games_tracked} game${g.games_tracked === 1 ? '' : 's'} · ${fmtHrs(g.total_seconds)} h total</div>`;
    h += serverStatsHtml(g);
    h += `<div class="st-link"><a data-action="tag-game">&raquo; This is a game…</a> &nbsp; <a data-action="my-stats">&raquo; Full stats</a></div></div>`;
    list.innerHTML = h;
  }
  /* Account totals from the server (all devices + Steam + imports + estimates). */
  // Same rounding as the server: estimates over 1,000 h go to the nearest 100.
  const roundEst = h => h > 1000 ? Math.floor((h + 50) / 100) * 100 : h;
  function serverStatsHtml(g) {
    if (!realMode()) return '';
    const sv = LIVE.server;
    let h = `<div class="xf-sec s-now"><i class="tri"></i>My Account (all devices)</div>`;
    if (!sv) return h + `<div class="fl-empty">Server totals unavailable right now, so this shows this PC only.</div>`;
    const local = {}; for (const r of g.top || []) local[r.game_id] = r.total_seconds / 60;
    const rows = (sv.games || []).filter(r => r.display_min > 0 || r.estimated_min > 0).slice(0, 8);
    if (!rows.length) h += `<div class="fl-empty">Nothing on your account yet. Sessions upload a few minutes after they end.</div>`;
    h += rows.map(r => {
      const id = ensureGame(r.game_id, r.name), est = r.display_min <= 0;
      // Never show less than this PC already knows (uploads lag a few minutes).
      const mins = est ? r.estimated_min : Math.max(r.display_min, r.display_tier === 'tracked' ? (local[r.game_id] || 0) : 0);
      const tot = est ? `<span class="est">~${nf(roundEst(Math.floor(mins / 60)))}h</span>` : `${fmtHrs(mins * 60)}h${tierMark(r.display_tier)}`;
      const tip = `${r.name} · tracked ${nf(r.tracked_min / 60)}h · imported ${nf(r.imported_min / 60)}h · estimated ~${nf(r.estimated_min / 60)}h`;
      return `<div class="xf-row st-row" data-game="${esc(id)}" title="${esc(tip)}"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(r.name.replace(/:.*/, ''))}</span></span><span class="st-w">${r.minutes_7d ? (r.minutes_7d / 60).toFixed(1) : ''}</span><span class="st-t">${tot}</span></div>`;
    }).join('');
    const o = sv.overall || {};
    h += `<div class="st-legend">${tierMark('tracked')}Tracked ${tierMark('imported')}Imported <span class="est">~</span> Estimated · ${nf((o.verified_min || 0) / 60)} h${o.estimated_min ? ` + ${esc(o.estimated_display || '')} est.` : ''}</div>`;
    return h;
  }
  const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
  function dayLabel(ts) {
    const d = new Date(ts * 1000), t = new Date(); const start = x => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const diff = Math.round((start(t) - start(d)) / 864e5);
    return diff <= 0 ? 'Today' : diff === 1 ? 'Yesterday' : diff < 7 ? WEEKDAYS[d.getDay()] : 'Earlier';
  }
  function whenLabel(ts) {
    const d = new Date(ts * 1000), lbl = dayLabel(ts);
    if (lbl === 'Today') { const m = Math.floor((Date.now() / 1000 - ts) / 60); return m < 1 ? 'now' : m < 60 ? m + 'm' : Math.floor(m / 60) + 'h'; }
    if (lbl === 'Earlier') return d.toLocaleDateString('en-AU', { day: 'numeric', month: 'short' });
    return d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  }
  function activityHtml(a) {
    const g = `<span class="g">${esc(a.game_name || a.game_id || 'a game')}</span>`, p = a.payload || {};
    switch (a.kind) {
      case 'started': return `started playing ${g}`;
      case 'stopped': return `played ${g} for <b class="hl">${fmtDur(p.active_seconds || 0)}</b>${p.recovered ? ' <small>(recovered)</small>' : ''}`;
      case 'first_time': return `played ${g} for the first time on Fraglink`;
      case 'milestone': return `hit <b class="hl">${nf(p.hours)} hours</b> in ${g}`;
      case 'tagged': return `tagged ${g} as a game`;
      case 'excluded': return `excluded ${g} from tracking`;
      default: return `${esc(a.kind)} ${g}`;
    }
  }
  function renderActivity() {
    let h = `<div class="av"><div class="xf-head av-head"><span class="c-name" style="width:auto;flex:1">Your Activity</span><span class="av-when">When</span></div>`;
    h += `<div class="fl-mock">Friends' activity arrives in a later update. For now this is what Fraglink logged on this PC.</div>`;
    if (!LIVE.activity.length) h += `<div class="fl-empty">Nothing yet. Start a game and it shows up here.</div>`;
    let cur = null;
    for (const a of LIVE.activity) {
      const lbl = dayLabel(a.ts);
      if (lbl !== cur) { cur = lbl; h += `<div class="xf-sec s-day"><i class="tri"></i>${esc(lbl)}</div>`; }
      const kindTile = a.game_id ? ensureGame(a.game_id, a.game_name) : '_new';
      h += `<div class="av-row" title="${esc(new Date(a.ts * 1000).toLocaleString())}"><i class="gi">${gameIcon(kindTile)}</i><div class="av-tx"><b class="me">You</b> ${activityHtml(a)}</div><span class="av-t">${esc(whenLabel(a.ts))}</span></div>`;
    }
    h += `<div class="st-link"><a data-action="website">&raquo; All activity on fraglink.gg (later)</a></div></div>`;
    list.innerHTML = h;
  }
  function render() {
    $$('.xf-tab, .xf-icontabs [data-tab], .dk-vtabs [data-tab]').forEach(t => t.classList.toggle('active', t.dataset.tab === state.tab));
    $('#sechead').textContent = state.tab === 'friends' && state.detectRun ? 'Detect Games' : state.tab === 'friends' && state.addFriend ? 'Add Friend' : state.tab === 'friends' && signedOut() ? 'Login' : ({ friends: 'Friends', stats: 'Stats', activity: 'Activity' })[state.tab] || '';
    $('#list').classList.toggle('nohbar', state.tab !== 'friends');
    const sl = list.scrollLeft, st = list.scrollTop;
    if (state.tab === 'stats') renderStats(); else if (state.tab === 'activity') renderActivity(); else renderFriends();
    list.scrollLeft = sl; list.scrollTop = st; LS.update();
    renderSide();
  }

  /* ---------------- side panel ---------------- */
  function renderSide() {
    const el = SS.vp; requestAnimationFrame(SS.update);
    if (signedOut() && !state.detectRun) { el.innerHTML = offlineSide(); return; }
    // A selected group heading only highlights the row; the panel keeps its default content.
    const f = friendsNow().find(x => x.id === state.sel);
    if (f) {
      // Xfire-style: big name, username, Profile link, then status or the game
      const H = esc(f.handle || f.name), playing = f.kind === 'playing' || f.kind === 'fof';
      let h = `<div class="sf-name">${esc(f.name)}</div><div class="sf-user">Username: ${H}</div><a data-action="friend-profile" data-handle="${H}">Profile</a><div class="sep"></div>`;
      if (f.kind === 'req') h += `<div>Wants to be your friend.</div><a data-action="friend-accept" data-handle="${H}">Accept</a> · <a data-action="friend-decline" data-handle="${H}">Decline</a> · <a data-action="friend-block" data-handle="${H}">Block</a>`;
      else if (f.kind === 'out') h += `<div>Friend request sent. They'll show up here once they accept.</div><a data-action="friend-remove" data-handle="${H}">Cancel request</a>`;
      else if (playing) {
        const mins = sessionMinutes(f);
        h += `<div class="sf-lbl">Playing Game</div><div class="act"><i class="gi">${friendGameIcon(f.game)}</i><div><b>${esc(GAMES[f.game].name)}</b>${mins != null ? `<br><span class="sf-for">Playing for ${esc(fmtMins(mins))}</span>` : f.server ? '<br>' + esc(f.server) : ''}${!f.real && f.info ? `<br><span style="color:var(--cyan)">${esc(f.info)}</span>` : ''}</div></div>`;
      }
      else h += `<div class="sf-lbl">Status</div><div>${esc(statusText(f))}${f.last ? ' · last online ' + esc(f.last) : ''}</div>`;
      if (f.kind !== 'req' && f.kind !== 'out') h += weekHtml(f);
      if (f.kind === 'fof') { const m = mutualOf(f); h += `<div class="sep"></div><h4>Mutual Friends <small>(${m.length})</small></h4><ul class="games">${m.map(x => `<li><i class="noicon"></i><span>${esc(x)}</span></li>`).join('') || '<li><span>None known</span></li>'}</ul>`; }
      if (f.real && (f.kind === 'playing' || f.kind === 'online' || f.kind === 'off')) {
        const muted = (LIVE.friends.muted || []).includes(f.handle.toLowerCase());
        h += `<div class="sep"></div><div><span style="color:#8ea3c8">Friends since:</span> <b>${f.since ? esc(new Date(f.since).toLocaleDateString('en-AU', { day: 'numeric', month: 'short', year: 'numeric' })) : '?'}</b></div><a data-action="friend-mute" data-handle="${H}" data-mute="${muted ? 0 : 1}">${muted ? 'Unmute' : 'Mute'} pop-ups</a>`;
      }
      el.innerHTML = h + `<div class="sep"></div><a data-action="clear-sel">« My Games</a>`;
      return;
    }
    // Nothing selected (Xfire layout): Activity Report, then the games on this PC with Launch links.
    el.innerHTML = activityReportHtml() + installedHtml();
  }
  /* Friends' recent activity (server: presence + finished sessions, friends only, opt-out per user). */
  function activityReportHtml() {
    const a = LIVE.friendAct, items = a && a.items ? a.items.slice(0, 6) : null;
    let rows;
    if (items === null) rows = `<div class="ar-empty">${realMode() || !TAURI ? 'Loading…' : 'No recent activities from friends.'}</div>`;
    else if (!items.length) rows = '<div class="ar-empty">No recent activities from friends.</div>';
    else rows = items.map(x => {
      const id = ensureGame(x.game_id || ('n:' + x.game_name), x.game_name || 'a game');
      const g = `<span class="g">${esc(x.game_name || 'a game')}</span>`;
      const what = x.kind === 'playing' ? `started playing ${g}` : `played ${g} for ${esc(fmtMins(Math.round((x.duration_s || 0) / 60)))}`;
      return `<div class="ar-row" title="${esc(new Date(x.at).toLocaleString())}"><i class="gi">${friendGameIcon(id)}</i><div><b>${esc(x.display_name || x.handle)}</b> ${what}<small>${esc(agoText(x.at))}</small></div></div>`;
    }).join('');
    return `<div class="side-sec"><h4>Activity Report</h4><a class="side-more" data-action="activity-more">View More</a></div>${rows}<div class="sep"></div>`;
  }
  function installedHtml() {
    const inst = (CATALOG || []).filter(g => g.installed && !g.excluded);
    const games = CATALOG === null ? '<li><span>Looking for games…</span></li>'
      : inst.map(g => { const id = ensureGame(g.id, g.name); return `<li title="${esc(g.game_path || g.detect_path || '')}"><i class="gi">${gameIcon(id)}</i><span>${esc(g.name)}</span>${g.can_launch ? `<a data-action="launch" data-game="${esc(g.id)}">Launch</a>` : ''}</li>`; }).join('') || '<li><span>No games found on this PC yet.</span></li>';
    return `<div class="side-sec"><h4>${inst.length} Installed Game${inst.length === 1 ? '' : 's'}</h4><a data-action="detect-games" class="side-more side-detect">Detect Games</a></div><ul class="games launch">${games}</ul>`;
  }
  function fmtMins(m) { m = Math.max(0, Math.round(m)); return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m}m`; }
  function agoText(iso) {
    const m = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
    return m < 1 ? 'just now' : m < 60 ? `${m}m ago` : m < 1440 ? `${Math.floor(m / 60)}h ago` : `${Math.floor(m / 1440)}d ago`;
  }
  // minutes in the current session: real friends from presence, sample friends get a stable stand-in
  function sessionMinutes(f) {
    if (f.real) { const t = f.presence && f.presence.session_started_at; return t ? (Date.now() - Date.parse(t)) / 60000 : null; }
    let h = 0; for (const c of f.id) h = (h * 31 + c.charCodeAt(0)) % 997; return 20 + h % 160;
  }
  function weekHtml(f) {
    if (!f.real) return `<div class="sf-week"><span>This week:</span> <b>${esc(String(f.week ?? 0))} h</b></div>`;
    const s = LIVE.summaries[f.handle];
    if (!s) { loadSummary(f.handle); return '<div class="sf-week"><span>This week:</span> …</div>'; }
    if (s.err) return '';
    if (s.week_seconds == null) return `<div class="sf-week"><span>This week:</span> <em>not shared</em></div>`;
    return `<div class="sf-week"><span>This week:</span> <b>${(s.week_seconds / 3600).toFixed(1)} h</b></div>`;
  }
  const summaryAt = {};
  function loadSummary(h) {
    if (!FLB.friendSummary || (summaryAt[h] && Date.now() - summaryAt[h] < 60000)) return;
    summaryAt[h] = Date.now();
    FLB.friendSummary(h).then(s => { LIVE.summaries[h] = s; renderSide(); }).catch(() => { LIVE.summaries[h] = { err: true }; });
  }
  async function refreshActivity() {
    if (!FLB.friendsActivity || (TAURI && !realMode())) { if (TAURI) LIVE.friendAct = { items: [] }; return; }
    try { LIVE.friendAct = await FLB.friendsActivity(); } catch (e) { LIVE.friendAct = LIVE.friendAct || { items: [] }; }
    for (const k in summaryAt) delete summaryAt[k];
    renderSide();
  }
  /* Drag the info panel's outer edge to make it wider (height follows the window). */
  const SIDE_MIN = 180, SIDE_MAX = 560;
  function setSideW(w, save) {
    w = Math.round(Math.max(SIDE_MIN, Math.min(SIDE_MAX, w)));
    document.documentElement.style.setProperty('--side-w', w + 'px');
    state.sideW = w; requestAnimationFrame(SS.update);
    if (save && TAURI) FLB.sideWidth(w).catch(() => {});
    if (save && !TAURI) try { localStorage.setItem('fl-side-w', w); } catch (e) {}
  }
  function toggleSide(force) {
    const app = $('#app');
    const open = force ?? !app.classList.contains('side-open');
    app.classList.toggle('side-open', open); requestAnimationFrame(SS.update);
    if (TAURI) FLB.scale(currentZoom(), open);
    $('#nub').title = open ? 'Hide Info Panel' : 'Show Info Panel';
    placeChat();
  }

  /* ---------------- chat ---------------- */
  const chat = $('#chat');
  function now() { const d = new Date(); return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }); }
  function placeChat() {
    if (chat.hidden || chat.dataset.moved) return;
    const app = $('#app'); const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--z')) || 1;
    const w = app.classList.contains('side-open') ? 468 : 286;
    chat.style.left = (app.offsetLeft + w) + 'px'; chat.style.top = (app.offsetTop + 30) + 'px';
  }
  function openChat(id) {
    if (document.body.classList.contains('app-mode')) { notice('Chat', 'Fraglink has no built-in chat. Once friends arrive (a later update), the chat button opens your friend on Discord.'); return; }
    const f = FRIENDS.find(x => x.id === id) || FRIENDS[0];
    state.chatWith = f.id; state.sel = f.id;
    chat.hidden = false; $('#task-chat').hidden = false; $('#task-chat').textContent = f.name + ' - Instant Message';
    $('#chat-title').textContent = f.name + ' - Instant Message';
    const playing = f.kind === 'playing' || f.kind === 'fof';
    $('#chat-info').innerHTML = `<div class="av" style="background:linear-gradient(135deg,${f.av},#0a1428)">${esc(f.name[0].toUpperCase())}</div>
      <div class="who"><b>${esc(f.name)}</b>${playing ? `<div class="playing">is playing ${friendGameIcon(f.game)}<span>${esc(GAMES[f.game].name)}</span></div><div class="srv">${esc(f.server)} · ${esc(f.info)}</div>` : `<div class="playing" style="color:${f.kind === 'off' ? '#bdbfbf' : '#fff'}">${esc(statusText(f))}</div><div class="srv">${f.last ? 'Last online ' + esc(f.last) : 'Not in game'}</div>`}</div>
      ${playing ? '<button class="hw" type="button" data-action="join">Join</button>' : ''}`;
    if (!state.logs[f.id]) {
      state.logs[f.id] = playing
        ? [{ sys: `${f.name} is playing ${GAMES[f.game].name} on ${f.server}` }, { who: f.name, t: 'hey, you around?' }]
        : [{ sys: f.kind === 'off' ? `${f.name} is offline. Messages will be delivered when they sign in.` : `${f.name} is ${statusText(f).toLowerCase()}.` }];
    }
    $('#chat-foot').textContent = playing ? `Voice: off  ·  ${GAMES[f.game].genre}  ·  in-game overlay: Scroll Lock` : 'Fraglink Instant Message';
    drawLog(); placeChat(); render(); requestAnimationFrame(() => { CS.vp.scrollTop = CS.vp.scrollHeight; CS.update(); });
    setTimeout(() => $('#chat-text').focus({ preventScroll: true }), 0);
  }
  function drawLog() {
    const log = state.logs[state.chatWith] || [];
    CS.vp.innerHTML = log.map(m => m.sys ? `<p class="sys">${esc(m.sys)}</p>` :
      `<p><span class="t">[${m.at || now()}]</span> <span class="${m.who === ME.name ? 'me' : 'them'}">${esc(m.who)}:</span> ${esc(m.t)}</p>`).join('');
    CS.vp.scrollTop = CS.vp.scrollHeight; CS.update();
  }
  $('#chat-form').addEventListener('submit', e => {
    e.preventDefault();
    const t = $('#chat-text').value.trim(); if (!t || !state.chatWith) return;
    const f = FRIENDS.find(x => x.id === state.chatWith);
    state.logs[f.id].push({ who: ME.name, t, at: now() }); $('#chat-text').value = ''; drawLog();
    if (f.kind === 'off') return;
    $('#chat-typing').textContent = f.name + ' is typing…';
    setTimeout(() => {
      $('#chat-typing').textContent = '';
      state.logs[f.id].push({ who: f.name, t: REPLIES[Math.floor(Math.random() * REPLIES.length)], at: now() });
      if (state.chatWith === f.id) drawLog();
    }, 1400 + Math.random() * 1200);
  });
  $('#chat-text').addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); $('#chat-form').requestSubmit(); } });

  /* ---------------- toast ---------------- */
  let toastTimer = null, toastFriend = null;
  function toast(f, game) {
    if (LIVE.settings && LIVE.settings.settings && LIVE.settings.settings.toasts === '0') return;
    const t = $('#toast'); toastFriend = f.id;
    $('#toast-body').innerHTML = `<i class="gi">${friendGameIcon(game)}</i><div><b>${esc(f.name)}</b> has started playing<br><span class="g">${esc(GAMES[game].name)}</span></div>`;
    t.classList.remove('out'); t.hidden = false;
    clearTimeout(toastTimer);
    if (!params.get('toast')) toastTimer = setTimeout(hideToast, 7000);
  }
  function hideToast() { const t = $('#toast'); t.classList.add('out'); setTimeout(() => { t.hidden = true; t.classList.remove('out'); }, 300); }
  function randomStart() {
    const cands = FRIENDS.filter(f => f.kind === 'online' || f.kind === 'off');
    const f = cands[Math.floor(Math.random() * cands.length)]; if (!f) return;
    const ids = Object.keys(GAMES); const g = ids[Math.floor(Math.random() * ids.length)];
    Object.assign(f, { kind: 'playing', game: g, server: SERVERS[Math.floor(Math.random() * SERVERS.length)], info: 'just joined' });
    render(); toast(f, g);
  }
  $('#toast').addEventListener('click', e => { if (e.target.closest('[data-action=toast-close]')) { hideToast(); return; } if (toastFriend) openChat(toastFriend); hideToast(); });

  /* ---------------- menus ---------------- */
  let menuOpen = false;
  function closeMenus() { $$('.xf-menu.open').forEach(m => m.classList.remove('open')); menuOpen = false; }
  $$('.xf-menu').forEach(m => {
    m.querySelector(':scope > span').addEventListener('mousedown', e => {
      e.stopPropagation(); const was = m.classList.contains('open'); closeMenus(); if (!was) { m.classList.add('open'); menuOpen = true; }
    });
    m.addEventListener('mouseenter', () => { if (menuOpen && !m.classList.contains('open')) { closeMenus(); m.classList.add('open'); menuOpen = true; } });
  });
  document.addEventListener('mousedown', e => { if (!e.target.closest('.xf-menu')) closeMenus(); });

  /* ---------------- website ---------------- */
  function webHeader(active) {
    const tabs = [['Home', 'home'], ['Games', 'home'], ['Friends', 'home'], ['My Stats', 'profile'], ['My Profile', 'profile'], ['Download', 'home']];
    return `<div class="wp-top"><div class="wp-logo"><img class="wp-wordmark" src="brand/fraglink-wordmark.svg" alt="Fraglink"><small>Play. Connect. Track.</small></div>
      <div class="wp-search"><input placeholder="Find gamers and games" /><button>Search</button></div>
      <div class="wp-login">Logged in as <b>Lucrona</b> · <a>Settings</a> · <a>Log out</a></div></div>
      <div class="wp-nav">${tabs.map(([t, pg]) => `<a class="${t === active ? 'on' : ''}" data-web="${pg}">${t}</a>`).join('')}</div>`;
  }
  function homePage() {
    const top = ['wow', 'cs', 'rust', 'hd2', 'wc3', 'bf2', 'd2', 'q3'];
    return `<div class="wp">${webHeader('Home')}<div class="wp-cols"><div class="wp-main">
      <div class="wp-box"><h3>Your gaming stats and your friends, from anywhere</h3><div class="wp-hero"><svg viewBox="0 0 32 22"><use href="#fl-mark" fill="url(#flNavy)"/><use href="#fl-mark" fill="url(#scan)"/></svg><div><p><b>A tiny tray app logs every hour you play, on every launcher. Your profile, history, recaps and friends live here on the web.</b></p><p>Import Steam hours and estimate your history in two minutes, so your profile is never empty. Export everything, any time.</p><button class="wp-btn" data-action="download">Download Fraglink 1.62</button> <button class="wp-btn alt" data-action="estimate">Estimate your history</button></div></div></div>
      <div class="wp-box"><h3>Top Games This Week</h3><table class="wp-t"><tr><th>#</th><th>Game</th><th>Hours Played</th><th>Gamers</th></tr>${top.map((g, i) => `<tr><td>${i + 1}</td><td><span class="gi">${gameIcon(g)}</span> <a>${esc(GAMES[g].name)}</a></td><td>${(98000 - i * 9133).toLocaleString()}</td><td>${(41000 - i * 3890).toLocaleString()}</td></tr>`).join('')}</table></div>
      </div><div class="wp-side"><div class="wp-box"><h3>Friends Playing Now</h3>${FRIENDS.filter(f => f.kind === 'playing').map(f => `<div class="wp-fr"><span class="gi">${gameIcon(f.game)}</span><a>${esc(f.name)}</a><br><small>${esc(GAMES[f.game].name)}</small></div>`).join('')}</div>
      <div class="wp-box"><h3>Fraglink Stats</h3><p>Registered users: <b>9,812</b><br>Hours tracked this week: <b>120,497</b></p></div></div></div>${webFoot()}</div>`;
  }
  function webFoot() { return `<div class="wp-foot">Fraglink is a mock prototype · All data and characters fictional · No Blizzard, Valve or Xfire assets used · <a>Privacy</a> · <a>Terms</a> · <a>Our data promise</a></div>`; }

  /* original placeholder portrait (hooded ranger silhouette + leaf) — not Blizzard art */
  function portrait(c1 = '#2f6b3a', c2 = '#0c1f14', mono = false) {
    return `<svg viewBox="0 0 64 64" class="pt"><defs><radialGradient id="ptg${c1.slice(1)}" cx=".5" cy=".3" r=".8"><stop offset="0" stop-color="${c1}"/><stop offset="1" stop-color="${c2}"/></radialGradient></defs>
      <rect width="64" height="64" fill="url(#ptg${c1.slice(1)})"/>
      <path d="M32 9c-10 0-17 9-17 21 0 5 1 9 3 12l-6 22h40l-6-22c2-3 3-7 3-12 0-12-7-21-17-21z" fill="rgba(0,0,0,.55)"/>
      <path d="M32 15c-7 0-11 7-11 15l4 6h14l4-6c0-8-4-15-11-15z" fill="rgba(0,0,0,.6)"/>
      <circle cx="27.5" cy="29" r="1.6" fill="${mono ? '#ccc' : '#bff5ff'}"/><circle cx="36.5" cy="29" r="1.6" fill="${mono ? '#ccc' : '#bff5ff'}"/>
      <path d="M46 44c5-6 12-6 14-5-2 5-7 9-14 5z" fill="rgba(255,255,255,.35)"/><path d="M46 44l12-4" stroke="rgba(255,255,255,.5)" stroke-width=".8"/></svg>`;
  }
  /* miniprofile signature (440x110) — the PNG the server would render at /sig/lucrona.png */
  function signatureSVG(theme) {
    const gold = theme === 'gold';
    const A = gold ? '#ffc233' : '#3fd0ff', B1 = gold ? '#3a2606' : '#0d2a52', B2 = gold ? '#120c04' : '#050b16', L = gold ? '#7a5210' : '#1d4f8a';
    const wk = [...MY_HOURS].sort((a, b) => b.week - a.week)[0], at = [...MY_HOURS].filter(r => r.tier !== 'estimated').sort((a, b) => b.total - a.total)[0];
    return `<svg class="sig" viewBox="0 0 440 110" width="440" height="110" font-family="Verdana, 'DejaVu Sans', sans-serif">
      <defs><linearGradient id="sg-${theme}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${B1}"/><stop offset="1" stop-color="${B2}"/></linearGradient>
      <pattern id="sgp-${theme}" width="4" height="4" patternUnits="userSpaceOnUse"><rect width="4" height="1" fill="rgba(255,255,255,.035)"/></pattern></defs>
      <rect x=".5" y=".5" width="439" height="109" rx="3" fill="url(#sg-${theme})" stroke="${L}"/><rect x="1" y="1" width="438" height="108" fill="url(#sgp-${theme})"/>
      <rect x="10" y="12" width="64" height="64" fill="#0c2236" stroke="${A}" stroke-width="1.5"/><text x="42" y="56" text-anchor="middle" font-size="34" font-weight="bold" fill="#fff">L</text>
      <circle cx="17" cy="84" r="3.5" fill="#3ddc5a"/><text x="25" y="87.5" font-size="9" fill="#9fe8a8">Online</text>
      <text x="86" y="26" font-size="15" font-weight="bold" fill="#fff">Lucrona</text><text x="160" y="26" font-size="9" fill="${A}">fraglink.gg/u/lucrona</text>
      <text x="86" y="44" font-size="8" fill="#8ea3c8" letter-spacing=".6">THIS WEEK</text>
      <text x="86" y="57" font-size="10.5" fill="#fff" font-weight="bold">${esc(GAMES[wk.game].name.replace(/:.*/, ''))}</text><text x="86" y="70" font-size="10" fill="${A}">${wk.week.toFixed(1)} hrs</text>
      <text x="262" y="44" font-size="8" fill="#8ea3c8" letter-spacing=".6">ALL TIME</text>
      <text x="262" y="57" font-size="10.5" fill="#fff" font-weight="bold">${esc(GAMES[at.game].name)}</text><text x="262" y="70" font-size="10" fill="${A}">${nf(at.total)} hrs <tspan fill="#7fb2ff">◆ imported</tspan></text>
      <line x1="86" y1="80" x2="430" y2="80" stroke="${L}"/>
      <text x="86" y="95" font-size="9" fill="#c9d4e3">${nf(VERIFIED)} hrs verified · ~${nf(ESTIMATED)} est. · Now playing <tspan fill="#fff" font-weight="bold">${esc(GAMES[ME.game].name.replace(/:.*/, ''))}</tspan></text>
      <g transform="translate(398 8) scale(1)"><svg width="32" height="22" viewBox="0 0 32 22"><use href="#fl-mark" fill="${A}"/></svg></g></svg>`;
  }
  const ACH = [
    { k: 'Clocked In', tier: 'Platinum', c: 'plat', prog: '12,616 / 20,000 h to Diamond', g: 'h' },
    { k: 'One More Round', tier: 'Diamond', c: 'dia', prog: 'WoW · 11,214 h', g: '1' },
    { k: 'No Days Off', tier: 'Gold', c: 'gold', prog: '61 / 90 days', g: '∞' },
    { k: 'Regular Circuit', tier: 'Platinum', c: 'plat', prog: '402 / 1,000 days', g: 'D' },
    { k: 'Game Hopper', tier: 'Silver', c: 'silv', prog: '14 / 25 games', g: 'G' },
    { k: 'Old Guard', tier: 'Bronze', c: 'bron', prog: '1 year on Fraglink', g: 'O' },
  ];
  function histRow(r, max) {
    const t = TIERS[r.tier];
    const total = r.tier === 'estimated' ? `<span class="est" title="Estimated with the history calculator; excluded from ranks and achievements">~${nf(r.total)} hrs</span>` : `${r.total >= 1000 ? nf(r.total) : r.total.toFixed(1)} hrs`;
    return `<tr><td><span class="gi">${gameIcon(r.game)}</span> <a>${esc(GAMES[r.game].name)}</a>${r.note ? `<div class="pf-note">“${esc(r.note)}”</div>` : ''}</td><td>${r.week.toFixed(1)} hrs</td><td class="bar"><i style="width:${max ? (r.week / max * 100) : 0}%"></i></td><td>${total}</td><td><span class="tb tb-${r.tier}" title="${esc(r.src)}">${t.sym} ${t.label}</span></td></tr>`;
  }
  function profilePage(theme) {
    const tw = sumBy(MY_HOURS, r => r.week);
    const max = Math.max(...MY_HOURS.map(r => r.week));
    const LINKED = [['Steam', 'lucrona_au', 'st'], ['Discord', 'lucrona', 'dc'], ['Twitch', 'lucrona_plays', 'tw'], ['Battle.net', 'Lucrona#1187', 'bn']];
    return `<div class="wp prof-${theme}">${webHeader('My Profile')}
      <div class="pf-head"><div class="pf-av">L</div><div class="pf-id"><h2>Lucrona</h2><div class="pf-st"><span class="gi">${gameIcon(ME.game)}</span> Currently playing <b>${esc(GAMES[ME.game].name)}</b></div>
      <div class="pf-links"><a>Add as Friend</a> | <a>Message on Discord</a> | <a>Compare hours</a> | <a data-profile-theme>Theme: ${theme === 'gold' ? 'Gold 567' : 'Cyan Grid'}</a> | <a>Edit layout (drag · hide · reorder)</a></div></div>
      <div class="pf-rank"><b>${nf(VERIFIED)}</b><small>hours verified</small><span class="pf-est">~${nf(ESTIMATED)} estimated</span></div></div>
      <div class="wp-cols"><div class="wp-side pf-left">
        <div class="wp-box" data-sec="details"><h3>Details</h3><table class="pf-kv">
          <tr><td>Username</td><td>lucrona</td></tr><tr><td>Location</td><td>Sydney, AU</td></tr><tr><td>Member Since</td><td>Mar 2025</td></tr><tr><td>Gaming Style</td><td>Weekend Raider</td></tr><tr><td>Last Seen</td><td style="color:var(--pf-accent)">Online now</td></tr></table>
          <div class="pf-sub">Linked Accounts</div>
          <ul class="pf-linked">${LINKED.map(([p, h, c]) => `<li><i class="lk lk-${c}">${p[0]}</i><span><small>${p}</small>${esc(h)}</span><em class="ver">✓ Verified</em></li>`).join('')}
            <li><i class="lk lk-un">R</i><span><small>Riot ID (typed)</small>Lucrona#OCE</span><em class="unver">Unverified</em></li></ul></div>
        <div class="wp-box" data-sec="friends"><h3>Friends (${FRIENDS.filter(f => f.kind !== 'fof').length})</h3><div class="pf-friends">${FRIENDS.filter(f => f.kind !== 'fof').slice(0, 9).map(f => `<a><i style="background:linear-gradient(135deg,${f.av},#0a1428)">${esc(f.name[0].toUpperCase())}</i>${esc(f.name)}</a>`).join('')}</div></div>
        <div class="wp-box" data-sec="rig"><h3>Gaming Rig <small class="h3n">auto-detected · edited</small></h3><table class="pf-kv pf-rig">
          <tr><td>CPU</td><td>Ryzen 7 7800X3D</td></tr><tr><td>GPU</td><td>GeForce RTX 4070 Super</td></tr><tr><td>Memory</td><td>32 GB DDR5</td></tr><tr><td>OS</td><td>Windows 11 23H2</td></tr><tr><td>Monitor</td><td>2560×1440 @ 165 Hz</td></tr><tr><td>Mouse</td><td>Logitech G Pro (typed)</td></tr></table></div>
        <div class="wp-box" data-sec="data"><h3>Your Data</h3><div class="pf-data"><button class="wp-btn" data-action="export">Export everything (JSON / CSV)</button>
          <p>Sessions, hours by tier, characters, friends and activity. Free, any time.</p><p class="pledge"><b>Our pledge:</b> if Fraglink ever shuts down you get at least 90 days' notice and a working export.</p></div></div>
      </div><div class="wp-main">
        <div class="wp-box" data-sec="miniprofile"><h3>Miniprofile <small class="h3n">signature image for forums, READMEs &amp; Twitch panels</small></h3><div class="pf-sig">${signatureSVG(theme)}
          <div class="pf-embed"><label>Image URL<input readonly value="https://fraglink.gg/sig/lucrona.png"></label><label>BBCode<input readonly value="[url=https://fraglink.gg/u/lucrona][img]https://fraglink.gg/sig/lucrona.png[/img][/url]"></label><span>Also: Markdown · HTML · 320×100 Twitch panel · 350×20 userbar</span></div></div></div>
        <div class="wp-box" data-sec="achievements"><h3>Achievements <small class="h3n">24 of 35 tiers · play-time only · estimated hours never count</small></h3><div class="pf-ach">${ACH.map(a => `<div class="ach ach-${a.c}" title="${esc(a.k)} · ${a.tier}"><i><b>${esc(a.g)}</b></i><div><b>${esc(a.k)}</b><span class="t">${a.tier}</span><small>${esc(a.prog)}</small></div></div>`).join('')}</div></div>
        <div class="wp-box" data-sec="history"><h3>Gaming History</h3><table class="wp-t pf-hist"><tr><th>Game</th><th>Past 7 Days</th><th></th><th>Total</th><th>Source</th></tr>
          ${MY_HOURS.map(r => histRow(r, max)).join('')}
          <tr class="tot"><td>Overall Hours</td><td>${tw.toFixed(1)} hrs</td><td></td><td>${nf(VERIFIED)} hrs</td><td>verified</td></tr>
          <tr class="tot est-tot"><td colspan="3"></td><td><span class="est">~${nf(ESTIMATED)} hrs</span></td><td>estimated</td></tr></table>
          <div class="pf-legend"><span class="tb tb-tracked">● Tracked</span> logged by Fraglink or Steam's official playtime · <span class="tb tb-imported">◆ Imported</span> the game's own counter (WoW /played via addon) · <span class="tb tb-estimated">~ Estimated</span> history calculator, excluded from ranks · <a data-action="estimate">Estimate your history…</a> · <a>Show entire gaming history</a></div></div>
        <div class="wp-box" data-sec="wow"><h3>World of Warcraft Characters <small class="h3n">◆ /played: 11,214 h across 6 characters</small></h3><div class="pf-wow">
          <div class="wow-main">${portrait()}<div class="wow-id"><div class="wow-name">Fragnar <span class="ver big">✓ Verified via Battle.net</span></div>
            <div class="wow-line">Level 80 Night Elf <b class="cls">Hunter</b> · Beast Mastery</div><div class="wow-guild">&lt;Southern Cross Vanguard&gt; · Frostmourne (US · Oceanic)</div>
            <div class="wow-stats"><div><small>Item level</small><b>639</b></div><div><small>M+ score</small><b>2,415</b><em>Raider.IO</em></div><div><small>Raid</small><b>8/8 H</b><em>6/8 M</em></div><div><small>Achievements</small><b>18,450</b></div><div><small>/played</small><b>4,812 h</b><em>◆ imported</em></div></div>
            <div class="wow-foot">Last updated 2 h ago · Blizzard updates character data after you log out · Fictional mock character, placeholder art</div></div></div>
          <ul class="wow-alts">
            <li>${portrait('#6a5a2a', '#1a1408', true)}<div><b>Fraglet</b> · Lvl 60 Human Warrior · Classic Era · Arugal</div><span class="ver">✓ Verified</span></li>
            <li>${portrait('#3a3f5a', '#0c0f1c', true)}<div><b>Oldfrag</b> · Lvl 42 Dwarf Priest · Turtle WoW (private server)</div><span class="unver">Unverified · manual</span></li></ul></div></div>
        <div class="wp-box" data-sec="activity"><h3>Recent Activity</h3><ul class="pf-act"><li>Played <b>Half-Life 2: Deathmatch</b> for 2.1 hours · today</li><li>Imported <b>/played</b> for 6 WoW characters · Tue</li><li>Unlocked <b>No Days Off · Gold</b> · Mon</li><li>Weekly recap: <b>22.3 h</b>, top game HL2:DM · <a>view recap</a> · Mon</li><li>Added <a>Riftwalker</a> as a friend · Sep 20</li></ul></div>
      </div></div>${webFoot()}${params.get('modal') === 'estimate' || state.estOpen ? estimatorModal() : ''}</div>`;
  }
  /* ---------------- "Estimate your history" calculator ---------------- */
  const EST = { game: 'cs', eras: [{ from: 2003, to: 2006, hpw: 18 }, { from: 2007, to: 2009, hpw: 4 }], note: 'LAN every weekend at the uni club' };
  const RELEASE = { cs: 2000, wow: 2004, d2: 2000, wc3: 2002, q3: 1999, ut: 2004, bfv: 2004 };
  const eraHours = e => Math.max(0, (e.to - e.from + 1)) * 52 * Math.min(80, Math.max(0, e.hpw));
  function estimatorModal() {
    const tot = sumBy(EST.eras, eraHours);
    const opts = ['cs', 'wow', 'd2', 'wc3', 'q3', 'ut', 'bfv'].map(g => `<option value="${g}" ${g === EST.game ? 'selected' : ''}>${esc(GAMES[g].name)}</option>`).join('');
    return `<div class="est-ovl" id="est"><div class="est-box"><h3>Estimate your history <button data-est="close" title="Close">×</button></h3><div class="est-body">
      <p class="est-intro">Played before Fraglink? Tell us roughly how much. Estimates show on your profile as <span class="est">~hours</span> with an <span class="tb tb-estimated">~ Estimated</span> badge and never count toward ranks, leaderboards or achievements.</p>
      <label class="est-game">Game <span class="gi">${gameIcon(EST.game)}</span><select data-est="game">${opts}</select></label>
      <table class="est-t"><tr><th>From</th><th>To</th><th>Hours / week</th><th>Weeks</th><th>Estimate</th><th></th></tr>
        ${EST.eras.map((e, i) => `<tr><td><input type="number" min="1990" max="2026" value="${e.from}" data-era="${i}" data-f="from"></td><td><input type="number" min="1990" max="2026" value="${e.to}" data-era="${i}" data-f="to"></td><td><input type="number" min="0" max="80" value="${e.hpw}" data-era="${i}" data-f="hpw"></td><td>${(e.to - e.from + 1) * 52}</td><td class="n">${nf(eraHours(e))} h</td><td>${EST.eras.length > 1 ? `<a data-est="del" data-i="${i}">remove</a>` : ''}</td></tr>`).join('')}
      </table><a class="est-add" data-est="add">+ Add another era</a>
      <label class="est-note">Story note (optional, 140 chars)<input maxlength="140" value="${esc(EST.note)}" data-est="note"></label>
      <div class="est-res"><div><small>Shown on your profile as</small><b>~${nf(Math.round(tot / 100) * 100)} h</b><span class="tb tb-estimated">~ Estimated</span></div>
        <div class="est-chk">${EST.eras.every(e => e.from >= RELEASE[EST.game]) ? `<div>✓ Starts after release (${RELEASE[EST.game]})</div>` : `<div class="bad">✗ Starts before release (${RELEASE[EST.game]})</div>`}${EST.eras.every(e => e.hpw <= 80) ? '<div>✓ Under 80 h/week per era</div>' : '<div class="bad">✗ Capped at 80 h/week</div>'}<div>Deleted characters or lost accounts? Estimate those years here: their time can't be recovered from the game.</div></div></div>
      </div><div class="est-foot"><button class="wp-btn" data-est="save">Save to profile</button><button class="wp-btn alt" data-est="close">Cancel</button></div></div></div>`;
  }
  function website(page) {
    const w = $('#web'); w.hidden = false;
    const prof = params.get('prof') === 'gold' ? 'gold' : 'cyan';
    state.webPage = page;
    $('#web-url').textContent = page === 'profile' ? 'http://www.fraglink.gg/u/lucrona' : 'http://www.fraglink.gg/';
    const pg = $('#web-page'), st = pg.scrollTop;
    pg.innerHTML = page === 'profile' ? profilePage(prof) : homePage() + (state.estOpen ? estimatorModal() : '');
    pg.scrollTop = st;
  }
  function openEstimator() { state.estOpen = true; website(state.webPage || 'profile'); }
  $('#web-page').addEventListener('click', e => {
    const a = e.target.closest('[data-web]'); if (a) { state.estOpen = false; website(a.dataset.web); return; }
    if (e.target.closest('[data-profile-theme]')) { params.set('prof', params.get('prof') === 'gold' ? 'cyan' : 'gold'); website('profile'); return; }
    const x = e.target.closest('[data-est]'); if (!x) return;
    const k = x.dataset.est;
    if (k === 'close') { state.estOpen = false; params.delete('modal'); website(state.webPage); }
    else if (k === 'add') { const l = EST.eras[EST.eras.length - 1]; EST.eras.push({ from: Math.min(2026, l.to + 1), to: Math.min(2026, l.to + 2), hpw: 5 }); website(state.webPage); }
    else if (k === 'del') { EST.eras.splice(+x.dataset.i, 1); website(state.webPage); }
    else if (k === 'save') { state.estOpen = false; params.delete('modal'); website(state.webPage); alert('Saved (mock): shown on your profile as an Estimated total.'); }
  });
  $('#web-page').addEventListener('change', e => {
    const t = e.target;
    if (t.dataset.era != null) { EST.eras[+t.dataset.era][t.dataset.f] = +t.value || 0; website(state.webPage); }
    else if (t.dataset.est === 'game') { EST.game = t.value; website(state.webPage); }
    else if (t.dataset.est === 'note') { EST.note = t.value; }
  });

  /* ---------------- dialogs / misc ---------------- */
  // "0.2.0-8" (the MSI-safe build version) reads as "0.2.0-dev.8"
  const devVer = v => String(v || '').replace(/^(\d+\.\d+\.\d+)-(\d+)$/, '$1-dev.$2');
  function notice(title, html) {
    $('#notice-title').textContent = title; $('#notice-body').innerHTML = html; $('#notice').hidden = false; placeDlg($('#notice'));
  }
  // In-window dialogs open centred near the top and can then be dragged by their title bar.
  function placeDlg(d) {
    if (!document.body.classList.contains('win') || d.dataset.moved) return;
    const bz = parseFloat(getComputedStyle(document.body).zoom) || 1;
    d.style.transform = 'none';
    const op = d.offsetParent, pw = op ? op.clientWidth : innerWidth / bz, px = op ? op.getBoundingClientRect().left / bz : 0;
    d.style.left = Math.max(4, Math.round((innerWidth / bz - d.offsetWidth) / 2 - px)) + 'px'; void pw;
    d.style.top = '56px';
  }
  const LATER = 'This lives on the Fraglink website, which arrives in a later update. Your hours are already being recorded on this PC.';
  function minimize() { if (TAURI) { FLB.hide(); return; } $('#app').hidden = true; $('#task-app').classList.remove('active'); }
  function restore() { $('#app').hidden = false; $('#task-app').classList.add('active'); }
  const toastErr = e => notice('Something went wrong', esc(String(e && e.message || e)));

  /* ---- data refresh ---- */
  let refreshTimer = null;
  async function refresh() {
    try {
      const [g, a] = await Promise.all([FLB.glance(), FLB.activity(50)]);
      LIVE.glance = g; LIVE.activity = a; LIVE.fetchedAt = Date.now();
      if (state.tab !== 'friends') render(); else renderSide();
      syncStatus();
    } catch (e) { console.error('refresh failed', e); }
  }
  function refreshSoon() { clearTimeout(refreshTimer); refreshTimer = setTimeout(refresh, 250); }
  async function refreshFriends() {
    if (!FLB.friends) return;
    try {
      if (FLB.syncStatus) LIVE.sync = await FLB.syncStatus().catch(() => null);
      const [f, sv] = await Promise.all([FLB.friends(), FLB.serverStats()]);
      LIVE.friends = f; LIVE.server = sv;
      if (f && f.signed_in && f.me && !jsonList('login_history').includes(f.me)) { saveSetting('login_history', JSON.stringify([f.me, ...jsonList('login_history')].slice(0, 5))); saveSetting('login_last', f.me); }
      if (state.sel && !String(state.sel).startsWith('g:') && !friendsNow().some(x => x.id === state.sel)) state.sel = null;
      render(); syncMenus(); syncStatus();
      if (!LIVE.friendAct || Date.now() - (LIVE.friendActAt || 0) > 30000) { LIVE.friendActAt = Date.now(); refreshActivity(); }
    } catch (e) { console.error('friends refresh failed', e); }
  }

  /* ---- options: its own window (options.html) ---- */
  function openOptions(page) {
    if (TAURI) { FLB.openOptions(page || null).catch(toastErr); return; }
    window.open('options.html' + (page ? '?page=' + page : ''), 'fl-options', 'width=640,height=480');
  }
  // Options (another window) changed a setting: follow it here.
  FLB.on && FLB.on('fraglink://settings', ([k, v]) => {
    if (LIVE.settings) (LIVE.settings.settings = LIVE.settings.settings || {})[k] = v;
    if (k === 'theme') applyTheme(v);
    if (k === 'text_size') applyTextSize(v);
    if (['show_offline', 'show_fof', 'always_on_top', 'share_game', 'custom_status', 'hidden_groups', 'custom_groups', 'custom_on', 'status_history'].includes(k)) { syncMenus(); syncStatus(); render(); }
    if (k.startsWith('game_path:') || k.startsWith('launch_args:')) loadCatalog();
    if (k === 'skin' && v !== state.skin) applySkin(v, false);
    else if (k === 'zoom' && state.skin !== 'dark') setZoom(v);
  });

  /* ---- games dialog: "this is a game" + exclusions ---- */
  let candSel = null;
  function prettyExe(exe) { return exe.replace(/\.exe$/i, '').replace(/[-_]+(win64|shipping|x64|dx11|dx12)\b/ig, '').replace(/[_-]+/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2').trim(); }
  async function openGames(focusTag) {
    $('#games-dlg').hidden = false; placeDlg($('#games-dlg')); $('#games-title').textContent = focusTag ? 'This is a game…' : 'Manage games';
    $('#cand-list').innerHTML = '<div class="g-row">Loading…</div>'; $('#known-list').innerHTML = '';
    try {
      const [cands, games] = await Promise.all([FLB.candidates(), FLB.games()]);
      candSel = null; $('#tag-name').value = '';
      $('#cand-list').innerHTML = cands.length ? cands.map(c => `<div class="g-row cand" data-exe="${esc(c.exe)}" title="${esc(c.path)}"><b>${esc(c.exe)}</b><small>${esc(c.path)}</small></div>`).join('')
        : '<div class="g-row">Nothing unknown is running. Start the game first, then press Refresh.</div>';
      $('#known-list').innerHTML = games.length ? games.map(g => `<div class="g-row known ${g.excluded ? 'ex' : ''}"><span><b>${esc(g.name)}</b><small>${fmtHrs(g.total_seconds)} h · ${esc(g.source)}${g.excluded ? ' · excluded' : ''}</small></span><button class="hw" type="button" data-exclude="${esc(g.id)}" data-ex="${g.excluded ? 0 : 1}">${g.excluded ? 'Include' : 'Exclude'}</button></div>`).join('')
        : '<div class="g-row">No games yet.</div>';
    } catch (e) { toastErr(e); }
  }
  $('#games-dlg').addEventListener('click', async e => {
    const c = e.target.closest('.cand'); if (c) { $$('.cand').forEach(x => x.classList.toggle('sel', x === c)); candSel = c.dataset.exe; $('#tag-name').value = prettyExe(candSel); $('#tag-name').focus(); return; }
    const x = e.target.closest('[data-exclude]'); if (x) { try { await FLB.exclude(x.dataset.exclude, x.dataset.ex === '1'); await openGames(false); refreshSoon(); } catch (err) { toastErr(err); } }
  });

  const actions = {
    'minimize': minimize, 'restore': restore,
    'quit': () => FLB.quit(),
    'toggle-side': () => toggleSide(),
    'options': el => openOptions(el && el.dataset && el.dataset.page),
    'win-min': () => { if (TAURI) FLB.minimizeWindow(); else minimize(); },
    'notice-close': () => { $('#notice').hidden = true; },
    'games-close': () => { $('#games-dlg').hidden = true; },
    'games-refresh': () => openGames($('#games-title').textContent !== 'Manage games'),
    'tag-game': () => openGames(true),
    'manage-games': () => openGames(false),
    'tag-submit': async () => {
      const name = $('#tag-name').value.trim();
      if (!candSel || !name) { notice('This is a game', 'Pick a running program from the list and type the game name.'); return; }
      try { await FLB.tag(candSel, name); $('#games-dlg').hidden = true; notice('Tagged', `<b>${esc(name)}</b> (${esc(candSel)}) will be tracked from now on.`); refreshSoon(); } catch (e) { toastErr(e); }
    },
    'export': async () => { try { const p = await FLB.exportData(); notice('Export my data', `Saved:<br><code>${p.map(esc).join('</code><br><code>')}</code>`); } catch (e) { toastErr(e); } },
    'open-data': () => FLB.openData().catch(toastErr),
    'privacy': () => notice('What Fraglink reads', '<ul class="fl-ul"><li>The <b>list of running programs</b> (names and install paths) every 5 seconds, matched against a list of known games.</li><li>Steam\'s own <b>RunningAppID</b> value and its library files, the <b>Epic Games</b> install manifests and <b>GOG Galaxy</b>\'s install list, to recognise games you installed through them.</li></ul>It never reads game memory, never injects anything into games and never hooks input, so it is safe with anti-cheat. Nothing leaves this PC unless you choose to <b>sign in to sync</b> in Options. Then your game sessions (game, start, end, time played) are uploaded, and while the app runs your friends see your status and the game you\'re playing now. Friends also see your recent sessions in their Activity Report and your hours this week, unless you turn that off in Options &gt; Connections. <b>Invisible</b> shows you as offline. Program names and paths are never sent.'),
    'add-friend': () => {
      if (!realMode() && TAURI) { state.tab = 'friends'; render(); notice('Add Friend', 'Log in first. Friends need a Fraglink account.'); return; }
      state.tab = 'friends'; state.addFriend = { step: 'search', q: '' }; render();
      setTimeout(() => $('#add-handle') && $('#add-handle').focus(), 0);
    },
    'add-cancel': () => { state.addFriend = null; render(); },
    'add-friend-go': async () => {
      const h = (($('#add-handle') || {}).value || '').trim(); if (!h) return;
      state.addFriend = { step: 'results', q: h, users: null }; render();
      try { const r = await FLB.friendSearch(h); state.addFriend.users = (r && r.users) || []; }
      catch (e) { state.addFriend.users = []; state.addFriend.err = String(e && e.message || e); }
      state.addFriend.sel = state.addFriend.users[0] ? state.addFriend.users[0].handle : null;
      if (state.addFriend.step === 'results') render();
    },
    'add-pick': a => { state.addFriend.sel = a.dataset.handle; render(); },
    'add-back': () => { state.addFriend = { step: 'search', q: state.addFriend.q }; render(); },
    'add-friend-send': async a => {
      const h = a.dataset.handle, m = $('#add-msg');
      try { const r = await FLB.friendAdd(h); m.innerHTML = r === 'accepted' ? `You and <b>${esc(h)}</b> are now friends.` : `Request sent. <b>${esc(h)}</b> shows under Friend Requests until they accept.`; refreshFriends(); }
      catch (e) { m.innerHTML = `<span class="bad">${esc(String(e && e.message || e))}</span>`; }
    },
    'update-now': a => { a.disabled = true; a.classList.add('busy'); a.title = 'Restarting…'; FLB.installUpdate().catch(e => { a.disabled = false; a.classList.remove('busy'); a.title = 'Update ready - click to restart'; toastErr(e); }); },
    // Help > Check for Updates: "You're up to date", or the update downloads, installs and restarts.
    'check-updates': async () => {
      notice('Check for Updates', '<p class="upd-msg" id="upd-msg">Checking for updates…</p>');
      let r; try { r = await FLB.checkUpdates(); } catch (e) { r = { result: 'failed', message: String(e) }; }
      const m = $('#upd-msg'); if (!m) return;
      if (r.result === 'up_to_date') m.innerHTML = `You're up to date.<br><small>Fraglink ${esc(devVer(r.version))}</small>`;
      else if (r.result === 'installing') m.innerHTML = `Installing Fraglink ${esc(devVer(r.version))}…<br><small>Fraglink restarts when it's done.</small>`;
      else m.innerHTML = `Couldn't check for updates.<br><small>${esc(r.message || '')}</small>`;
    },
    'detect-skip': () => { state.detectRun = false; if (TAURI) saveSetting('games_detected_once', '1'); render(); },
    'activity-more': () => { if (tabOn('activity')) { state.tab = 'activity'; render(); } else notice('Activity Report', 'The full Activity Report (all of your friends\' recent games, with filters) arrives with the Fraglink website. The side panel shows the latest few.'); },
    'status-menu': a => statusMenu(a),
    'games': () => openOptions('games'),
    'web': a => { if (TAURI && FLB.openWebsite) FLB.openWebsite(a.dataset.page).catch(toastErr); else notice('Fraglink website', LATER); },
    'login-go': async () => {
      const u = ($('#lg-user').value || '').trim(), pw = $('#lg-pass').value || '';
      if (!u || !pw) { state.loginErr = 'Enter your username and password.'; render(); return; }
      saveSetting('save_password', $('#lg-save').checked ? '1' : '0'); saveSetting('auto_login', $('#lg-auto').checked ? '1' : '0');
      state.loginErr = ''; state.loginBusy = 'Logging in…'; render();
      try { await FLB.syncLogin(srvVal(), u, pw); state.loginMode = null; } catch (e) { state.loginErr = String(e && e.message || e); }
      state.loginBusy = ''; refreshFriends();
    },
    'signup-go': async () => {
      const u = ($('#lg-nuser').value || '').trim(), pw = $('#lg-npass').value || '', pw2 = $('#lg-npass2').value || '', em = ($('#lg-email').value || '').trim();
      if (!/^[A-Za-z0-9_-]{3,20}$/.test(u)) { state.loginErr = 'Usernames are 3-20 characters: letters, digits, _ or -.'; render(); return; }
      if (pw.length < 10) { state.loginErr = 'Use a password of at least 10 characters.'; render(); return; }
      if (pw !== pw2) { state.loginErr = "The passwords don't match."; render(); return; }
      state.loginErr = ''; state.loginBusy = 'Creating your account…'; render();
      try { await FLB.syncSignup(srvVal(), u, pw, em); state.loginMode = null; } catch (e) { state.loginErr = String(e && e.message || e); }
      state.loginBusy = ''; refreshFriends();
    },
    'login-back': () => { state.loginMode = null; state.loginErr = ''; render(); },
    'login-browser': () => { state.loginErr = ''; FLB.syncSignIn(srvVal()).then(refreshFriends).catch(toastErr); },
    'login-cancel': () => FLB.syncCancel().then(refreshFriends).catch(toastErr),
    'register': () => { state.tab = 'friends'; state.loginMode = 'create'; state.loginErr = ''; render(); setTimeout(() => $('#lg-nuser') && $('#lg-nuser').focus(), 0); },
    'forgot': () => { if (TAURI && FLB.openWebsite) FLB.openWebsite('forgot').catch(toastErr); else notice('Forgot Password', LATER); },
    'invite': () => notice('Invite via Email', 'Inviting friends by email arrives with the Fraglink website. For now, send them the download link and your username.'),
    'friend-accept': a => FLB.friendRespond(a.dataset.handle, true).then(refreshFriends).catch(toastErr),
    'friend-decline': a => FLB.friendRespond(a.dataset.handle, false).then(() => { state.sel = null; refreshFriends(); }).catch(toastErr),
    'friend-remove': a => { if (a.dataset.confirm && !confirm(`Remove ${a.dataset.handle} from your friends?`)) return; FLB.friendRemove(a.dataset.handle).then(() => { state.sel = null; refreshFriends(); }).catch(toastErr); },
    'friend-block': a => { if (!confirm(`Block ${a.dataset.handle}? They won't be able to send you requests or see your status. They aren't told.`)) return; FLB.friendBlock(a.dataset.handle, true).then(() => { state.sel = null; refreshFriends(); }).catch(toastErr); },
    'friend-mute': a => FLB.friendMute(a.dataset.handle, a.dataset.mute === '1').then(refreshFriends).catch(toastErr),
    'my-profile': () => notice('My Profile', LATER),
    'friend-profile': a => { const h = a && a.dataset && a.dataset.handle || a && a.dataset && a.dataset.key; if (TAURI && FLB.openWebsite && h) FLB.openWebsite('u/' + h).catch(toastErr); else notice('Profile', LATER); },
    'my-stats': () => notice('Full stats', LATER), 'website': () => notice('fraglink.gg', LATER), 'estimate': () => notice('Estimate my history', LATER),
    'about': async () => { let v = ''; try { const st = await FLB.settings(); v = `${devVer(st.version)}${st.sim ? ' (simulation mode)' : ''}`; } catch (e) {}
      notice('About Fraglink', `<div class="about-box"><img class="about-lock" src="brand/v28-lockup.svg" alt="Fraglink" draggable="false"><b class="about-ver">Fraglink ${esc(v)}</b><div class="about-slogan">PLAY. CONNECT. TRACK.</div>
        <p>Track your game hours and see what friends are playing.</p><p class="about-c">\u00a9 2026 Lucrona. All rights reserved.</p>
        <p class="about-credits">Look inspired by the classic gaming IM clients; original Fraglink art. Logo: <i>Split Orbit</i>, wordmark set in Saira Expanded Bold (SIL OFL 1.1). Fonts: Russo One (SIL OFL 1.1), Wine Tahoma clone (LGPL).</p></div>`); },
    'chat': () => { const f = friendsNow().find(x => x.id === state.sel); if (f && f.real) { notice('Chat', 'Fraglink has no built-in chat. Linking Discord so this opens a DM arrives in a later update.'); return; } openChat(state.sel); },
    'join': () => notice('Join Game', 'Joining friends arrives with friends in a later update.'),
    'clear-sel': () => { state.sel = null; render(); },
    'download': () => notice('Download', 'You are already running Fraglink.'),
    'always-on-top': () => { const v = !setOn('always_on_top', false); saveSetting('always_on_top', v ? '1' : '0'); syncMenus(); },
    'sign-out': async () => { if (!realMode()) { openOptions('account'); return; } if (!confirm('Sign out of your Fraglink account on this PC? Tracking keeps running; friends disappear until you sign in again.')) return; try { await FLB.syncSignOut(); } catch (e) { toastErr(e); } refreshFriends(); },
    'toggle-offline': () => { saveSetting('show_offline', setOn('show_offline') ? '0' : '1'); syncMenus(); render(); },
    'toggle-fof': () => { saveSetting('show_fof', setOn('show_fof') ? '0' : '1'); syncMenus(); render(); },
    'show-groups': () => { saveSetting('hidden_groups', '[]'); render(); },
    'help-contents': () => notice('Fraglink Help', '<ul class="fl-ul"><li><b>Friends list</b>: click a group heading to fold or unfold it (Fraglink remembers), hover a friend for details, right-click a heading to add a custom group or hide the category (Tools &gt; Friends &gt; Show Hidden Categories brings it back).</li><li><b>Info panel</b>: the arrow on the right edge. It shows your games with <b>Launch</b> links, or details for the friend or group you selected.</li><li><b>Status</b>: the box under the banner. While you play, it shows the game automatically.</li><li><b>Games</b>: Options &gt; Games lists what Fraglink found; set paths and launch options there, or add a game by hand.</li><li><b>Privacy</b>: see Options &gt; Privacy for what friends can see.</li></ul>'),
    'my-stats': () => { const u = realMode() && LIVE.friends.me; if (TAURI && FLB.openWebsite) FLB.openWebsite(u ? 'u/' + u : '').catch(toastErr); else notice('My Stats', LATER); },
    'custom-status-go': () => { useCustom($('#custom-status').value); $('#notice').hidden = true; },
    'add-group-go': () => { const v = ($('#group-name').value || '').trim().slice(0, 30); if (v) saveSetting('custom_groups', JSON.stringify([...new Set([...jsonList('custom_groups'), v])]));
      if (v && state.moveAfterAdd) { const o = groupOf(); o[state.moveAfterAdd] = v; saveSetting('friend_groups', JSON.stringify(o)); } state.moveAfterAdd = null; $('#notice').hidden = true; render(); },
    'launch': async a => { try { await FLB.launchGame(a.dataset.game); } catch (e) { notice('Launch', esc(String(e && e.message || e))); } },
    'detect-games': async a => { if (a && a.classList.contains('side-detect')) a.textContent = 'Detecting…'; try { setCatalog(await FLB.detectGames()); } catch (e) { toastErr(e); } },
  };
  /* games on this PC (Options > Games, Tools > Launch, the info panel) */
  function setCatalog(c) {
    CATALOG = c || [];
    for (const g of CATALOG) if (g.genre) { GENRE[g.id] = g.genre; if (g.steam_appid) GENRE['steam:' + g.steam_appid] = g.genre; }
    const inst = CATALOG.filter(g => g.installed && g.can_launch && !g.excluded);
    $('#menu-launch').innerHTML = (inst.map(g => `<button data-action="launch" data-game="${esc(g.id)}">${esc(g.name)}</button>`).join('') || '<button disabled>No games found</button>') + '<hr /><button data-action="detect-games">Detect Games</button>';
    render();
  }
  const loadCatalog = () => FLB.catalog ? FLB.catalog().then(setCatalog).catch(e => console.warn('catalog', e)) : null;
  const THEMES = { blue: ['#4b82d6', '#1f4f9e', '#173d7c'], crimson: ['#d65b5b', '#9e1f2a', '#7c1720'], emerald: ['#4bb87a', '#1f7e4a', '#175f38'], graphite: ['#8a9099', '#4f555e', '#3a3f46'] };
  function applyTheme(t) {
    if (!THEMES[t]) t = 'blue';
    const [a, b, c] = THEMES[t], r = document.documentElement.style;
    r.setProperty('--dk-blue1', a); r.setProperty('--dk-blue2', b); r.setProperty('--dk-blue3', c);
    $$('.theme-opt').forEach(x => x.classList.toggle('checked', x.dataset.theme === t));
  }
  function syncMenus() {
    $('#menu-ontop').classList.toggle('checked', setOn('always_on_top', false));
    $('#menu-offline').classList.toggle('checked', setOn('show_offline'));
    $('#menu-fof').classList.toggle('checked', setOn('show_fof'));
    $('#menu-signout').disabled = !realMode();
  }
  document.addEventListener('click', e => {
    const skin = e.target.closest('.skin-opt'); if (skin) { applySkin(skin.dataset.skin); closeMenus(); return; }
    const zo = e.target.closest('.zoom-opt'); if (zo) { setZoom(zo.dataset.zoom); closeMenus(); return; }
    const th = e.target.closest('.theme-opt'); if (th) { applyTheme(th.dataset.theme); saveSetting('theme', th.dataset.theme); closeMenus(); return; }
    const a = e.target.closest('[data-action]');
    if (a && !a.closest('#toast')) { closeMenus(); const fn = actions[a.dataset.action]; if (fn) { e.preventDefault(); fn(a); } return; }
    const t = e.target.closest('[data-tab]'); if (t) { state.tab = tabOn(t.dataset.tab) ? t.dataset.tab : 'friends'; render(); return; }
  });
  $$('input[name=skin]').forEach(r => r.addEventListener('change', () => applySkin(r.value)));
  list.addEventListener('click', e => {
    const s = e.target.closest('[data-sec]');
    if (s) {
      const k = s.dataset.sec;
      // the arrow folds the group; clicking the heading only selects it (grey bar, like Xfire)
      if (e.target.closest('.tri')) { state.collapsed[k] = !state.collapsed[k]; saveSetting('collapsed_groups', JSON.stringify(Object.keys(state.collapsed).filter(x => state.collapsed[x]).slice(0, 30))); }
      else state.sel = 'g:' + k;
      render(); return;
    }
    const so = e.target.closest('[data-sort]'); if (so) { const k = so.dataset.sort; state.sort = { key: k, dir: state.sort.key === k ? -state.sort.dir : 1 }; render(); return; }
    const hs = e.target.closest('[data-hsort]'); if (hs) { const k = hs.dataset.hsort; state.hoursSort = { key: k, dir: state.hoursSort.key === k ? -state.hoursSort.dir : -1 }; render(); return; }
    const w = e.target.closest('[data-web]'); if (w) { website(w.dataset.web); return; }
    const r = e.target.closest('[data-friend]'); if (r) { state.sel = r.dataset.friend; render(); }
  });
  document.addEventListener('keydown', e => { if (e.key === 'Enter' && e.target.id === 'add-handle') actions['add-friend-go'](); if (e.key === 'Enter' && e.target.id === 'custom-status') actions['custom-status-go'](); if (e.key === 'Escape') hideCtx(); });
  /* Rich friend tooltip (Windows-style), shown after a short hover; the info panel stays click-only. */
  const tip = $('#ftip'); let tipTimer = null, tipRow = null;
  const mutualOf = f => (f.mutual || ((f.info || '').match(/via ([^·]+)/) || [])[1] || '').split(',').map(x => x.trim()).filter(Boolean);
  function tipHtml(f) {
    const L = (k, v) => v ? `<div><b>${k}</b> ${esc(v)}</div>` : '';
    const user = f.handle || f.name, nick = f.handle && f.name !== f.handle ? f.name : '';
    let h = L('Username:', user) + L('Nickname:', nick);
    if (f.kind === 'playing' || f.kind === 'fof') {
      h += L('Playing:', GAMES[f.game] && GAMES[f.game].name);
      if (f.real) h += L('Session:', f.server);
      else if (/^[\d.]+:\d+$/.test(f.server || '')) h += L('Server:', f.info) + L('IP:', f.server);
      else h += L('Server:', f.server);
      if (f.ping) h += L('Ping:', f.ping + ' ms');
      if (f.kind === 'fof') h += L('Common Friends:', mutualOf(f).join(', ') || 'none known');
    } else if (f.kind === 'off') h += f.last ? `<div>Last online ${esc(f.last)}</div>` : L('Status:', 'Offline');
    else h += L('Status:', statusText(f));
    return h;
  }
  function hideTip() { clearTimeout(tipTimer); tipRow = null; tip.hidden = true; }
  list.addEventListener('mouseover', e => {
    const r = e.target.closest('.xf-row[data-friend]');
    if (r === tipRow) return;
    hideTip(); if (!r) return;
    tipRow = r; const mx = e.clientX;
    tipTimer = setTimeout(() => {
      const f = friendsNow().find(x => x.id === r.dataset.friend); if (!f || !r.isConnected) return;
      tip.innerHTML = tipHtml(f); tip.hidden = false;
      const rr = r.getBoundingClientRect(), tw = tip.offsetWidth, th = tip.offsetHeight;
      const x = Math.max(2, Math.min(mx + 4, innerWidth - tw - 2));
      const y = rr.bottom + 2 + th <= innerHeight ? rr.bottom + 2 : Math.max(2, rr.top - th - 2); // never over the row
      tip.style.left = x + 'px'; tip.style.top = y + 'px';
    }, 450);
  });
  list.addEventListener('mouseleave', hideTip);
  list.addEventListener('mousedown', hideTip);
  list.addEventListener('wheel', hideTip, { passive: true });
  /* Menus: real native Windows popup menus in the app (like the tray menu); the browser
     preview draws the same items itself. items: {id, label, disabled, checked, sep, items}. */
  const ctx = $('#ctx');
  let menuPick = null;
  function hideCtx() { ctx.hidden = true; }
  function menuHtml(items) {
    return items.map(it => it.sep ? '<hr>' : it.items && it.items.length
      ? `<div class="ctx-sub"><button class="${it.disabled ? 'dis' : ''}">${esc(it.label)}<b>▸</b></button><div class="ctx-subm">${menuHtml(it.items)}</div></div>`
      : `<button data-pick="${esc(it.id)}" class="${it.checked ? 'chk' : ''}"${it.disabled ? ' disabled' : ''}>${esc(it.label)}</button>`).join('');
  }
  function showMenu(items, x, y, onPick) {
    hideTip(); menuPick = onPick;
    if (TAURI && FLB.popup) { FLB.popup(items, x, y).catch(e => console.warn('menu', e)); return; }
    ctx.innerHTML = menuHtml(items); ctx.hidden = false;
    const bz = parseFloat(getComputedStyle(document.body).zoom) || 1, W = innerWidth / bz, H = innerHeight / bz;
    ctx.style.left = Math.max(0, Math.min(x / bz, W - ctx.offsetWidth - 2)) + 'px';
    ctx.style.top = Math.max(0, Math.min(y / bz, H - ctx.offsetHeight - 2)) + 'px';
  }
  FLB.on && FLB.on('fraglink://menu-pick', id => { const f = menuPick; menuPick = null; if (f) f(String(id)); });
  ctx.addEventListener('click', e => {
    const b = e.target.closest('[data-pick]'); if (!b || b.disabled) return;
    hideCtx(); const f = menuPick; menuPick = null; if (f) f(b.dataset.pick);
  });
  list.addEventListener('contextmenu', e => {
    const r = e.target.closest('.xf-row[data-friend]');
    if (r) {
      e.preventDefault(); hideTip();
      const f = friendsNow().find(x => x.id === r.dataset.friend); if (!f) return;
      state.sel = f.id; render();
      const key = f.handle || f.id, fav = jsonList('favorite_friends').includes(key), join = joinUrl(f);
      const groups = jsonList('custom_groups'), cur = groupOf()[key];
      const friendRow = f.kind !== 'req' && f.kind !== 'out' && f.kind !== 'fof';
      const items = [{ id: 'profile', label: `View ${f.name}'s Profile` }];
      if (join) items.push({ id: 'join', label: 'Join Game' });
      if (friendRow) {
        items.push({ id: 'fav', label: fav ? 'Remove from Favorites' : 'Add to Favorites' });
        const sub = groups.map((g, i) => ({ id: 'move:' + i, label: g, checked: cur === g }));
        if (cur) sub.push({ id: 'move:-', label: '(No group)' });
        if (sub.length) sub.push({ sep: true });
        sub.push({ id: 'add-group', label: 'Add Custom Group...' });
        items.push({ label: 'Move to Group', items: sub }, { sep: true }, { id: 'remove', label: 'Remove Friend...' }, { id: 'block', label: 'Block User...' });
      }
      showMenu(items, e.clientX, e.clientY, id => {
        if (id === 'profile') actions['friend-profile']({ dataset: { handle: key } });
        else if (id === 'join') { if (TAURI && FLB.joinGame) FLB.joinGame(join.replace('steam://connect/', '')).catch(toastErr); else notice('Join Game', esc(join)); }
        else if (id === 'fav') { const l = jsonList('favorite_friends'); saveSetting('favorite_friends', JSON.stringify(l.includes(key) ? l.filter(x => x !== key) : [...l, key].slice(-100))); render(); }
        else if (id.startsWith('move:')) { const o = groupOf(), g = groups[+id.slice(5)]; if (id === 'move:-' || !g) delete o[key]; else o[key] = g; saveSetting('friend_groups', JSON.stringify(o)); render(); }
        else if (id === 'add-group') addGroup(key);
        else if (id === 'remove' || id === 'block') removeFriend(key, id === 'block');
      });
      return;
    }
    const s = e.target.closest('[data-sec]'); if (!s) return;
    e.preventDefault(); const k = s.dataset.sec;
    state.sel = 'g:' + k; render();
    const items = [{ id: 'add-group', label: 'Add Custom Group...' }, { id: 'hide', label: 'Hide Category' }];
    if (k.startsWith('c:')) items.push({ id: 'del-group', label: 'Delete Group' });
    showMenu(items, e.clientX, e.clientY, id => {
      if (id === 'add-group') addGroup(null);
      else if (id === 'hide') { saveSetting('hidden_groups', JSON.stringify([...new Set([...jsonList('hidden_groups'), k])])); if (state.sel === 'g:' + k) state.sel = null; render(); }
      else if (id === 'del-group') { saveSetting('custom_groups', JSON.stringify(jsonList('custom_groups').filter(g => 'c:' + g !== k))); render(); }
    });
  });
  function addGroup(moveKey) {
    state.moveAfterAdd = moveKey || null;
    notice('Add Custom Group', `Group name:<br><input id="group-name" maxlength="30" autocomplete="off" /><div class="opt-btns"><button class="hw" type="button" data-action="add-group-go">Add</button></div>`);
    setTimeout(() => $('#group-name') && $('#group-name').focus(), 0);
  }
  // Join Game only when we know a joinable address (e.g. a Source server IP → steam://connect)
  function joinUrl(f) { return f.kind === 'playing' && /^[\d.]+:\d+$/.test(f.server || '') ? 'steam://connect/' + f.server : f.join || ''; }
  async function removeFriend(k, block) {
    const f = friendsNow().find(x => (x.handle || x.id) === k), n = f ? f.name : k;
    if (!confirm(block ? `Block ${n}? They won't be able to send you requests or see your status. They aren't told.` : `Remove ${n} from your friends list?`)) return;
    if (f && !f.real) { FRIENDS = FRIENDS.filter(x => x !== f); state.sel = null; render(); return; }
    try { await (block ? FLB.friendBlock(k, true) : FLB.friendRemove(k)); state.sel = null; refreshFriends(); } catch (e) { toastErr(e); }
  }
  document.addEventListener('mousedown', e => { if (!e.target.closest('#ctx')) hideCtx(); });
  list.addEventListener('dblclick', e => { const r = e.target.closest('[data-friend]'); if (r) openChat(r.dataset.friend); });
  // prototype: single click also opened chat; the app only selects (friends are sample data until M3)
  $('#list').addEventListener('keydown', e => {
    const rows = $$('[data-friend]', list); if (!rows.length) return;
    let i = rows.findIndex(r => r.dataset.friend === state.sel);
    if (e.key === 'ArrowDown') i = Math.min(rows.length - 1, i + 1); else if (e.key === 'ArrowUp') i = Math.max(0, i - 1);
    else if (e.key === 'Enter' && state.sel) { openChat(state.sel); return; } else return;
    e.preventDefault(); state.sel = rows[i].dataset.friend; render();
  });
  /* Status: the game I'm in shows automatically (it is "Online, playing X"),
     then Online / Away / Invisible, a custom line, and Set Custom Status. */
  let myStatus = 'online', autoAway = false;
  const LOOKING = 'Looking to Play';
  // automatic Away only replaces Online; a status picked by hand stays
  const shownStatus = () => myStatus === 'online' && autoAway ? 'away' : myStatus;
  function statusLabel() { const s = shownStatus(); return s === 'online' ? '' : ' (' + ({ away: 'Away', invisible: 'Invisible' })[s] + ')'; }
  function syncStatus() {
    const btn = $('#status-btn'), np = LIVE.glance && LIVE.glance.now_playing, custom = SET().custom_status || '';
    const customOn = SET().custom_on === '1' && custom;
    btn.disabled = signedOut();
    if (btn.disabled) { btn.querySelector('span').textContent = 'Offline'; $('#username').textContent = '(Offline)'; if ($('#px-user')) $('#px-user').textContent = '(Offline)'; return; }
    const auto = myStatus === 'online' && autoAway, game = np && setOn('share_game') ? np.name : '';
    const txt = auto ? (game ? game + ' (Away)' : 'Away') : myStatus === 'away' ? 'Away' : myStatus === 'invisible' ? 'Invisible' : customOn ? custom : game ? 'Playing ' + game : 'Online';
    btn.querySelector('span').textContent = txt; btn.title = 'Status: ' + txt + (auto ? ' (no keyboard or mouse input for 10 minutes)' : '');
    btn.dataset.st = shownStatus();
    $('#username').textContent = (realMode() && LIVE.friends.me ? LIVE.friends.me : ME.name) + statusLabel();
    if ($('#px-user') && $('#px-user').textContent === '(Offline)') $('#px-user').textContent = realMode() && LIVE.friends.me ? LIVE.friends.me : ME.name;
  }
  function useCustom(v) {
    v = String(v || '').trim().slice(0, 60);
    saveSetting('custom_status', v); saveSetting('custom_on', v ? '1' : '0');
    if (v && v !== LOOKING) saveSetting('status_history', JSON.stringify([v, ...jsonList('status_history').filter(x => x !== v)].slice(0, 5)));
    if (myStatus !== 'online') { myStatus = 'online'; FLB.set('status', 'online').catch(() => {}); }
    syncStatus();
  }
  function setMyStatus(st) {
    saveSetting('custom_on', '0');
    if (st !== myStatus) { myStatus = st; FLB.set('status', st).catch(() => {}); }
    syncStatus();
  }
  // Status menu (native): Online, Away, Invisible, Looking to Play, Music Status, ─, Set Custom Status..., Reset Status
  function statusMenu(btn) {
    if (!btn || btn.disabled) return;
    const custom = SET().custom_status || '', customOn = SET().custom_on === '1' && custom;
    const hist = jsonList('status_history').filter(x => x !== LOOKING).slice(0, 5);
    const on = myStatus === 'online';
    const items = [
      { id: 'online', label: 'Online', checked: on && !customOn },
      { id: 'away', label: 'Away', checked: myStatus === 'away' },
      { id: 'invisible', label: 'Invisible', checked: myStatus === 'invisible' },
      { id: 'looking', label: LOOKING, checked: on && customOn && custom === LOOKING },
      { id: 'music', label: 'Music Status (coming soon)', disabled: true },
      { sep: true },
      { id: 'custom', label: 'Set Custom Status...' },
    ];
    if (hist.length) items.push({ label: 'Recent Statuses', items: hist.map((x, i) => ({ id: 'h:' + i, label: x, checked: on && customOn && custom === x })) });
    items.push({ id: 'reset', label: 'Reset Status' });
    const r = btn.getBoundingClientRect(); // viewport px (already includes the text-size zoom)
    showMenu(items, r.left, r.bottom, id => {
      if (id === 'custom') {
        notice('Set Custom Status', `Shown under your name in this window. <em>(Friends see it in a later update.)</em><br><input id="custom-status" maxlength="60" value="${esc(SET().custom_status || '')}" autocomplete="off" /><div class="opt-btns"><button class="hw" type="button" data-action="custom-status-go">OK</button></div>`);
        setTimeout(() => $('#custom-status') && $('#custom-status').focus(), 0);
      } else if (id === 'looking') useCustom(LOOKING);
      else if (id.startsWith('h:')) useCustom(hist[+id.slice(2)]);
      else if (id === 'online' || id === 'reset') setMyStatus('online'); // back to automatic (Playing <game> or Online)
      else if (id === 'away' || id === 'invisible') setMyStatus(id);
    });
  }

  /* Text size (Options > Appearance): Normal / Large / Larger scales the whole dark window. */
  const TEXT_ZOOM = { normal: 1, large: 1.15, larger: 1.3 };
  function applyTextSize(v) {
    const z = TEXT_ZOOM[v] || 1;
    document.body.style.zoom = z === 1 ? '' : String(z);
    document.body.classList.toggle('ts-big', z !== 1);
    requestAnimationFrame(() => { LS.update(); SS.update(); });
  }

  /* Info panel grip: drag the panel's outer edge to widen it (min 180 px). */
  document.addEventListener('mousedown', e => {
    const g = e.target.closest('#side-grip'); if (!g || e.button !== 0) return;
    e.preventDefault(); e.stopPropagation();
    const sx = e.screenX, w0 = state.sideW || SIDE_MIN, z = parseFloat(getComputedStyle(document.body).zoom) || 1;
    let raf = 0, last = w0;
    document.body.classList.add('side-dragging');
    const move = ev => { last = w0 + (ev.screenX - sx) / z; setSideW(last, false); if (TAURI && !raf) raf = requestAnimationFrame(() => { raf = 0; FLB.sideWidth(state.sideW).catch(() => {}); }); if (!TAURI) $('#app').style.width = ''; };
    const up = () => { document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); document.body.classList.remove('side-dragging'); setSideW(last, true); };
    document.addEventListener('mousemove', move); document.addEventListener('mouseup', up);
  }, true);

  /* drag: the app window is moved by the OS (Tauri); dialogs move inside it */
  const dragTargets = { app: '#app', chat: '#chat', web: '#web', 'games-dlg': '#games-dlg', notice: '#notice' };
  // frameless dark window: edges and the bottom-right grip resize the OS window
  document.addEventListener('mousedown', e => {
    const r = e.target.closest('[data-resize]');
    if (r && TAURI && e.button === 0 && state.skin === 'dark') { e.preventDefault(); e.stopPropagation(); FLB.resize(r.dataset.resize); }
  }, true);
  window.addEventListener('resize', () => { LS.update(); SS.update(); });
  document.addEventListener('mousedown', e => {
    const h = e.target.closest('[data-drag]'); if (!h || e.target.closest('button,select,input,textarea,a')) return;
    if (TAURI && h.dataset.drag === 'app') { e.preventDefault(); FLB.drag(); return; }
    const win = $(dragTargets[h.dataset.drag]); if (!win) return;
    const zoomed = win !== $('#web'); const z = zoomed ? (parseFloat(getComputedStyle(win).zoom) || 1) : 1;
    const sx = e.clientX, sy = e.clientY, ox = win.offsetLeft, oy = win.offsetTop;
    const dlg = win.classList.contains('xf-dialog'), bz = parseFloat(getComputedStyle(document.body).zoom) || 1;
    const move = ev => {
      let x = ox + (ev.clientX - sx) / (z * bz), y = oy + (ev.clientY - sy) / (z * bz);
      if (dlg) { // keep the title bar reachable inside the window
        const W = innerWidth / bz, H = innerHeight / bz;
        x = Math.max(40 - win.offsetWidth, Math.min(W - 40, x)); y = Math.max(0, Math.min(H - 24, y)); win.dataset.moved = 1;
      }
      win.style.left = x + 'px'; win.style.top = y + 'px'; if (win.id === 'web') win.style.transform = 'none'; if (win.id === 'chat') chat.dataset.moved = 1; if (win.id === 'app') placeChat();
    };
    const up = () => { document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); };
    document.addEventListener('mousemove', move); document.addEventListener('mouseup', up); e.preventDefault();
  });

  /* zoom (1x / 1.5x / 2x): in the app the OS window is resized to match */
  function currentZoom() { return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--z')) || 1; }
  function setZoom(z) {
    z = String(z); document.documentElement.style.setProperty('--z', z); $('#zoom').value = ['1', '1.5', '2'].includes(z) ? z : '2';
    $$('.zoom-opt').forEach(b => b.classList.toggle('checked', b.dataset.zoom === z));
    if (TAURI) FLB.scale(z, $('#app').classList.contains('side-open')).catch(() => {});
    layout(); LS.update();
  }
  function layout() {
    if (TAURI) return; // the window itself is the chassis
    const z = currentZoom();
    const d = $('#desktop'); const app = $('#app');
    if (!app.dataset.moved) {
      const wide = params.get('chat') ? 290 + 300 : 290;
      app.style.left = Math.max(4, Math.round((d.clientWidth / z - wide) / 2 - 14)) + 'px';
      app.style.top = Math.max(0, Math.round((d.clientHeight / z - 381) / 2)) + 'px';
    }
    placeChat();
  }
  $('#zoom').addEventListener('change', e => setZoom(e.target.value));
  window.addEventListener('resize', layout);
  function clock() {
    $('#clock').textContent = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    const s = $('#session'); if (s) s.textContent = fmtT(liveSeconds() * 1000);
  }
  setInterval(clock, 1000);
  document.addEventListener('input', e => { if (e.target.id === 'add-handle') { state.addFriend.q = e.target.value; $('#add-search').disabled = !e.target.value.trim(); } });
  document.addEventListener('keydown', e => { if (e.key !== 'Enter') return; if (e.target.id === 'add-handle') actions['add-friend-go'](); else if (e.target.id === 'lg-pass') actions['login-go'](); else if (e.target.id === 'lg-npass2' || e.target.id === 'lg-email') actions['signup-go'](); });
  setInterval(() => { const t = $('#lg-tmo'); if (t) t.textContent = `Time out in ${Math.max(0, Math.round(+t.dataset.exp - Date.now() / 1000))} secs`; }, 1000);

  /* ---------------- boot ---------------- */
  (async () => {
    let st = null;
    try { st = await FLB.settings(); LIVE.settings = st; } catch (e) {}
    const saved = st && st.settings || {};
    if (TAURI && saved.skin) state.skin = params.get('skin') || saved.skin;
    if (['online', 'away', 'invisible'].includes(saved.status)) myStatus = saved.status;
    applyTheme(saved.theme); syncMenus(); syncStatus();
    for (const k of jsonList('collapsed_groups')) state.collapsed[k] = true;
    setZoom(params.get('zoom') || (TAURI ? (saved.zoom || '1.5') : (document.body.classList.contains('compare') ? 1 : 2)));
    applySkin(state.skin);
    clock();
    { const sw = TAURI ? +saved.side_w : +(params.get('sidew') || localStorage.getItem('fl-side-w')); if (sw) setSideW(sw, false); }
    applyTextSize(saved.text_size || params.get('text') || 'normal');
    if (params.get('side') === '1') toggleSide(true);
    if (params.get('sel')) { state.sel = params.get('sel'); render(); }
    if (params.get('add') === '1') setTimeout(() => actions['add-friend'](), 300);
    /* First run: detect installed games in the main window (progress + Skip) before the friends list. */
    if ((TAURI && !saved.games_detected_once) || params.get('detect') === '1') {
      state.detectRun = true; state.detectPct = 0; render(); if (!TAURI) refreshActivity();
      const tick = setInterval(() => { state.detectPct = Math.min(92, state.detectPct + 4); const b = $('#fd-bar'); if (b) b.style.width = state.detectPct + '%'; }, 150);
      FLB.detectGames().then(c => { setCatalog(c); if (TAURI) saveSetting('games_detected_once', '1'); }).catch(e => console.warn('detect', e)).finally(() => {
        clearInterval(tick); const b = $('#fd-bar'); if (b) b.style.width = '100%';
        setTimeout(() => { if (state.detectRun && params.get('detect') !== '1') { state.detectRun = false; render(); } }, 400);
      });
    }
    await refreshFriends();
    await refresh();
    loadCatalog();
    FLB.on('fraglink://events', () => { for (const k in ICONS) if (ICONS[k] === null && !k.startsWith('f:')) delete ICONS[k]; refreshSoon(); }); // a newly detected game may now have an exe icon
    FLB.on('fraglink://friends', refreshFriends);
    FLB.on('fraglink://sync', refreshFriends);
    FLB.on('fraglink://logoff', () => actions['sign-out']());
    // Auto-update: Rust downloads + verifies quietly (startup + every 6 h); then a green arrow
    // in the title bar installs and restarts in one click. No prompt.
    const askUpdate = v => { const b = $('#upd-ready'); if (!b || !v) return;
      b.title = 'Update ready - click to restart'; b.dataset.version = devVer(v); b.hidden = false; document.body.classList.add('has-upd'); };
    FLB.on('fraglink://update-found', v => { const m = $('#upd-msg'); if (m) m.innerHTML = `Downloading Fraglink ${esc(v)}…<br><small>Fraglink restarts when it's done.</small>`; });
    FLB.on('fraglink://update-progress', p => { const m = $('#upd-msg small'); if (m && p != null && m.parentNode.textContent.startsWith('Downloading')) m.textContent = `${p}% · Fraglink restarts when it's done.`; });
    // Automatic Away after 10 minutes without input (Rust watches; like Discord).
    FLB.on('fraglink://auto-away', v => { autoAway = !!v; syncStatus(); });
    if (FLB.autoAway) FLB.autoAway().then(v => { autoAway = !!v; syncStatus(); }).catch(() => {});
    FLB.on('fraglink://update', askUpdate);
    if (FLB.updateAvailable) FLB.updateAvailable().then(askUpdate).catch(() => {});
    FLB.on('fraglink://catalog', loadCatalog);
    FLB.on('fraglink://show-friend', h => { state.tab = 'friends'; state.sel = 'u:' + h; refreshFriends(); });
    setInterval(() => { if (realMode() && state.tab === 'friends') render(); refreshActivity(); }, 60000);
    FLB.on('fraglink://status', v => { myStatus = v; syncStatus(); });
    setInterval(refresh, 30000);
    if (!TAURI && params.get('toast') === '1') { const f = FRIENDS.find(x => x.id === 'rift'); Object.assign(f, { kind: 'playing', game: 'bf2', server: SERVERS[1], info: 'Karkand · 32/64' }); render(); toast(f, 'bf2'); }
    if (params.get('upd') === '1') askUpdate('0.2.0-10');
    if (params.get('check') === '1') actions['check-updates']();
    if (params.get('modal') === 'options') openOptions();
    if (params.get('modal') === 'about') actions.about();
    if (params.get('menu') === 'status') setTimeout(() => statusMenu($('#status-btn')), 300);
    if (params.get('modal') === 'games') openGames(true);
  })();
  if (params.get('compare') === '1') document.body.classList.add('compare');
})();
