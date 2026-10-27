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
  const LIVE = { glance: null, activity: [], fetchedAt: 0, settings: null, friends: null, server: null, friendAct: null, summaries: {}, profiles: {} };


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
  /* Real game icons (classic style: the game's own 16x16 icon). The app resolves
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
  // Friends' games use the same icon priority as ours (server, Steam, exe), see icons.rs.
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
    { id: 'curm', name: 'curmudgeon', kind: 'playing', game: 'cs', server: SERVERS[0], info: 'de_dust2 · 18/20', av: '#6d6d6d', week: 11.4, total: 812, recent: [['cs', 790.2], ['rust', 21.8]] },
    { id: 'dizz', name: 'diZZ the Spinner', handle: 'dizz_spinner', kind: 'playing', game: 'wow', server: SERVERS[2], info: 'Lvl 60 Rogue · Stormwind', detail: 'Frostmourne', av: '#b8860b', week: 23.1, total: 503, recent: [['wow', 488.6], ['wc3', 12.4], ['hl2', 2.0]] },
    { id: 'shar', name: 'sharpe_', kind: 'playing', game: 'coh', server: SERVERS[3], info: 'Atlas Park · lvl 32', av: '#1e4fb8', week: 7.8, total: 390 },
    { id: 'tool', name: 'tooTALL', kind: 'playing', game: 'eq2', server: SERVERS[4], info: 'Qeynos Harbor', av: '#c89a20', week: 15.0, total: 720 },
    { id: 'rift', name: 'Riftwalker', kind: 'off', last: '20 minutes ago', status: 'Online', av: '#3a7a3a', week: 4.2, total: 260, recent: [['bf2', 140.5], ['hd2', 62.0], ['q3', 9.5]] },
    { id: 'nyx', name: 'nyx0', kind: 'off', last: '5 hours ago', status: 'Away', av: '#6a3a8a', week: 1.1, total: 88 },
    { id: 'abby', name: 'Abbey', kind: 'fof', game: 'aa', server: SERVERS[5], info: 'via tooTALL · Bridge', av: '#b0172a', week: 0, total: 0 },
    { id: 'jj', name: 'JJ_toasty', kind: 'fof', game: 'bfv', server: SERVERS[6], info: 'via curmudgeon · Hue', av: '#7a1a1a', week: 0, total: 0 },
    { id: 'kr', name: 'Krakatoa', kind: 'off', last: '2 hours ago', av: '#444', week: 6.3, total: 402, recent: [['wow', 380.2], ['d2', 22.0]] },
    { id: 'mo', name: 'mothball', kind: 'off', last: 'yesterday', av: '#444', week: 0.5, total: 97 },
    { id: 'pz', name: 'Pz_Kpfw', kind: 'off', last: '3 days ago', av: '#444', week: 0, total: 1210 },
    { id: 'lu', name: 'lumen', kind: 'off', last: '1 week ago', av: '#444', week: 0, total: 55 },
  ];
  const ME = { name: 'Lucrona', since: 'Mar 2005', game: 'hl2', started: Date.now() - (47 * 60 + 12) * 1000 };
  // tier: tracked (Fraglink/Steam official) | imported (game's own counter, e.g. WoW /played)
  const MY_HOURS = [
    { game: 'hl2', week: 9.6, today: 1.6, total: 311.4, tier: 'tracked', src: 'Steam playtime', last: 'today' },
    { game: 'wow', week: 6.8, today: 1.1, total: 11214, tier: 'imported', src: '/played via Fraglink addon · 6 characters', last: 'today', note: 'Raided every night through TBC and Wrath' },
    { game: 'wc3', week: 3.2, today: 0, total: 544.0, tier: 'tracked', src: 'Fraglink tracked', last: 'yesterday' },
    { game: 'bf2', week: 2.7, today: 0, total: 188.9, tier: 'tracked', src: 'Fraglink tracked', last: 'Thu' },
    { game: 'q3', week: 0.0, today: 0, total: 260.5, tier: 'tracked', src: 'Fraglink tracked', last: 'Jul 12' },
    { game: 'rust', week: 0.0, today: 0, total: 97.2, tier: 'tracked', src: 'Steam playtime', last: 'May 2' },
  ];
  const TIERS = { tracked: { label: 'Tracked', sym: '●' }, imported: { label: 'Imported', sym: '◆' } };
  const sumBy = (arr, f) => arr.reduce((a, b) => a + f(b), 0);
  const VERIFIED = sumBy(MY_HOURS, r => r.total);
  const nf = n => Math.round(n).toLocaleString('en-AU');
  function tierMark(t) { const x = TIERS[t]; return `<i class="tm tm-${t}" data-tip="${x.label}">${x.sym}</i>`; }
  function fmtTotalShort(r) { return `${nf(r.total)}h${tierMark(r.tier)}`; }
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
    skin: 'dark',
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
  // Dark is the only skin (0.2.0-dev.20 removed Classic Silver).
  function applySkin() {
    state.skin = 'dark';
    document.body.classList.add('skin-dark');
    render();
  }

  /* ---------------- list rendering ---------------- */
  const LS = makeScroller($('#list'), true); const list = LS.vp;
  const SS = makeScroller($('#side-content'), false);
  const CS = makeScroller($('#chat-log'), false);
  function fmtH(h) { return h.toFixed(1) + ' h'; }
  function fmtT(ms) { const s = Math.floor(ms / 1000), h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, x = s % 60; return `${h}:${String(m).padStart(2, '0')}:${String(x).padStart(2, '0')}`; }
  // The WoW realm gets its own labelled row ("Realm  Frostmourne") in the side panel and
  // tooltip, like the classic client; gameLabel is just the game's name.
  const gameLabel = f => f.game === UNLISTED ? 'A game' : (GAMES[f.game] ? GAMES[f.game].name : '');
  const SHOW_REALM = false; // realm detection on the back burner (dev.18); true brings the rows back
  const realmOf = f => SHOW_REALM && f.game !== UNLISTED && f.detail ? String(f.detail) : '';
  // A game the server can't name yet (a program someone tagged that isn't on Fraglink's
  // list): friends just see "Playing a game" (the server sends game_id "unlisted").
  const UNLISTED = 'unlisted';
  function statusText(f) {
    if ((f.kind === 'playing' || f.kind === 'fof') && f.game === UNLISTED) return 'Playing a game' + (f.away ? ' (Away)' : '');
    if (f.kind === 'playing' || f.kind === 'fof') return GAMES[f.game].name + (f.away ? ' (Away)' : '');
    if (f.kind === 'online' || f.kind === 'req' || f.kind === 'out') return f.status;
    return 'Offline';
  }
  function row(f, cls = '') {
    // classic: the game icon sits before the name while playing, a blank slot otherwise
    const icon = f.game && f.kind !== 'off' ? `<i class="gi">${friendGameIcon(f.game)}</i>` : '<i class="noicon"></i>';
    const info = f.kind === 'playing' || f.kind === 'fof' ? f.server : f.kind === 'off' ? (f.last ? 'Last online ' + f.last : '') : (f.info || '');
    return `<div class="xf-row ${cls} ${state.sel === f.id ? 'sel' : ''}" data-friend="${f.id}"><span class="c-name">${icon}${f.fav ? '<i class="fav" data-tip="Favourite">★</i>' : ''}<span class="nm" data-tip="${esc(f.name)}">${esc(f.name)}</span></span><span class="c-status" data-tip="${esc(statusText(f))}">${esc(statusText(f))}</span><span class="c-info" data-tip="${esc(info)}">${esc(info)}</span></div>`;
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
    online: 'Friends who are on Fraglink right now.',
    fof: 'People your friends know who are in a game right now. Hover over one to see which friends you have in common.',
    req: "People who want to be your friend, and requests you've sent.",
    off: "Friends who aren't on Fraglink right now.",
    custom: 'A group you made.',
  };
  // The grouped list shared by sample and real friends.
  const groupOf = () => { try { const o = JSON.parse(SET().friend_groups || '{}'); return o && typeof o === 'object' ? o : {}; } catch (e) { return {}; } };
  function groupsHtml(F, note) {
    const hidden = jsonList('hidden_groups');
    const groups = jsonList('custom_groups'), moved = groupOf();
    const inGroup = f => groups.includes(moved[f.handle || f.id]);
    const favs = jsonList('favorite_friends');
    F = F.map(f => favs.includes(f.handle || f.id) ? { ...f, fav: true } : f);
    // Steam-style: a friend in your own group is listed there AND still under Friends Online /
    // Friends Offline, so the online list always shows everyone who's on.
    const custom = Object.fromEntries(groups.map(g => [g, sortedOnlineFirst(F.filter(f => moved[f.handle || f.id] === g))]));
    void inGroup;
    const on = sorted(F.filter(f => f.kind === 'playing')).concat(sorted(F.filter(f => f.kind === 'online')));
    const fof = sorted(F.filter(f => f.kind === 'fof')), reqs = F.filter(f => f.kind === 'req' || f.kind === 'out'), off = sorted(F.filter(f => f.kind === 'off'));
    const block = (key, label, cls, rows, rowCls = '', count) => {
      if (hidden.includes(key)) return '';
      return sec(key, label, cls, count != null ? count : rows.length) + (state.collapsed[key] ? '' : rows.map(f => row(f, (rowCls || (f.kind === 'off' ? 'off' : '')) + (f.kind === 'req' ? ' pending' : ''))).join(''));
    };
    const onlineOf = rows => rows.filter(f => f.kind === 'playing' || f.kind === 'online').length;
    let h = note || '';
    h += block('online', GROUP_NAMES.online, 's-on', on);
    if (setOn('show_fof')) h += block('fof', 'Friends of Friends Playing', 's-fof', fof, 'fof');
    if (reqs.length) h += block('req', GROUP_NAMES.req, 's-fof', reqs, 'req');
    // your groups count who's on out of everyone in it, e.g. (2/5)
    for (const g of groups) h += block('c:' + g, esc(g), 's-cus', custom[g], '', `${onlineOf(custom[g])}/${custom[g].length}`);
    if (setOn('show_offline')) h += block('off', GROUP_NAMES.off, 's-off', off, 'off');
    return h;
  }
  const sortedOnlineFirst = arr => sorted(arr.filter(f => f.kind === 'playing')).concat(sorted(arr.filter(f => f.kind === 'online')), sorted(arr.filter(f => f.kind !== 'playing' && f.kind !== 'online')));
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
      const p = f.presence || {}, name = f.display_name || f.handle, base = { id: 'u:' + f.handle, handle: f.handle, name, avatar: f.avatar, real: true, since: f.since, presence: p, week: 0, total: 0 };
      if (p.status !== 'offline' && p.game_id) out.push({ ...base, kind: 'playing', game: ensureGame(p.game_id, p.game_name), detail: p.game_detail || '', server: sinceText(p.session_started_at), info: '', away: p.status === 'away' });
      else if (p.status === 'online' || p.status === 'away') out.push({ ...base, kind: 'online', status: p.status === 'away' ? 'Away' : 'Online' });
      else out.push({ ...base, kind: 'off', last: '' });
    }
    for (const q of l.incoming || []) out.push({ id: 'u:' + q.handle, handle: q.handle, avatar: q.avatar, name: q.display_name || q.handle, kind: 'req', status: 'Wants to add you', info: '', real: true, incoming: true });
    for (const q of l.outgoing || []) out.push({ id: 'u:' + q.handle, handle: q.handle, avatar: q.avatar, name: q.display_name || q.handle, kind: 'out', status: 'Request sent', info: '', real: true });
    return out;
  }
  const friendsNow = () => realMode() ? realFriends() : FRIENDS;
  /* Signed out: the login form sits in the main window (like the classic client). Fraglink signs
     in through the browser (device pairing), so there's no password box here. */
  const signedOut = () => params.get('login') === '1' || (TAURI && !!LIVE.friends && !LIVE.friends.signed_in);
  // No Server field any more: the app always signs in to https://playfraglink.com (sync::sign_in_base).
  const srvVal = () => 'https://playfraglink.com';
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
    else if (state.loginMode === 'mfa') {
      h += `<b class="lg-lbl">Two-step sign-in</b>
        <p class="lg-dim">Enter the 6-digit code from your authenticator app, or one of your backup codes.</p>
        <label class="lg-lbl" for="lg-code">Code</label>
        <div class="lg-row"><input id="lg-code" maxlength="16" autocomplete="one-time-code" /><button class="hw" type="button" data-action="mfa-go">Verify</button></div>
        <p class="lg-dim">Lost your authenticator and backup codes? Email <a href="#" data-action="mail-support">support@playfraglink.com</a></p>
        <div class="af-btns"><button class="hw" type="button" data-action="login-back">Cancel</button></div>`;
    }
    else if (state.loginMode === 'create') {
      h += `<b class="lg-lbl">Create Account</b>
        <label class="lg-lbl" for="lg-nuser">Username</label><input id="lg-nuser" maxlength="20" autocomplete="off" placeholder="3-20 letters, digits, _ or -" />
        <label class="lg-lbl" for="lg-npass">Password</label><input id="lg-npass" type="password" maxlength="128" placeholder="at least 10 characters" />
        <label class="lg-lbl" for="lg-npass2">Confirm Password</label><input id="lg-npass2" type="password" maxlength="128" />
        <label class="lg-lbl" for="lg-email">Email <span class="lg-opt">(optional)</span></label><input id="lg-email" maxlength="254" autocomplete="off" />
        <span class="lg-lbl">Date of birth</span>
        <div class="lg-dob">
          <select id="lg-dob-d" aria-label="Day"><option value="">Day</option><option value="01">1</option><option value="02">2</option><option value="03">3</option><option value="04">4</option><option value="05">5</option><option value="06">6</option><option value="07">7</option><option value="08">8</option><option value="09">9</option><option value="10">10</option><option value="11">11</option><option value="12">12</option><option value="13">13</option><option value="14">14</option><option value="15">15</option><option value="16">16</option><option value="17">17</option><option value="18">18</option><option value="19">19</option><option value="20">20</option><option value="21">21</option><option value="22">22</option><option value="23">23</option><option value="24">24</option><option value="25">25</option><option value="26">26</option><option value="27">27</option><option value="28">28</option><option value="29">29</option><option value="30">30</option><option value="31">31</option></select>
          <select id="lg-dob-m" aria-label="Month"><option value="">Month</option>
            <option value="01">Jan</option><option value="02">Feb</option><option value="03">Mar</option>
            <option value="04">Apr</option><option value="05">May</option><option value="06">Jun</option>
            <option value="07">Jul</option><option value="08">Aug</option><option value="09">Sep</option>
            <option value="10">Oct</option><option value="11">Nov</option><option value="12">Dec</option>
          </select>
          <select id="lg-dob-y" aria-label="Year"><option value="">Year</option><option value="2026">2026</option><option value="2025">2025</option><option value="2024">2024</option><option value="2023">2023</option><option value="2022">2022</option><option value="2021">2021</option><option value="2020">2020</option><option value="2019">2019</option><option value="2018">2018</option><option value="2017">2017</option><option value="2016">2016</option><option value="2015">2015</option><option value="2014">2014</option><option value="2013">2013</option><option value="2012">2012</option><option value="2011">2011</option><option value="2010">2010</option><option value="2009">2009</option><option value="2008">2008</option><option value="2007">2007</option><option value="2006">2006</option><option value="2005">2005</option><option value="2004">2004</option><option value="2003">2003</option><option value="2002">2002</option><option value="2001">2001</option><option value="2000">2000</option><option value="1999">1999</option><option value="1998">1998</option><option value="1997">1997</option><option value="1996">1996</option><option value="1995">1995</option><option value="1994">1994</option><option value="1993">1993</option><option value="1992">1992</option><option value="1991">1991</option><option value="1990">1990</option><option value="1989">1989</option><option value="1988">1988</option><option value="1987">1987</option><option value="1986">1986</option><option value="1985">1985</option><option value="1984">1984</option><option value="1983">1983</option><option value="1982">1982</option><option value="1981">1981</option><option value="1980">1980</option><option value="1979">1979</option><option value="1978">1978</option><option value="1977">1977</option><option value="1976">1976</option><option value="1975">1975</option><option value="1974">1974</option><option value="1973">1973</option><option value="1972">1972</option><option value="1971">1971</option><option value="1970">1970</option><option value="1969">1969</option><option value="1968">1968</option><option value="1967">1967</option><option value="1966">1966</option><option value="1965">1965</option><option value="1964">1964</option><option value="1963">1963</option><option value="1962">1962</option><option value="1961">1961</option><option value="1960">1960</option><option value="1959">1959</option><option value="1958">1958</option><option value="1957">1957</option><option value="1956">1956</option><option value="1955">1955</option><option value="1954">1954</option><option value="1953">1953</option><option value="1952">1952</option><option value="1951">1951</option><option value="1950">1950</option><option value="1949">1949</option><option value="1948">1948</option><option value="1947">1947</option><option value="1946">1946</option><option value="1945">1945</option><option value="1944">1944</option><option value="1943">1943</option><option value="1942">1942</option><option value="1941">1941</option><option value="1940">1940</option><option value="1939">1939</option><option value="1938">1938</option><option value="1937">1937</option><option value="1936">1936</option><option value="1935">1935</option><option value="1934">1934</option><option value="1933">1933</option><option value="1932">1932</option><option value="1931">1931</option><option value="1930">1930</option><option value="1929">1929</option><option value="1928">1928</option><option value="1927">1927</option><option value="1926">1926</option><option value="1925">1925</option><option value="1924">1924</option><option value="1923">1923</option><option value="1922">1922</option><option value="1921">1921</option><option value="1920">1920</option><option value="1919">1919</option><option value="1918">1918</option><option value="1917">1917</option><option value="1916">1916</option><option value="1915">1915</option><option value="1914">1914</option><option value="1913">1913</option><option value="1912">1912</option><option value="1911">1911</option><option value="1910">1910</option><option value="1909">1909</option><option value="1908">1908</option><option value="1907">1907</option><option value="1906">1906</option><option value="1905">1905</option><option value="1904">1904</option><option value="1903">1903</option><option value="1902">1902</option><option value="1901">1901</option><option value="1900">1900</option></select>
        </div>
        <p class="lg-hint">By creating an account you agree to the <a data-action="web" data-page="terms">Terms of Use</a> and <a data-action="privacy-policy">Privacy Policy</a>.</p>
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
  // Your relation to a search result: 'friend', 'out' (request sent), 'in' (they asked you) or ''.
  function relationOf(handle) {
    const h = String(handle || '').toLowerCase(), l = realMode() ? LIVE.friends.list : null;
    const has = xs => (xs || []).some(x => String(x.handle || '').toLowerCase() === h);
    if (l) return has(l.friends) ? 'friend' : has(l.outgoing) ? 'out' : has(l.incoming) ? 'in' : '';
    const f = FRIENDS.find(x => String(x.handle || x.name || '').toLowerCase() === h);
    return !f ? '' : f.kind === 'out' ? 'out' : f.kind === 'req' ? 'in' : 'friend';
  }
  const REL_TAG = { friend: 'Already friends', out: 'Request sent', in: 'Wants to add you' };
  /* Add Friend: small modal over the dimmed friends list (search, results, personal message in one window). */
  function addFriendHtml() {
    const A = state.addFriend, us = A.users, sel = A.sel || '';
    let rows = '';
    if (us === null) rows = '<tr class="af-note"><td colspan="2">Searching…</td></tr>';
    else if (us) rows = us.length ? us.map(u => { const r = relationOf(u.handle); return `<tr class="${u.handle === sel ? 'sel' : ''}" data-action="add-pick" data-handle="${esc(u.handle)}"><td>${esc(u.handle)}</td><td>${esc(u.display_name || u.handle)}${r ? ` <em class="af-rel">${REL_TAG[r]}</em>` : ''}</td></tr>`; }).join('') : `<tr class="af-note"><td colspan="2">${esc(A.err || A.self || 'No users found. Check the spelling, or try part of their name.')}</td></tr>`;
    const n = us ? us.length : 0, rel = sel ? relationOf(sel) : '', canAdd = sel && rel !== 'friend' && rel !== 'out' && !A.busy;
    return `<div class="af-dlg" role="dialog" aria-label="Add Friend"><div class="af-title"><span>Add Friend</span><button type="button" class="af-x" data-action="add-cancel" data-tip="Close">×</button></div><div class="af-body">
      <label for="add-handle">Search by nickname, username or email:</label>
      <input id="add-handle" maxlength="60" autocomplete="off" spellcheck="false" value="${esc(A.q)}" />
      <div class="af-ex">(Example: Jo, Jo Doe, or jo@doe.com)</div>
      <div class="af-btns"><button class="af-b" type="button" data-action="add-cancel">Cancel</button><button class="af-b" type="button" id="add-search" data-action="add-friend-go"${A.q.trim() ? '' : ' disabled'}>Search</button></div>
      <div class="af-h"><span>Search Results (select one):</span><span>${us ? `${n} User${n === 1 ? '' : 's'} Found` : ''}</span></div>
      <div class="af-tw"><table class="af-t"><tr><th>Username</th><th>Nickname</th></tr>${rows}</table></div>
      <label for="add-msg-text">Personal Message:</label><textarea id="add-msg-text" maxlength="200" rows="3">${esc(A.msg)}</textarea>
      <div id="add-msg" class="${A.bad ? 'bad' : ''}">${esc(A.out || '')}</div>
      <div class="af-btns"><button class="af-b" type="button" data-action="friend-profile" data-handle="${esc(sel)}"${sel ? '' : ' disabled'}>View Profile</button><button class="af-b" type="button" data-action="add-friend-send" data-handle="${esc(sel)}"${canAdd ? '' : ' disabled'}${rel === 'friend' || rel === 'out' ? ` data-tip="${REL_TAG[rel]}"` : ''}>${A.busy ? 'Adding…' : rel === 'in' ? 'Accept' : 'Add Friend'}</button></div>
    </div></div>`;
  }
  function renderAddFriend() {
    let m = $('#af-modal');
    if (!state.addFriend) { if (m) m.remove(); return; }
    if (!m) { m = document.createElement('div'); m.id = 'af-modal'; m.className = 'af-modal'; ($('#app') || document.body).appendChild(m); /* inside the window: dims only Fraglink */ }
    const f = document.activeElement && document.activeElement.id, pos = f === 'add-handle' ? document.activeElement.selectionStart : null;
    const tw = m.querySelector('.af-tw'), top = tw ? tw.scrollTop : 0;
    const app = $('#app'), sd = $('.xf-side'); // centre over the friends list, not the side panel
    m.style.setProperty('--af-side', app && sd && app.classList.contains('side-open') ? sd.offsetWidth + 'px' : '0px');
    m.innerHTML = addFriendHtml();
    if (m.querySelector('.af-tw')) m.querySelector('.af-tw').scrollTop = top;
    if (f === 'add-handle' || f === 'add-msg-text') { const e = $('#' + f); e.focus(); if (pos != null) e.setSelectionRange(pos, pos); }
  }
  /* dev.19: resizable Name / Status columns (Game Info takes the rest), saved locally. */
  const COL_MIN = { name: 60, status: 50 }, INFO_MIN = 40;
  function colWidths() { try { const o = JSON.parse(localStorage.getItem('fl-cols') || 'null'); return o && o.name > 0 && o.status > 0 ? o : null; } catch (e) { return null; } }
  function startColDrag(e, grip) {
    const grid = grip.closest('.xf-grid'), head = grid.querySelector('.xf-head');
    const w = { name: head.querySelector('.c-name').offsetWidth, status: head.querySelector('.c-status').offsetWidth };
    const col = grip.dataset.col, x0 = e.clientX, start = w[col], total = head.clientWidth;
    e.preventDefault(); e.stopPropagation(); document.body.classList.add('col-dragging');
    const move = ev => {
      const other = col === 'name' ? w.status : w.name;
      w[col] = Math.max(COL_MIN[col], Math.min(start + ev.clientX - x0, total - other - INFO_MIN));
      grid.classList.add('cw'); grid.style.setProperty('--cw-name', w.name + 'px'); grid.style.setProperty('--cw-status', w.status + 'px');
    };
    const up = () => { document.removeEventListener('pointermove', move); document.removeEventListener('pointerup', up); document.body.classList.remove('col-dragging'); localStorage.setItem('fl-cols', JSON.stringify(w)); };
    document.addEventListener('pointermove', move); document.addEventListener('pointerup', up);
  }
  document.addEventListener('pointerdown', e => { const g = e.target.closest && e.target.closest('.col-grip'); if (g) startColDrag(e, g); }, true);
  document.addEventListener('click', e => { if (e.target.closest && e.target.closest('.col-grip')) { e.stopPropagation(); e.preventDefault(); } }, true);
  document.addEventListener('dblclick', e => { if (e.target.closest && e.target.closest('.col-grip')) { e.stopPropagation(); localStorage.removeItem('fl-cols'); render(); } }, true);
  if (params.get('cols')) { const [n, s2] = params.get('cols').split(',').map(Number); localStorage.setItem('fl-cols', JSON.stringify({ name: n, status: s2 })); }
  function renderFriends() {
    if (state.detectRun) { list.innerHTML = `<div class="lg"><div class="lg-h">Detecting installed games…</div><p>Looking for games from Steam, Battle.net, Epic, GOG, EA app, Ubisoft Connect and Xbox…</p><div class="det-track"><div class="det-bar" id="fd-bar" style="width:${state.detectPct}%"></div></div><button class="dk-btn" data-action="detect-skip">Skip</button></div>`; return; }
    const s = state.sort;
    // the arrow sits on the active column and points the way it's sorted (click again to flip)
    const arrow = k => s.key === k ? ` sort${s.dir < 0 ? ' desc' : ''}` : '';
    const cw = colWidths(), cs = cw ? ` cw" style="--cw-name:${cw.name}px;--cw-status:${cw.status}px` : '';
    let h = `<div class="xf-grid${cs}"><div class="xf-head"><span class="c-name${arrow('name')}" data-sort="name">Name<i class="col-grip" data-col="name" data-tip="Drag to resize, double-click to reset"></i></span><span class="c-status${arrow('status')}" data-sort="status">Status<i class="col-grip" data-col="status" data-tip="Drag to resize, double-click to reset"></i></span><span class="c-info">Game Info</span></div>`;
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
    if (np) { const id = ensureGame(np.game_id, np.name); h += `<div class="xf-row st-row" data-game="${esc(id)}" data-tip="${esc(np.name)} · detected via ${esc(String(np.source).replace('_', ' '))}"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(np.name)}</span></span><span class="st-live timer" id="session">${fmtT(liveSeconds() * 1000)}</span></div>`; }
    else if (g.paused) h += `<div class="xf-row st-row fl-muted" data-tip="Hours only count while you're signed in and connected to Fraglink: the server records them as you play."><span class="c-name st-g"><i class="noicon"></i><span class="nm">${realMode() ? 'Offline · hours count again once reconnected' : 'Not tracking · sign in to count hours'}</span></span><span class="st-live"></span></div>`;
    else h += `<div class="xf-row st-row fl-muted"><span class="c-name st-g"><i class="noicon"></i><span class="nm">Not in a game · watching</span></span><span class="st-live"></span></div>`;
    const change = g.last_week_seconds > 0 ? Math.round((g.week_seconds - g.last_week_seconds) / g.last_week_seconds * 100) : null;
    const chg = change === null ? '' : `<em class="${change < 0 ? 'down' : ''}" data-tip="vs all of last week">${change < 0 ? '&#9660;' : '&#9650;'}${Math.abs(change)}%</em>`;
    h += `<div class="st-sum"><div><small>Today (this PC)</small><b>${fmtHrs(g.today_seconds)} h</b></div><div><small>This week (this PC)</small><b>${fmtHrs(g.week_seconds)} h</b>${chg}</div><div><small>Streak</small><b>${g.streak_days} day${g.streak_days === 1 ? '' : 's'}</b></div></div>`;
    h += `<div class="xf-sec s-fof"><i class="tri"></i>Top Games This Week (this PC)</div>`;
    const top = g.top || []; const max = Math.max(1, ...top.map(r => r.seven_day_seconds));
    if (!top.length) h += `<div class="fl-empty">No games tracked yet. Start a game, or use <a data-action="tag-game">Fraglink → This is a game…</a></div>`;
    h += top.map(r => { const id = ensureGame(r.game_id, r.name); return `<div class="xf-row st-row" data-game="${esc(id)}" data-tip="${esc(r.name)} · ${fmtDur(r.total_seconds)} tracked on this PC"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(r.name.replace(/:.*/, ''))}</span></span><span class="st-w"><i class="st-bar"><b style="width:${Math.round(r.seven_day_seconds / max * 100)}%"></b></i>${hrs(r.seven_day_seconds).toFixed(1)}</span><span class="st-t">${fmtHrs(r.total_seconds)}h${tierMark(r.tier || 'tracked')}</span></div>`; }).join('');
    h += `<div class="st-legend">${tierMark('tracked')}Tracked on this PC · ${g.games_tracked} game${g.games_tracked === 1 ? '' : 's'} · ${fmtHrs(g.total_seconds)} h total</div>`;
    h += serverStatsHtml(g);
    h += `<div class="st-link"><a data-action="tag-game">&raquo; This is a game…</a> &nbsp; <a data-action="my-stats">&raquo; Full stats</a></div></div>`;
    list.innerHTML = h;
  }
  /* Account totals from the server (all devices + Steam + imports). */
  function serverStatsHtml(g) {
    if (!realMode()) return '';
    const sv = LIVE.server;
    let h = `<div class="xf-sec s-now"><i class="tri"></i>My Account (all devices)</div>`;
    if (!sv) return h + `<div class="fl-empty">Server totals unavailable right now, so this shows this PC only.</div>`;
    // The server's total plus the session running now (saved on the server when it ends).
    const np = g.now_playing, liveMin = np ? liveSeconds() / 60 : 0;
    const rows = (sv.games || []).filter(r => r.display_min > 0).slice(0, 8);
    if (!rows.length) h += `<div class="fl-empty">Nothing on your account yet. Hours are recorded live while you play.</div>`;
    h += rows.map(r => {
      const id = ensureGame(r.game_id, r.name);
      const mins = r.display_min + (np && np.game_id === r.game_id && r.display_tier === 'tracked' ? liveMin : 0);
      const tot = `${fmtHrs(mins * 60)}h${tierMark(r.display_tier === 'imported' ? 'imported' : 'tracked')}`;
      const unl = r.game_id.startsWith('exe:') && r.name === 'Unlisted game';
      const tip = unl ? 'A program you tagged as a game. Friends see "Playing a game" until it is added to Fraglink\'s game list; its hours stay with it.' : `${r.name} · tracked ${nf(r.tracked_min / 60)}h · imported ${nf(r.imported_min / 60)}h`;
      return `<div class="xf-row st-row" data-game="${esc(id)}" data-tip="${esc(tip)}"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(r.name.replace(/:.*/, ''))}</span></span><span class="st-w">${r.minutes_7d ? (r.minutes_7d / 60).toFixed(1) : ''}</span><span class="st-t">${tot}</span></div>`;
    }).join('');
    const o = sv.overall || {};
    h += `<div class="st-legend">${tierMark('tracked')}Tracked ${tierMark('imported')}Imported · ${nf((o.verified_min || 0) / 60)} h</div>`;
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
      h += `<div class="av-row" data-tip="${esc(new Date(a.ts * 1000).toLocaleString())}"><i class="gi">${gameIcon(kindTile)}</i><div class="av-tx"><b class="me">You</b> ${activityHtml(a)}</div><span class="av-t">${esc(whenLabel(a.ts))}</span></div>`;
    }
    h += `<div class="st-link"><a data-action="website">&raquo; All activity on playfraglink.com (later)</a></div></div>`;
    list.innerHTML = h;
  }
  function pendingReqCount() {
    const l = LIVE.friends && LIVE.friends.list;
    return l && Array.isArray(l.incoming) ? l.incoming.length : 0;
  }
  function paintFriendsBadge() {
    const n = pendingReqCount();
    $$('.xf-tab[data-tab=friends], .dk-vtabs [data-tab=friends]').forEach(t => {
      let b = t.querySelector('.fl-req-badge');
      if (!n) { if (b) b.remove(); return; }
      if (!b) { b = document.createElement('span'); b.className = 'fl-req-badge'; t.appendChild(b); }
      b.textContent = n > 99 ? '99+' : String(n);
      b.title = n === 1 ? '1 friend request' : n + ' friend requests';
    });
  }
  function render() {
    $$('.xf-tab, .xf-icontabs [data-tab], .dk-vtabs [data-tab]').forEach(t => t.classList.toggle('active', t.dataset.tab === state.tab));
    paintFriendsBadge();
    $('#sechead').textContent = state.tab === 'friends' && state.detectRun ? 'Detect Games' : state.tab === 'friends' && signedOut() ? 'Login' : ({ friends: 'Friends', stats: 'Stats', activity: 'Activity' })[state.tab] || '';
    $('#list').classList.toggle('nohbar', state.tab !== 'friends');
    const sl = list.scrollLeft, st = list.scrollTop;
    if (state.tab === 'stats') renderStats(); else if (state.tab === 'activity') renderActivity(); else renderFriends();
    list.scrollLeft = sl; list.scrollTop = st; LS.update();
    renderSide(); renderAddFriend();
  }

  /* ---------------- side panel ---------------- */
  function renderSide() {
    const el = SS.vp; requestAnimationFrame(SS.update);
    if (signedOut() && !state.detectRun) { el.innerHTML = offlineSide(); return; }
    // A selected group heading (not Friends Online): its name and a short description (classic style).
    // (Friends Online keeps the default view, like the classic client)
    if (String(state.sel || '').startsWith('g:') && state.sel !== 'g:online') {
      const k = state.sel.slice(2), cus = k.startsWith('c:');
      const name = cus ? k.slice(2) : k === 'fof' ? 'Friends of Friends Playing' : GROUP_NAMES[k] || k;
      el.innerHTML = sfSec(esc(name)) + `<div class="grp-help">${esc(GROUP_HELP[cus ? 'custom' : k] || '')}</div>`;
      return;
    }
    const f = friendsNow().find(x => x.id === state.sel);
    if (f) { el.innerHTML = friendPanelHtml(f); return; }
    // Nothing selected (classic layout): Activity Report, then the games on this PC with Launch links.
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
      return `<div class="ar-row" data-tip="${esc(new Date(x.at).toLocaleString())}"><i class="gi">${friendGameIcon(id)}</i><div><b>${esc(x.display_name || x.handle)}</b> ${what}<small>${esc(agoText(x.at))}</small></div></div>`;
    }).join('');
    return `<div class="side-sec"><h4>Activity Report</h4><a class="side-more" data-action="activity-more" data-tip="View more of your friends' activity">More</a></div>${rows}`;
  }
  function installedHtml() {
    const inst = (CATALOG || []).filter(g => g.installed && !g.excluded);
    const games = CATALOG === null ? '<li><span>Looking for games…</span></li>'
      : inst.map(g => { const id = ensureGame(g.id, g.name); return `<li data-tip="${esc(g.game_path || g.detect_path || '')}"><i class="gi">${gameIcon(id)}</i><span>${esc(g.name)}</span>${g.can_launch ? `<a data-action="launch" data-game="${esc(g.id)}">Launch</a>` : ''}</li>`; }).join('') || '<li><span>No games found on this PC yet.</span></li>';
    return `<div class="side-sec"><h4>${inst.length} Installed Game${inst.length === 1 ? '' : 's'}</h4><a data-action="detect-games" class="side-more side-detect" data-tip="Detect Games: look for installed games on this PC">Detect</a></div><ul class="games launch">${games}</ul>`;
  }
  function fmtMins(m) { m = Math.max(0, Math.round(m)); return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m}m`; }
  function agoText(iso) {
    const m = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
    return m < 1 ? 'just now' : m < 60 ? `${m}m ago` : m < 1440 ? `${Math.floor(m / 60)}h ago` : `${Math.floor(m / 1440)}d ago`;
  }
  /* Friend info panel (dev.17). Top to bottom: header (avatar, name, username, status),
     Playing Game (icon, name, Realm and a live "Playing for" timer), Stats (This week, Total), then
     the buttons. Empty rows and sections are left out. Real friends use presence plus the
     server's friend summary and profile; sample friends (browser preview) their own fields. */
  const sampleStart = {};
  function sessionStart(f) {
    if (f.real) { const t = f.presence && Date.parse(f.presence.session_started_at); return t || null; }
    if (!sampleStart[f.id]) { let h = 0; for (const c of f.id) h = (h * 31 + c.charCodeAt(0)) % 997; sampleStart[f.id] = Date.now() - (20 + h % 160) * 60000 - h % 60 * 1000; }
    return sampleStart[f.id];
  }
  // val is HTML (already escaped); plain values also get a title so a clipped value can be read
  const kv = (label, val) => val ? `<div class="sf-kv"><span>${label}</span><b${/</.test(val) ? '' : ` data-tip="${val}"`}>${val}</b></div>` : '';
  const sfSec = title => `<div class="side-sec"><h4>${title}</h4></div>`;
  // "1h 23m" / "45m": the friend's current game session, ticked each minute by clock()
  const fmtSess = ms => { const m = Math.max(0, Math.floor(ms / 60000)); return m >= 60 ? `${Math.floor(m / 60)}h ${m % 60}m` : `${m}m`; };
  const hrsText = sec => sec == null ? '' : fmtHrs(sec) + 'h';
  /* Profile pictures (dev.19), drawn like the website: an uploaded picture (fetched and
     cached by the app), a preset tile, or the Fraglink mark on one of six muted colours. */
  const AV_GLYPHS = ['\u2694', '\u25C9', '\u25B2', '\u2605', '\u25C6', '\u2726', '\u2663', '\u2699', '\u2620', '\u265B', '\u26A1', '\u271A'];
  const AVIMG = {}, avAsk = new Set(); let avTimer = 0;
  function avImg(url) {
    if (!url) return null;
    if (AVIMG[url] !== undefined) return AVIMG[url];
    if (FLB.avatarImages && !avAsk.has(url)) {
      avAsk.add(url); clearTimeout(avTimer);
      avTimer = setTimeout(async () => {
        const ask = [...avAsk].filter(u => AVIMG[u] === undefined); if (!ask.length) return;
        try { const r = await FLB.avatarImages(ask) || {}; for (const u of ask) AVIMG[u] = r[u] || null; render(); syncStatus(); } catch (e) { for (const u of ask) AVIMG[u] = null; }
      }, 50);
    }
    return null;
  }
  function avColor(f) { if (f.avatar && f.avatar.color != null) return f.avatar.color % 6; const n = String(f.handle || f.name || ''); let h = 0; for (const c of n) h = (h * 31 + c.codePointAt(0)) >>> 0; return h % 6; }
  function avatarHtml(f, size = 48, cls = 'sf-av') {
    const a = f.avatar || {}, c = avColor(f), st = `width:${size}px;height:${size}px`;
    if (a.kind === 'upload') { const src = avImg(size > 32 ? a.url_64 : a.url_32); if (src) return `<i class="fav-av up ${cls}" style="${st}"><img src="${src}" alt=""></i>`; }
    if (a.kind === 'preset') return `<i class="fav-av ${cls} p c${a.preset % 6}" style="${st};font-size:${Math.round(size * .46)}px">${AV_GLYPHS[a.preset % 12]}</i>`;
    return `<i class="fav-av ${cls} c${c}" style="${st}"><img class="mk" src="brand/v28-mark.svg" alt=""></i>`;
  }
  function friendStatus(f) {
    const playing = f.kind === 'playing' || f.kind === 'fof';
    if (playing) return f.away ? ['away', 'In game · Away'] : ['game', 'In game'];
    if (f.kind === 'online') return f.status === 'Away' ? ['away', 'Away'] : ['on', 'Online'];
    if (f.kind === 'req') return ['off', 'Friend request'];
    if (f.kind === 'out') return ['off', 'Request sent'];
    return ['off', 'Offline'];
  }
  // This week and Total, or null while nothing has loaded yet (Total comes from the friend's profile).
  function friendHistory(f) {
    if (!f.real) return { week: f.week != null ? f.week * 3600 : null, total: f.total ? f.total * 3600 : null };
    const s = LIVE.summaries[f.handle], p = LIVE.profiles[f.handle];
    loadSummary(f.handle); loadProfile(f.handle); // each refreshes at most once a minute
    if ((!s || s.err) && (!p || p.err)) return s && p ? { private: false, none: true } : null;
    const ps = p && !p.err ? p : null, ss = s && !s.err ? s : null;
    if ((ss && ss.week_seconds == null && !ss.shares_activity) || (ps && ps.history_visible === false)) return { private: true };
    return { week: ss ? ss.week_seconds : null, total: ps && ps.games_visible !== false && ps.overall ? ps.overall.total_min * 60 : null };
  }
  function friendPanelHtml(f) {
    const H = esc(f.handle || f.name), playing = f.kind === 'playing' || f.kind === 'fof';
    const [dot, stText] = friendStatus(f);
    const since = f.since ? 'Friends since ' + new Date(f.since).toLocaleDateString('en-AU', { day: 'numeric', month: 'short', year: 'numeric' }) : '';
    let h = `<div class="sf-head" data-tip="${esc(since)}">${avatarHtml(f, 48)}<div class="sf-id"><div class="sf-name">${esc(f.name)}</div>${f.handle || f.real ? `<div class="sf-user">@${H}</div>` : ''}<div class="sf-st"><i class="sf-dot ${dot}"></i>${esc(stText)}</div></div></div>`;
    if (f.kind === 'req') return h + `<p class="sf-note">Wants to be your friend.</p><div class="sf-foot"><button class="dk-btn" data-action="friend-accept" data-handle="${H}">Accept</button><button class="dk-btn" data-action="friend-decline" data-handle="${H}">Decline</button><a class="sf-small" data-action="friend-block" data-handle="${H}">Block</a></div>`;
    if (f.kind === 'out') return h + `<p class="sf-note">Friend request sent. They'll show up here once they accept.</p><div class="sf-foot"><a class="sf-small" data-action="friend-remove" data-handle="${H}">Cancel request</a></div>`;
    if (playing) {
      const t0 = sessionStart(f);
      h += sfSec('Playing Game') + `<div class="sf-game"><i class="gi sf-gi">${friendGameIcon(f.game)}</i><div class="sf-gn"><b data-tip="${esc(gameLabel(f))}">${esc(gameLabel(f))}</b></div></div>`;
      if (realmOf(f)) h += kv('Realm', esc(realmOf(f)));
      h += kv('Playing for', t0 ? `<span class="sf-timer" data-start="${t0}">${fmtT(Date.now() - t0)}</span>` : '');
    }
    if (f.kind === 'off' && f.last) h += kv('Last online', esc(f.last));
    if (f.kind === 'fof') {
      const m = mutualOf(f);
      h += sfSec(`Mutual Friends <small>(${m.length})</small>`) + `<ul class="games">${m.map(x => `<li><i class="noicon"></i><span>${esc(x)}</span></li>`).join('') || '<li><span>None known</span></li>'}</ul>`;
    } else {
      const s = friendHistory(f);
      if (!s) h += sfSec('Stats') + '<p class="sf-note">Loading…</p>';
      else if (s.private) h += sfSec('Stats') + `<p class="sf-note">${esc(f.name)} keeps their play history private.</p>`;
      else if (!s.none) {
        const t0 = playing ? sessionStart(f) : null;
        const rows = kv('This session', t0 ? `<span class="sf-sess" data-start="${t0}">${fmtSess(Date.now() - t0)}</span>` : '') + kv('This week', hrsText(s.week)) + kv('Total', s.total ? hrsText(s.total) : '');
        if (rows) h += sfSec('Stats') + rows;
      }
    }
    // Footer: View Profile, Message on Discord (only when the friend has Discord linked), Mute pop-ups
    h += `<div class="sf-foot"><button class="dk-btn" data-action="friend-profile" data-handle="${H}">View Profile</button>`;
    if (f.discord) h += `<button class="dk-btn" data-action="chat">Message on Discord</button>`;
    if (f.real && (f.kind === 'playing' || f.kind === 'online' || f.kind === 'off')) {
      const muted = (LIVE.friends.muted || []).includes(f.handle.toLowerCase());
      h += `<a class="sf-small" data-action="friend-mute" data-handle="${H}" data-mute="${muted ? 0 : 1}">${muted ? 'Unmute' : 'Mute'} pop-ups</a>`;
    }
    return h + '</div>';
  }
  const summaryAt = {}, profileAt = {};
  function loadSummary(h) {
    if (!FLB.friendSummary || (summaryAt[h] && Date.now() - summaryAt[h] < 60000)) return;
    summaryAt[h] = Date.now();
    FLB.friendSummary(h).then(s => { LIVE.summaries[h] = s; renderSide(); }).catch(() => { LIVE.summaries[h] = { err: true }; renderSide(); });
  }
  function loadProfile(h) {
    if (profileAt[h] && Date.now() - profileAt[h] < 60000) return;
    profileAt[h] = Date.now();
    if (!FLB.friendProfile) { LIVE.profiles[h] = { err: true }; return; }
    FLB.friendProfile(h).then(p => { LIVE.profiles[h] = p || { err: true }; renderSide(); }).catch(() => { LIVE.profiles[h] = { err: true }; renderSide(); });
  }
  async function refreshActivity() {
    if (!FLB.friendsActivity || (TAURI && !realMode())) { if (TAURI) LIVE.friendAct = { items: [] }; return; }
    try { LIVE.friendAct = await FLB.friendsActivity(); } catch (e) { LIVE.friendAct = LIVE.friendAct || { items: [] }; }
    for (const k in summaryAt) delete summaryAt[k];
    for (const k in profileAt) delete profileAt[k];
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
    $('#nub').dataset.tip = open ? 'Hide Info Panel' : 'Show Info Panel';
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
      <div class="wp-box"><h3>Your gaming stats and your friends, from anywhere</h3><div class="wp-hero"><svg viewBox="0 0 32 22"><use href="#fl-mark" fill="url(#flNavy)"/><use href="#fl-mark" fill="url(#scan)"/></svg><div><p><b>A tiny tray app logs every hour you play, on every launcher. Your profile, history, recaps and friends live here on the web.</b></p><p>Import your Steam hours so your profile is never empty. Export everything, any time.</p><button class="wp-btn" data-action="download">Download Fraglink 1.62</button></div></div></div>
      <div class="wp-box"><h3>Top Games This Week</h3><table class="wp-t"><tr><th>#</th><th>Game</th><th>Hours Played</th><th>Gamers</th></tr>${top.map((g, i) => `<tr><td>${i + 1}</td><td><span class="gi">${gameIcon(g)}</span> <a>${esc(GAMES[g].name)}</a></td><td>${(98000 - i * 9133).toLocaleString()}</td><td>${(41000 - i * 3890).toLocaleString()}</td></tr>`).join('')}</table></div>
      </div><div class="wp-side"><div class="wp-box"><h3>Friends Playing Now</h3>${FRIENDS.filter(f => f.kind === 'playing').map(f => `<div class="wp-fr"><span class="gi">${gameIcon(f.game)}</span><a>${esc(f.name)}</a><br><small>${esc(GAMES[f.game].name)}</small></div>`).join('')}</div>
      <div class="wp-box"><h3>Fraglink Stats</h3><p>Registered users: <b>9,812</b><br>Hours tracked this week: <b>120,497</b></p></div></div></div>${webFoot()}</div>`;
  }
  function webFoot() { return `<div class="wp-foot">Fraglink is a mock prototype · All data and characters fictional · No third-party assets used · <a>Privacy</a> · <a>Terms</a> · <a>Our data promise</a></div>`; }

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
    const wk = [...MY_HOURS].sort((a, b) => b.week - a.week)[0], at = [...MY_HOURS].sort((a, b) => b.total - a.total)[0];
    return `<svg class="sig" viewBox="0 0 440 110" width="440" height="110" font-family="Verdana, 'DejaVu Sans', sans-serif">
      <defs><linearGradient id="sg-${theme}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${B1}"/><stop offset="1" stop-color="${B2}"/></linearGradient>
      <pattern id="sgp-${theme}" width="4" height="4" patternUnits="userSpaceOnUse"><rect width="4" height="1" fill="rgba(255,255,255,.035)"/></pattern></defs>
      <rect x=".5" y=".5" width="439" height="109" rx="3" fill="url(#sg-${theme})" stroke="${L}"/><rect x="1" y="1" width="438" height="108" fill="url(#sgp-${theme})"/>
      <rect x="10" y="12" width="64" height="64" fill="#0c2236" stroke="${A}" stroke-width="1.5"/><text x="42" y="56" text-anchor="middle" font-size="34" font-weight="bold" fill="#fff">L</text>
      <circle cx="17" cy="84" r="3.5" fill="#3ddc5a"/><text x="25" y="87.5" font-size="9" fill="#9fe8a8">Online</text>
      <text x="86" y="26" font-size="15" font-weight="bold" fill="#fff">Lucrona</text><text x="160" y="26" font-size="9" fill="${A}">playfraglink.com/u/lucrona</text>
      <text x="86" y="44" font-size="8" fill="#8ea3c8" letter-spacing=".6">THIS WEEK</text>
      <text x="86" y="57" font-size="10.5" fill="#fff" font-weight="bold">${esc(GAMES[wk.game].name.replace(/:.*/, ''))}</text><text x="86" y="70" font-size="10" fill="${A}">${wk.week.toFixed(1)} hrs</text>
      <text x="262" y="44" font-size="8" fill="#8ea3c8" letter-spacing=".6">ALL TIME</text>
      <text x="262" y="57" font-size="10.5" fill="#fff" font-weight="bold">${esc(GAMES[at.game].name)}</text><text x="262" y="70" font-size="10" fill="${A}">${nf(at.total)} hrs <tspan fill="#7fb2ff">◆ imported</tspan></text>
      <line x1="86" y1="80" x2="430" y2="80" stroke="${L}"/>
      <text x="86" y="95" font-size="9" fill="#c9d4e3">${nf(VERIFIED)} hrs verified · Now playing <tspan fill="#fff" font-weight="bold">${esc(GAMES[ME.game].name.replace(/:.*/, ''))}</tspan></text>
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
    const total = `${r.total >= 1000 ? nf(r.total) : r.total.toFixed(1)} hrs`;
    return `<tr><td><span class="gi">${gameIcon(r.game)}</span> <a>${esc(GAMES[r.game].name)}</a>${r.note ? `<div class="pf-note">“${esc(r.note)}”</div>` : ''}</td><td>${r.week.toFixed(1)} hrs</td><td class="bar"><i style="width:${max ? (r.week / max * 100) : 0}%"></i></td><td>${total}</td><td><span class="tb tb-${r.tier}" data-tip="${esc(r.src)}">${t.sym} ${t.label}</span></td></tr>`;
  }
  function profilePage(theme) {
    const tw = sumBy(MY_HOURS, r => r.week);
    const max = Math.max(...MY_HOURS.map(r => r.week));
    const LINKED = [['Steam', 'lucrona_au', 'st'], ['Discord', 'lucrona', 'dc'], ['Twitch', 'lucrona_plays', 'tw'], ['Battle.net', 'Lucrona#1187', 'bn']];
    return `<div class="wp prof-${theme}">${webHeader('My Profile')}
      <div class="pf-head"><div class="pf-av">L</div><div class="pf-id"><h2>Lucrona</h2><div class="pf-st"><span class="gi">${gameIcon(ME.game)}</span> Currently playing <b>${esc(GAMES[ME.game].name)}</b></div>
      <div class="pf-links"><a>Add as Friend</a> | <a>Message on Discord</a> | <a>Compare hours</a> | <a data-profile-theme>Theme: ${theme === 'gold' ? 'Gold 567' : 'Cyan Grid'}</a> | <a>Edit layout (drag · hide · reorder)</a></div></div>
      <div class="pf-rank"><b>${nf(VERIFIED)}</b><small>hours verified</small></div></div>
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
          <div class="pf-embed"><label>Image URL<input readonly value="https://playfraglink.com/sig/lucrona.png"></label><label>BBCode<input readonly value="[url=https://playfraglink.com/u/lucrona][img]https://playfraglink.com/sig/lucrona.png[/img][/url]"></label><span>Also: Markdown · HTML · 320×100 Twitch panel · 350×20 userbar</span></div></div></div>
        <div class="wp-box" data-sec="achievements"><h3>Achievements <small class="h3n">24 of 35 tiers · play-time only</small></h3><div class="pf-ach">${ACH.map(a => `<div class="ach ach-${a.c}" data-tip="${esc(a.k)} · ${a.tier}"><i><b>${esc(a.g)}</b></i><div><b>${esc(a.k)}</b><span class="t">${a.tier}</span><small>${esc(a.prog)}</small></div></div>`).join('')}</div></div>
        <div class="wp-box" data-sec="history"><h3>Gaming History</h3><table class="wp-t pf-hist"><tr><th>Game</th><th>Past 7 Days</th><th></th><th>Total</th><th>Source</th></tr>
          ${MY_HOURS.map(r => histRow(r, max)).join('')}
          <tr class="tot"><td>Overall Hours</td><td>${tw.toFixed(1)} hrs</td><td></td><td>${nf(VERIFIED)} hrs</td><td>verified</td></tr></table>
          <div class="pf-legend"><span class="tb tb-tracked">● Tracked</span> logged by Fraglink or Steam's official playtime · <span class="tb tb-imported">◆ Imported</span> the game's own counter (WoW /played via addon) · <a>Show entire gaming history</a></div></div>
        <div class="wp-box" data-sec="wow"><h3>World of Warcraft Characters <small class="h3n">◆ /played: 11,214 h across 6 characters</small></h3><div class="pf-wow">
          <div class="wow-main">${portrait()}<div class="wow-id"><div class="wow-name">Fragnar <span class="ver big">✓ Verified via Battle.net</span></div>
            <div class="wow-line">Level 80 Night Elf <b class="cls">Hunter</b> · Beast Mastery</div><div class="wow-guild">&lt;Southern Cross Vanguard&gt; · Frostmourne (US · Oceanic)</div>
            <div class="wow-stats"><div><small>Item level</small><b>639</b></div><div><small>M+ score</small><b>2,415</b><em>Raider.IO</em></div><div><small>Raid</small><b>8/8 H</b><em>6/8 M</em></div><div><small>Achievements</small><b>18,450</b></div><div><small>/played</small><b>4,812 h</b><em>◆ imported</em></div></div>
            <div class="wow-foot">Last updated 2 h ago · Blizzard updates character data after you log out · Fictional mock character, placeholder art</div></div></div>
          <ul class="wow-alts">
            <li>${portrait('#6a5a2a', '#1a1408', true)}<div><b>Fraglet</b> · Lvl 60 Human Warrior · Classic Era · Arugal</div><span class="ver">✓ Verified</span></li>
            <li>${portrait('#3a3f5a', '#0c0f1c', true)}<div><b>Oldfrag</b> · Lvl 42 Dwarf Priest · Turtle WoW (private server)</div><span class="unver">Unverified · manual</span></li></ul></div></div>
        <div class="wp-box" data-sec="activity"><h3>Recent Activity</h3><ul class="pf-act"><li>Played <b>Half-Life 2: Deathmatch</b> for 2.1 hours · today</li><li>Imported <b>/played</b> for 6 WoW characters · Tue</li><li>Unlocked <b>No Days Off · Gold</b> · Mon</li><li>Weekly recap: <b>22.3 h</b>, top game HL2:DM · <a>view recap</a> · Mon</li><li>Added <a>Riftwalker</a> as a friend · Sep 20</li></ul></div>
      </div></div>${webFoot()}</div>`;
  }
  function website(page) {
    const w = $('#web'); w.hidden = false;
    const prof = params.get('prof') === 'gold' ? 'gold' : 'cyan';
    state.webPage = page;
    $('#web-url').textContent = page === 'profile' ? 'https://playfraglink.com/u/lucrona' : 'https://playfraglink.com/';
    const pg = $('#web-page'), st = pg.scrollTop;
    pg.innerHTML = page === 'profile' ? profilePage(prof) : homePage();
    pg.scrollTop = st;
  }
  $('#web-page').addEventListener('click', e => {
    const a = e.target.closest('[data-web]'); if (a) { website(a.dataset.web); return; }
    if (e.target.closest('[data-profile-theme]')) { params.set('prof', params.get('prof') === 'gold' ? 'cyan' : 'gold'); website('profile'); return; }
  });

  /* ---------------- dialogs / misc ---------------- */
  // "0.2.0-8" (the MSI-safe build version) reads as "0.2.0-dev.8"
  const devVer = v => String(v || '').replace(/^(\d+\.\d+\.\d+)-(\d+)$/, '$1-dev.$2');
  // Change your nickname (the name friends see). The username stays; staff can change that.
  function openNickname() {
    if (signedOut() || !FLB.setNickname) return;
    const me = realMode() && LIVE.friends.me ? LIVE.friends.me : ME.name, cur = (realMode() ? LIVE.meNick : ME.nick) || me;
    notice('Change Nickname', `<div class="nick-dlg"><p>Your nickname is the name friends see in Fraglink and on the website. Your username <b>@${esc(me)}</b> stays the same.</p>
      <label>Nickname <input type="text" id="nick-in" maxlength="32" value="${esc(cur)}" spellcheck="false"></label>
      <small>Up to 32 characters, 3 changes a day. It can't be someone else's name or look like staff. Leave it empty to use your username.</small>
      <div class="nick-foot"><span id="nick-msg" class="msg"></span></div></div>`);
    $('#notice .dlg-foot').innerHTML = `<button class="hw" type="button" id="nick-save">Save</button> <button class="hw" data-action="notice-close" type="button">Cancel</button>`;
    const inp = $('#nick-in'); inp.focus(); inp.select();
    const save = async () => {
      const b = $('#nick-save'), m = $('#nick-msg'); b.disabled = true; m.className = 'msg'; m.textContent = 'Saving…';
      try {
        const r = await FLB.setNickname(inp.value);
        if (realMode()) LIVE.meNick = r.display_name; else ME.nick = r.display_name;
        syncStatus(); $('#notice').hidden = true;
        flash(`Nickname changed to ${r.display_name}.`);
      } catch (e) { m.className = 'msg bad'; m.textContent = String(e && e.message || e); b.disabled = false; }
    };
    $('#nick-save').addEventListener('click', save);
    inp.addEventListener('keydown', e => { if (e.key === 'Enter') save(); });
  }
  document.addEventListener('click', e => { if (e.target.closest && e.target.closest('#username, #px-user')) openNickname(); });
  // a nickname changed in Options (or on the website) shows up when the window is next focused
  addEventListener('focus', () => { if (realMode()) { LIVE.meAvFor = null; syncStatus(); } });
  /* Staff announcement banner under the header (the website's banner). Hidden ones stay hidden. */
  let newsCur = null, newsEnd = null;
  async function loadNews() {
    if (!FLB.announcement || (TAURI && !realMode())) return showNews(null);
    try { const r = await FLB.announcement(); showNews(r && r.announcement); } catch (e) { /* offline: keep what's shown */ }
  }
  function showNews(a) {
    const el = $('#news'), gone = (localStorage.getItem('fl-news-hidden') || '') === String(a && a.id);
    const expired = a && a.ends_at && Date.parse(a.ends_at) <= Date.now();
    newsCur = a && a.message && !gone && !expired ? a : null;
    // an announcement with an end time disappears right when it ends, not at the next check
    clearTimeout(newsEnd); newsEnd = null;
    if (newsCur && newsCur.ends_at) newsEnd = setTimeout(() => showNews(newsCur), Math.min(Date.parse(newsCur.ends_at) - Date.now() + 500, 2147483000));
    el.hidden = !newsCur; document.body.classList.toggle('has-news', !!newsCur);
    if (!newsCur) return;
    el.className = 'fl-news ' + (/^(warn|warning|alert)$/.test(a.style || '') ? 'warn' : 'info') + (a.link ? ' linked' : '');
    el.querySelector('.nw-t').textContent = String(a.message).slice(0, 200);
    el.dataset.tip = a.link ? 'Click to open' : '';
  }
  $('#news').addEventListener('click', e => {
    if (!newsCur) return;
    if (e.target.closest('.nw-x')) { localStorage.setItem('fl-news-hidden', String(newsCur.id)); showNews(null); return; }
    if (newsCur.link && /^https:\/\//.test(newsCur.link)) { if (FLB.openUrl) FLB.openUrl(newsCur.link).catch(() => {}); else open(newsCur.link, '_blank'); }
  });
  setTimeout(loadNews, 1500); setInterval(loadNews, 2 * 60 * 1000); addEventListener('focus', () => loadNews());
  /* "What's new" after an update. Edit WHATS_NEW for each release (keyed by the
     app version as devVer() shows it); versions without an entry just get the link. */
  const WHATS_NEW = {
    '0.3.0-beta.9': [
      'Tray icon shows one mark: update ready, friend requests, or your status',
      'Friend requests badge on the Friends tab',
      'Session time in the friend side panel',
      'New Help menu with FAQ and Changelog',
      'Hours imported dates for Steam and Battle.net in Connections',
    ],
  };
  function whatsNew() {
    const s = LIVE.settings; if (!s || !s.version) return;
    const v = devVer(s.version), last = SET().last_run_version;
    // Not a first install: an earlier version ran (older builds didn't record it, so use other traces).
    const before = last || SET().login_last || SET().games_detected_once;
    if (last === v) return;
    saveSetting('last_run_version', v);
    if (!before) return;
    const items = WHATS_NEW[v] || [];
    notice('Fraglink updated', `<p><b>Fraglink updated to ${esc(v)}</b></p>${items.length ? `<ul class="wn-list">${items.map(i => `<li>${esc(i)}</li>`).join('')}</ul>` : ''}<p><a data-action="changelog-web">See full changelog</a></p>`);
  }
  function notice(title, html) {
    const foot = $('#notice .dlg-foot'); foot.innerHTML = `<button class="hw" data-action="notice-close" type="button">OK</button>`;
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
  // Short status message at the bottom of the main window (fades after a few seconds).
  let flashTimer = null;
  function flash(text) {
    let f = $('#fl-flash'); if (!f) { f = document.createElement('div'); f.id = 'fl-flash'; f.className = 'fl-flash'; ($('#app') || document.body).appendChild(f); }
    f.textContent = text; f.style.setProperty('--af-side', $('#app') && $('#app').classList.contains('side-open') && $('.xf-side') ? $('.xf-side').offsetWidth + 'px' : '0px');
    f.classList.add('on'); clearTimeout(flashTimer); flashTimer = setTimeout(() => f.classList.remove('on'), params.get('flash') ? 1e9 : 3500);
  }
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
      $('#cand-list').innerHTML = cands.length ? cands.map(c => `<div class="g-row cand" data-exe="${esc(c.exe)}" data-tip="${esc(c.path)}"><b>${esc(c.exe)}</b><small>${esc(c.path)}</small></div>`).join('')
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
    'privacy': () => notice('What Fraglink reads', '<ul class="fl-ul"><li>The <b>list of running programs</b> (names and install paths) every 5 seconds, matched against a list of known games.</li><li>Steam\'s own <b>RunningAppID</b> value and its library files, the <b>Epic Games</b> install manifests and <b>GOG Galaxy</b>\'s install list, to recognise games you installed through them.</li></ul>It never reads game memory, never injects anything into games and never hooks input, so it is safe with anti-cheat. Nothing leaves this PC unless you choose to <b>sign in to sync</b> in Options. Hours only count while you are signed in and connected: while the app runs it tells the server which game you are in (just the game\'s id) and the server records the time itself. Nothing is tracked or queued offline. Your friends see your status and the game you\'re playing now, named from Fraglink\'s own game list. A program you tagged with <b>This is a game</b> shows as <b>Playing a game</b> to everyone else until it is added to that list; your own name for it stays on this PC. Friends also see your recent sessions in their Activity Report and your hours this week, unless you turn that off in Options &gt; Connections. <b>Invisible</b> shows you as offline. Paths are never sent; the only program names sent are the .exe of a program you tagged as a game.'),
    'add-friend': () => {
      if (!realMode() && TAURI) { state.tab = 'friends'; render(); notice('Add Friend', 'Log in first. Friends need a Fraglink account.'); return; }
      state.tab = 'friends'; state.addFriend = { q: '', msg: 'Add me to your friends list' }; render();
      setTimeout(() => $('#add-handle') && $('#add-handle').focus(), 0);
    },
    'add-cancel': () => { state.addFriend = null; render(); },
    'add-friend-go': async () => {
      const A = state.addFriend; if (!A) return;
      const h = (A.q || '').trim(); if (!h) return;
      Object.assign(A, { users: null, sel: null, err: '', self: '', out: '', bad: false }); renderAddFriend();
      const myId = realMode() && LIVE.friends.me ? LIVE.friends.me : ME.name, lc = h.toLowerCase();
      try { const r = await FLB.friendSearch(h); A.users = (r && r.users) || []; }
      catch (e) { A.users = []; A.err = String(e && e.message || e); }
      if (state.addFriend !== A) return;
      if (!A.users.length && !A.err && myId && (lc === String(myId).toLowerCase() || lc === String(myId).toLowerCase().replace(/^@/, ''))) A.self = "That's you!";
      A.sel = null; if (A.users.length === 1) { actions['add-pick']({ dataset: { handle: A.users[0].handle } }); return; }
      renderAddFriend();
    },
    'add-pick': a => { const A = state.addFriend; A.sel = a.dataset.handle; const r = relationOf(A.sel); A.out = r === 'friend' ? `${A.sel} is already your friend.` : r === 'out' ? `Request already sent to ${A.sel}.` : ''; A.bad = false; renderAddFriend(); },
    // Always answers: success closes the window with a short status message; problems show inside it.
    // (The old code wrote the result into #add-msg and then refreshFriends() re-rendered the window,
    // wiping the message, so nothing seemed to happen.)
    'add-friend-send': async a => {
      const A = state.addFriend, h = a.dataset.handle; if (!A || !h || A.busy) return;
      const rel = relationOf(h);
      if (rel === 'friend' || rel === 'out') { Object.assign(A, { out: rel === 'friend' ? `${h} is already your friend.` : `Request already sent to ${h}.`, bad: true }); renderAddFriend(); return; }
      Object.assign(A, { busy: true, out: '', bad: false }); renderAddFriend();
      try {
        const r = await FLB.friendAdd(h);
        if (state.addFriend !== A) return;
        state.addFriend = null; render();
        flash(r === 'accepted' ? (rel === 'in' ? `You and ${h} are now friends.` : `${h} is already your friend.`) : `Friend request sent to ${h}.`);
        refreshFriends();
      } catch (e) {
        if (state.addFriend !== A) return;
        Object.assign(A, { busy: false, out: String(e && e.message || e) || "Couldn't send the request. Try again.", bad: true }); renderAddFriend();
      }
    },
    'update-now': a => { a.disabled = true; a.classList.add('busy'); a.dataset.tip = 'Restarting…'; FLB.installUpdate().catch(e => { a.disabled = false; a.classList.remove('busy'); a.dataset.tip = 'Update ready - click to restart'; toastErr(e); }); },
    // Help > Check for Updates: "You're up to date", or the update downloads, installs and restarts.
    'check-updates': async () => {
      notice('Check for Updates', '<p class="upd-msg" id="upd-msg">Checking for updates…</p>');
      let r; try { r = await FLB.checkUpdates(); } catch (e) { r = { result: 'failed', message: String(e) }; }
      const m = $('#upd-msg'); if (!m) return;
      if (r.result === 'up_to_date') m.innerHTML = `You're up to date.<br><small>Fraglink ${esc(devVer(r.version))}</small>`;
      else if (r.result === 'installing') m.innerHTML = `Installing Fraglink ${esc(devVer(r.version))}…<br><small>Fraglink restarts when it's done.</small>`;
      else m.innerHTML = `Couldn't check for updates.<br><small>${esc(r.message || '')}</small>`;
    },
    'changelog-web': () => { const u = 'https://playfraglink.com/changelog'; if (FLB.openUrl) FLB.openUrl(u).catch(() => {}); else open(u, '_blank'); },
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
      try { await FLB.syncLogin(srvVal(), u, pw); state.loginMode = null; } catch (e) {
        const m = String(e && e.message || e);
        // Two-step sign-in: the server wants a code; keep the ticket and ask for it.
        if (m.startsWith('mfa_required:')) { state.mfaTicket = m.slice(13); state.mfaServer = srvVal(); state.loginMode = 'mfa'; state.loginErr = ''; state.loginBusy = ''; render(); setTimeout(() => $('#lg-code') && $('#lg-code').focus(), 0); return; }
        state.loginErr = m;
      }
      state.loginBusy = ''; refreshFriends();
    },
    'mfa-go': async () => {
      const c = ($('#lg-code').value || '').trim();
      if (!c) { state.loginErr = 'Enter the code from your authenticator app.'; render(); return; }
      state.loginErr = ''; state.loginBusy = 'Checking code…'; render();
      try { await FLB.syncLoginMfa(state.mfaServer || srvVal(), state.mfaTicket || '', c); state.loginMode = null; state.mfaTicket = null; }
      catch (e) { const m = String(e && e.message || e); state.loginErr = m; if (/too long|password again/i.test(m)) { state.loginMode = null; state.mfaTicket = null; } }
      state.loginBusy = ''; refreshFriends();
    },
    'signup-go': async () => {
      const u = ($('#lg-nuser').value || '').trim(), pw = $('#lg-npass').value || '', pw2 = $('#lg-npass2').value || '', em = ($('#lg-email').value || '').trim();
      if (!/^[A-Za-z0-9_-]{3,20}$/.test(u)) { state.loginErr = 'Usernames are 3-20 characters: letters, digits, _ or -.'; render(); return; }
      if (pw.length < 10) { state.loginErr = 'Use a password of at least 10 characters.'; render(); return; }
      if (pw !== pw2) { state.loginErr = "The passwords don't match."; render(); return; }
      const dd = ($('#lg-dob-d') && $('#lg-dob-d').value) || '', dm = ($('#lg-dob-m') && $('#lg-dob-m').value) || '', dy = ($('#lg-dob-y') && $('#lg-dob-y').value) || '';
      if (!dd || !dm || !dy) { state.loginErr = 'Please enter your date of birth.'; render(); return; }
      const birth = `${dy}-${dm}-${dd}`;
      state.loginErr = ''; state.loginBusy = 'Creating your account…'; render();
      try { await FLB.syncSignup(srvVal(), u, pw, em, birth); state.loginMode = null; } catch (e) { state.loginErr = String(e && e.message || e); }
      state.loginBusy = ''; refreshFriends();
    },
    'mail-support': () => { const u = 'mailto:support@playfraglink.com'; if (FLB.openUrl) FLB.openUrl(u).catch(() => {}); else open(u, '_blank'); },
    'login-back': () => { state.loginMode = null; state.mfaTicket = null; state.loginErr = ''; render(); },
    'login-browser': () => { state.loginErr = ''; FLB.syncSignIn(srvVal()).then(refreshFriends).catch(toastErr); },
    'login-cancel': () => FLB.syncCancel().then(refreshFriends).catch(toastErr),
    'register': () => { state.tab = 'friends'; state.loginMode = 'create'; state.loginErr = ''; render(); setTimeout(() => $('#lg-nuser') && $('#lg-nuser').focus(), 0); },
    'privacy-policy': () => { if (TAURI && FLB.openWebsite) FLB.openWebsite('privacy').catch(toastErr); else notice('Privacy Policy', 'playfraglink.com/privacy'); },
    'forgot': () => { if (TAURI && FLB.openWebsite) FLB.openWebsite('forgot').catch(toastErr); else notice('Forgot Password', LATER); },
    'invite': () => notice('Invite via Email', 'Inviting friends by email arrives with the Fraglink website. For now, send them the download link and your username.'),
    'friend-accept': a => FLB.friendRespond(a.dataset.handle, true).then(refreshFriends).catch(toastErr),
    'friend-decline': a => FLB.friendRespond(a.dataset.handle, false).then(() => { state.sel = null; refreshFriends(); }).catch(toastErr),
    'friend-remove': a => { if (a.dataset.confirm && !confirm(`Remove ${a.dataset.handle} from your friends?`)) return; FLB.friendRemove(a.dataset.handle).then(() => { state.sel = null; refreshFriends(); }).catch(toastErr); },
    'friend-block': a => { if (!confirm(`Block ${a.dataset.handle}? They won't be able to send you requests or see your status. They aren't told.`)) return; FLB.friendBlock(a.dataset.handle, true).then(() => { state.sel = null; refreshFriends(); }).catch(toastErr); },
    'friend-mute': a => FLB.friendMute(a.dataset.handle, a.dataset.mute === '1').then(refreshFriends).catch(toastErr),
    'my-profile': () => notice('My Profile', LATER),
    'friend-profile': a => { const h = a && a.dataset && a.dataset.handle || a && a.dataset && a.dataset.key; if (TAURI && FLB.openWebsite && h) FLB.openWebsite('u/' + h).catch(toastErr); else notice('Profile', LATER); },
    'my-stats': () => notice('Full stats', LATER), 'website': () => notice('playfraglink.com', LATER),
    'about': async () => { let v = ''; try { const st = await FLB.settings(); v = `${devVer(st.version)}${st.sim ? ' (simulation mode)' : ''}`; } catch (e) {}
      notice('About Fraglink', `<div class="about-box"><img class="about-lock" src="brand/v28-lockup.svg" alt="Fraglink" draggable="false"><b class="about-ver">Fraglink ${esc(v)}</b><div class="about-slogan">PLAY. CONNECT. TRACK.</div>
        <p>Track your game hours and see what friends are playing.</p><p class="about-c">\u00a9 2026 Lucrona. All rights reserved.</p>
        <p class="about-credits">Logo: <i>Split Orbit</i>. Fonts: Saira Expanded, Russo One (SIL OFL 1.1), Wine Tahoma clone (LGPL).</p></div>`); },
    'chat': () => { const f = friendsNow().find(x => x.id === state.sel); if (f && f.real) { notice('Chat', 'Fraglink has no built-in chat. Linking Discord so this opens a DM arrives in a later update.'); return; } openChat(state.sel); },
    'join': () => notice('Join Game', 'Joining friends arrives with friends in a later update.'),
    'clear-sel': () => { state.sel = null; render(); },
    'download': () => notice('Download', 'You are already running Fraglink.'),
    'always-on-top': () => { const v = !setOn('always_on_top', false); saveSetting('always_on_top', v ? '1' : '0'); syncMenus(); },
    'sign-out': async () => { if (!realMode()) { openOptions('account'); return; } if (!confirm('Sign out of your Fraglink account on this PC? Tracking keeps running; friends disappear until you sign in again.')) return; try { await FLB.syncSignOut(); } catch (e) { toastErr(e); } refreshFriends(); },
    'toggle-offline': () => { saveSetting('show_offline', setOn('show_offline') ? '0' : '1'); syncMenus(); render(); },
    'toggle-fof': () => { saveSetting('show_fof', setOn('show_fof') ? '0' : '1'); syncMenus(); render(); },
    'show-groups': () => { saveSetting('hidden_groups', '[]'); render(); },
    'help-contents': () => notice('Fraglink Help', '<ul class="fl-ul"><li><b>Friends list</b>: click the arrow beside a group heading (or double-click the heading) to fold or unfold it (Fraglink remembers), hover a friend for details, right-click a heading to add a custom group or hide the category (Tools &gt; Friends &gt; Show Hidden Categories brings it back).</li><li><b>Info panel</b>: the arrow on the right edge. It shows your games with <b>Launch</b> links, or details for the friend or group you selected.</li><li><b>Status</b>: the box under the banner. While you play, it shows the game automatically.</li><li><b>Games</b>: Options &gt; Games lists what Fraglink found; set paths and launch options there, or add a game by hand.</li><li><b>Nickname</b>: click your own name to change the name friends see. Your username stays the same.</li><li><b>Privacy</b>: see Options &gt; Privacy for what friends can see.</li></ul>'),
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
      // the arrow folds the group; clicking the heading only selects it (grey bar, classic style)
      if (e.target.closest('.tri')) { state.collapsed[k] = !state.collapsed[k]; saveSetting('collapsed_groups', JSON.stringify(Object.keys(state.collapsed).filter(x => state.collapsed[x]).slice(0, 30))); }
      else state.sel = 'g:' + k;
      render(); return;
    }
    const so = e.target.closest('[data-sort]'); if (so) { const k = so.dataset.sort; state.sort = { key: k, dir: state.sort.key === k ? -state.sort.dir : 1 }; render(); return; }
    const hs = e.target.closest('[data-hsort]'); if (hs) { const k = hs.dataset.hsort; state.hoursSort = { key: k, dir: state.hoursSort.key === k ? -state.hoursSort.dir : -1 }; render(); return; }
    const w = e.target.closest('[data-web]'); if (w) { website(w.dataset.web); return; }
    const r = e.target.closest('[data-friend]'); if (r) { state.sel = r.dataset.friend; render(); return; }
    // empty space: deselect, the side panel goes back to its default view
    if (state.sel && !e.target.closest('a,button,input,select,label')) { state.sel = null; render(); }
  });
  document.addEventListener('keydown', e => { if (e.key === 'Enter' && e.target.id === 'add-handle') actions['add-friend-go'](); if (e.key === 'Enter' && e.target.id === 'custom-status') actions['custom-status-go'](); if (e.key === 'Escape') { if (!ctx.hidden) hideCtx(); else if (state.addFriend) actions['add-cancel'](); else if (state.sel && !e.target.closest('input,textarea,select')) { state.sel = null; render(); } } });
  /* "Track <name> as a game?" bar (also offered as a popup). Yes tracks it from now on and
     sends the exe + name to the server for review; Not a game never asks again. */
  const exeName = exe => { const s = String(exe || '').replace(/\.exe$/i, '').replace(/[_-]+/g, ' ').replace(/\s+/g, ' ').trim(); return (s || String(exe || '')).slice(0, 80); };
  function showTrackAsk(c) {
    let bar = $('#track-ask');
    if (!c || !c.exe) { if (bar) bar.hidden = true; return; }
    if (!bar) { bar = document.createElement('div'); bar.id = 'track-ask'; bar.className = 'fl-ask'; document.body.appendChild(bar); }
    const name = exeName(c.exe);
    // beta.5: Discord-style "Playing <name>? Add it!"; × just hides it for now
    bar.innerHTML = `Playing <b>${esc(name)}</b>? <span class="fl-ask-b"><button type="button" class="add-it" data-yes="1">Add it!</button><button type="button" class="not-game" data-yes="0">Not a game</button><button type="button" class="ask-x" aria-label="Hide">×</button></span>`;
    bar.dataset.tip = c.path || c.exe; bar.hidden = false;
    bar.querySelector('.ask-x').onclick = () => { bar.hidden = true; };
    bar.querySelectorAll('button[data-yes]').forEach(b => b.onclick = async () => {
      bar.hidden = true;
      try { await FLB.trackAnswer(c.exe, b.dataset.yes === '1', name); refreshSoon(); } catch (e) { toastErr(e); }
    });
  }
  /* Rich friend tooltip (Windows-style), shown after a short hover; the info panel stays click-only. */
  const tip = $('#ftip'); let tipTimer = null, tipRow = null;
  const mutualOf = f => (f.mutual || ((f.info || '').match(/via ([^·]+)/) || [])[1] || '').split(',').map(x => x.trim()).filter(Boolean);
  function tipHtml(f) {
    const L = (k, v) => v ? `<div><b>${k}</b> ${esc(v)}</div>` : '';
    // nickname first, the permanent @username beside it
    const user = f.handle || f.name, nick = f.name || user;
    let h = `<div class="tip-who"><b>${esc(nick)}</b> <span>@${esc(user)}</span></div>`;
    if (f.kind === 'playing' || f.kind === 'fof') {
      h += L('Playing:', GAMES[f.game] && gameLabel(f)) + L('Realm:', realmOf(f));
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
  /* Measure at the top-left (a fixed box near the right edge shrinks to the space left
     there, which wrapped tips into a thin column), then keep the whole tip in the window. */
  function placeTip(el, x, below, above) { window.flPlaceTip(el, x, below, above); }
  list.addEventListener('mouseover', e => {
    const r = e.target.closest('.xf-row[data-friend]');
    if (r === tipRow) return;
    hideTip(); if (!r) return;
    tipRow = r; const mx = e.clientX;
    tipTimer = setTimeout(() => {
      const f = friendsNow().find(x => x.id === r.dataset.friend); if (!f || !r.isConnected) return;
      tip.innerHTML = tipHtml(f); tip.hidden = false;
      const rr = r.getBoundingClientRect();
      placeTip(tip, mx + 4, rr.bottom + 2, rr.top - 2); // never over the row
    }, 450);
  });
  list.addEventListener('mouseleave', hideTip);
  list.addEventListener('mousedown', hideTip);
  list.addEventListener('wheel', hideTip, { passive: true });
  /* Every other hover hint uses the same dark tooltip (no native title attributes in this window:
     they'd show a second, cream tooltip on Windows). Text that is its own hint shows only when cut off. */
  const stip = $('#stip'); let stipTimer = null, stipEl = null;
  function hideSTip() { clearTimeout(stipTimer); stipEl = null; stip.hidden = true; }
  document.addEventListener('mouseover', e => {
    let el = e.target.closest ? e.target.closest('[data-tip],[title]') : null;
    if (el && el.hasAttribute('title')) { if (!el.dataset.tip) el.dataset.tip = el.getAttribute('title'); el.removeAttribute('title'); }
    if (el && el.closest('.xf-row[data-friend]')) el = null; // friend rows have the rich tooltip
    if (el === stipEl) return;
    hideSTip(); if (!el || !el.dataset.tip) return;
    const t = el.dataset.tip;
    if (t.trim() === el.textContent.trim() && el.scrollWidth <= el.clientWidth + 1) return;
    stipEl = el; const mx = e.clientX, my = e.clientY;
    stipTimer = setTimeout(() => {
      if (!el.isConnected || stipEl !== el) return;
      stip.textContent = el.dataset.tip; stip.hidden = false;
      placeTip(stip, mx + 4, my + 20, my - 6);
    }, 450);
  }, true);
  document.addEventListener('mousedown', hideSTip, true);
  document.addEventListener('wheel', hideSTip, { passive: true, capture: true });
  addEventListener('blur', hideSTip);
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
        items.push({ id: 'fav', label: fav ? 'Remove from Favourites' : 'Add to Favourites' });
        const sub = groups.map((g, i) => ({ id: 'move:' + i, label: g, checked: cur === g }));
        if (cur) sub.push({ id: 'move:-', label: '(No group)' });
        if (sub.length) sub.push({ sep: true });
        sub.push({ id: 'add-group', label: 'Add Custom Group…' });
        items.push({ label: 'Move to Group', items: sub }, { sep: true }, { id: 'remove', label: 'Remove Friend…' }, { id: 'block', label: 'Block User…' });
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
    const items = [{ id: 'add-group', label: 'Add Custom Group…' }, { id: 'hide', label: 'Hide Category' }];
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
  list.addEventListener('dblclick', e => {
    const s = e.target.closest('[data-sec]');
    if (s && !e.target.closest('.tri')) { const k = s.dataset.sec; state.collapsed[k] = !state.collapsed[k]; saveSetting('collapsed_groups', JSON.stringify(Object.keys(state.collapsed).filter(x => state.collapsed[x]).slice(0, 30))); render(); return; }
    const r = e.target.closest('[data-friend]'); if (r) openChat(r.dataset.friend); });
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
    if (btn.disabled) { btn.querySelector('span').textContent = 'Offline'; btn.dataset.user = '(Offline)'; $('#username').textContent = '(Offline)'; if ($('#px-user')) $('#px-user').textContent = '(Offline)'; return; }
    const auto = myStatus === 'online' && autoAway, game = np && setOn('share_game') ? np.name : '';
    const txt = auto ? (game ? game + ' (Away)' : 'Away') : myStatus === 'away' ? 'Away' : myStatus === 'invisible' ? 'Invisible' : customOn ? custom : game ? game : 'Online';
    // hover: the game with its detail (WoW realm), e.g. "Playing World of Warcraft: Forever · Frostmourne"
    const detail = SHOW_REALM && game && np.detail ? ' · ' + np.detail : '';
    const tipTxt = detail && txt.includes(game) ? txt.replace(game, game + detail) : txt;
    // the game's 16 px icon and its name (no "Playing"; the hover says it), nothing when not in a game
    const stIcon = game && txt.startsWith(game) ? `<i class="st-gi">${gameIcon(ensureGame(np.game_id, np.name))}</i>` : '';
    btn.querySelector('span').innerHTML = stIcon + esc(txt); btn.dataset.tip = (detail ? '' : game && txt.startsWith(game) ? 'Playing ' : 'Status: ') + tipTxt + (auto ? ' (no keyboard or mouse input for 10 minutes)' : '');
    btn.dataset.st = shownStatus();
    const me = realMode() && LIVE.friends.me ? LIVE.friends.me : ME.name;
    // nickname shown, username (permanent) in the hover; click either to change the nickname
    const nick = (realMode() ? LIVE.meNick : ME.nick) || me;
    $('#username').textContent = nick + statusLabel();
    for (const s of ['#username', '#px-user']) { const m = $(s); if (m) { m.dataset.tip = `@${me} · click to change your nickname`; m.classList.add('nick-edit'); } }
    if (realMode() && FLB.accountInfo && LIVE.meAvFor !== me) { LIVE.meAvFor = me; FLB.accountInfo().then(a => { LIVE.meAv = a && a.avatar; LIVE.meNick = a && a.display_name || ''; syncStatus(); }).catch(() => {}); }
    const meAv = avatarHtml({ handle: me, avatar: realMode() ? LIVE.meAv : ME.avatar }, 16, 'me-av-i');
    for (const s of ['#me-av', '#px-av']) { const m = $(s); if (m) m.innerHTML = meAv; }
    if ($('#px-user')) $('#px-user').textContent = nick;
    btn.dataset.user = nick; // dark skin: keeps the username's room in the status row
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
  // Status menu (native): Online, Away, Invisible, Looking to Play, ─, Set Custom Status..., Reset Status
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
      { sep: true },
      { id: 'custom', label: 'Set Custom Status…' },
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

  /* Text size (Options > Appearance): Small / Normal (default) / Large / Larger scales the whole dark window. */
  const TEXT_ZOOM = { small: 1, normal: 1.15, large: 1.3, larger: 1.45 };
  function applyTextSize(v) {
    const z = TEXT_ZOOM[v] || TEXT_ZOOM.normal;
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
    if (r && TAURI && e.button === 0) { e.preventDefault(); e.stopPropagation(); FLB.resize(r.dataset.resize); }
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
  // Auto-Login needs a saved session: ticking it ticks Save Password; unticking Save Password unticks Auto-Login.
  document.addEventListener('change', e => {
    const t = e.target; if (!t || !t.id) return;
    if (t.id === 'lg-auto' && t.checked && $('#lg-save')) $('#lg-save').checked = true;
    if (t.id === 'lg-save' && !t.checked && $('#lg-auto')) $('#lg-auto').checked = false;
  });
  $('#zoom').addEventListener('change', e => setZoom(e.target.value));
  window.addEventListener('resize', layout);
  function clock() {
    $('#clock').textContent = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    const s = $('#session'); if (s) s.textContent = fmtT(liveSeconds() * 1000);
    $$('.sf-timer').forEach(t => { t.textContent = fmtT(Date.now() - +t.dataset.start); });
    $$('.sf-sess').forEach(t => { const v = fmtSess(Date.now() - +t.dataset.start); if (t.textContent !== v) t.textContent = v; });
  }
  setInterval(clock, 1000);
  document.addEventListener('input', e => { if (!state.addFriend) return; if (e.target.id === 'add-handle') { state.addFriend.q = e.target.value; $('#add-search').disabled = !e.target.value.trim(); } else if (e.target.id === 'add-msg-text') state.addFriend.msg = e.target.value; });
  document.addEventListener('keydown', e => { if (e.key !== 'Enter') return; if (e.target.id === 'lg-pass') actions['login-go'](); else if (e.target.id === 'lg-code') actions['mfa-go'](); else if (e.target.id === 'lg-npass2' || e.target.id === 'lg-email') actions['signup-go'](); });
  setInterval(() => { const t = $('#lg-tmo'); if (t) t.textContent = `Time out in ${Math.max(0, Math.round(+t.dataset.exp - Date.now() / 1000))} secs`; }, 1000);

  /* ---------------- boot ---------------- */
  (async () => {
    let st = null;
    try { st = await FLB.settings(); LIVE.settings = st; } catch (e) {}
    const saved = st && st.settings || {};
    if (['online', 'away', 'invisible'].includes(saved.status)) myStatus = saved.status;
    applyTheme(saved.theme); syncMenus(); syncStatus();
    for (const k of jsonList('collapsed_groups')) state.collapsed[k] = true;
    setZoom(params.get('zoom') || (TAURI ? (saved.zoom || '1.5') : (document.body.classList.contains('compare') ? 1 : 2)));
    applySkin();
    clock();
    { const sw = TAURI ? +saved.side_w : +(params.get('sidew') || localStorage.getItem('fl-side-w')); if (sw) setSideW(sw, false); }
    applyTextSize(saved.text_size || params.get('text') || 'normal');
    if (params.get('side') === '1') toggleSide(true);
    if (params.get('sel')) { state.sel = params.get('sel'); render(); }
    if (params.get('add')) setTimeout(async () => { // preview: ?add=1, or ?add=<query>[&addsel=<row>]
      actions['add-friend'](); const q = params.get('add'); if (q === '1') return;
      state.addFriend.q = q; await actions['add-friend-go']();
      const i = +params.get('addsel'), u = state.addFriend && state.addFriend.users; if (u && u[i - 1]) actions['add-pick']({ dataset: { handle: u[i - 1].handle } });
    }, 300);
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
    whatsNew();
    loadCatalog();
    FLB.on('fraglink://events', () => { for (const k in ICONS) if (ICONS[k] === null && !k.startsWith('f:')) delete ICONS[k]; refreshSoon(); }); // a newly detected game may now have an exe icon
    FLB.on('fraglink://friends', refreshFriends);
    FLB.on('fraglink://sync', refreshFriends);
    FLB.on('fraglink://logoff', () => actions['sign-out']());
    // Auto-update: Rust downloads + verifies quietly (startup + every 6 h); then a green arrow
    // in the title bar installs and restarts in one click. No prompt.
    const askUpdate = v => { const b = $('#upd-ready'); if (!b || !v) return;
      b.dataset.tip = 'Update ready - click to restart'; b.dataset.version = devVer(v); b.hidden = false; document.body.classList.add('has-upd'); };
    FLB.on('fraglink://update-found', v => { const m = $('#upd-msg'); if (m) m.innerHTML = `Downloading Fraglink ${esc(v)}…<br><small>Fraglink restarts when it's done.</small>`; });
    FLB.on('fraglink://update-progress', p => { const m = $('#upd-msg small'); if (m && p != null && m.parentNode.textContent.startsWith('Downloading')) m.textContent = `${p}% · Fraglink restarts when it's done.`; });
    // Automatic Away after 10 minutes without input (Rust watches; like Discord).
    FLB.on('fraglink://auto-away', v => { autoAway = !!v; syncStatus(); });
    if (FLB.autoAway) FLB.autoAway().then(v => { autoAway = !!v; syncStatus(); }).catch(() => {});
    FLB.on('fraglink://update', askUpdate);
    if (FLB.updateAvailable) FLB.updateAvailable().then(askUpdate).catch(() => {});
    FLB.on('fraglink://catalog', loadCatalog);
    // "Track <name> as a game?": an unknown program was full screen for 5 minutes
    if (FLB.trackPrompt) { FLB.on('fraglink://track-prompt', showTrackAsk); FLB.trackPrompt().then(showTrackAsk).catch(() => {}); }
    FLB.on('fraglink://show-friend', h => { state.tab = 'friends'; state.sel = 'u:' + h; refreshFriends(); });
    setInterval(() => { if (realMode() && state.tab === 'friends') render(); refreshActivity(); }, 60000);
    FLB.on('fraglink://status', v => { myStatus = v; syncStatus(); });
    setInterval(refresh, 30000);
    if (!TAURI && params.get('toast') === '1') { const f = FRIENDS.find(x => x.id === 'rift'); Object.assign(f, { kind: 'playing', game: 'bf2', server: SERVERS[1], info: 'Karkand · 32/64' }); render(); toast(f, 'bf2'); }
    if (params.get('upd') === '1') askUpdate('0.2.0-10');
    if (params.get('check') === '1') actions['check-updates']();
    if (params.get('modal') === 'options') openOptions();
    if (params.get('modal') === 'about') actions.about();
    if (params.get('menu') && params.get('menu') !== 'status') setTimeout(() => { const m = $(`.xf-menu[data-menu="${params.get('menu')}"]`); if (m) { m.classList.add('open'); const sub = params.get('sub') && m.querySelectorAll('.has-sub')[+params.get('sub')]; if (sub) sub.classList.add('force'); } }, 300); // preview
    if (params.get('menu') === 'status') setTimeout(() => statusMenu($('#status-btn')), 300);
    if (params.get('modal') === 'games') openGames(true);
  })();
  if (params.get('compare') === '1') document.body.classList.add('compare');
})();
