/* Fraglink tray client UI (from the classic client prototype).
   Stats and Activity show real local data from the Rust core via window.FL (backend.js).
   Friends are still sample data until M3. No network calls. */
(() => {
  'use strict';
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const params = new URLSearchParams(location.search);
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const FLB = window.FL; const TAURI = !!(FLB && FLB.tauri);
  document.body.classList.add('app-mode'); if (TAURI) document.body.classList.add('tauri');
  // live local data from the tracker
  const LIVE = { glance: null, activity: [], fetchedAt: 0, settings: null };


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
    if (GAMES[id] && (!name || GAMES[id].name === name || GAMES[id].real)) return id;
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
  function gameIcon(id) {
    const g = GAMES[id]; if (!g) return '';
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

  /* ---------------- state ---------------- */
  const state = {
    skin: params.get('skin') || localStorage.getItem('fl-skin') || 'classic',
    tab: ({ hours: 'stats', profile: 'stats' })[params.get('tab')] || params.get('tab') || 'friends',
    collapsed: { online: false, fof: false, off: false },
    sel: null, sort: { key: 'name', dir: 1 }, hoursSort: { key: 'week', dir: -1 },
    chatWith: null, logs: {},
  };
  if (params.get('off') === '1') state.collapsed.off = false;

  /* ---------------- skins ---------------- */
  function applySkin(s) {
    if (!['classic', 'phoenix', 'stream'].includes(s)) s = 'classic';
    state.skin = s;
    document.body.classList.remove('skin-classic', 'skin-phoenix', 'skin-stream');
    document.body.classList.add('skin-' + s);
    try { localStorage.setItem('fl-skin', s); } catch (e) {}
    if (TAURI) FLB.set('skin', s).catch(() => {});
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
    if (f.kind === 'playing' || f.kind === 'fof') return GAMES[f.game].name;
    if (f.kind === 'online') return f.status;
    return 'Offline';
  }
  function row(f, cls = '') {
    const icon = f.game && f.kind !== 'off' ? `<i class="gi">${gameIcon(f.game)}</i>` : '<i class="noicon"></i>';
    const info = f.kind === 'playing' || f.kind === 'fof' ? f.server : f.kind === 'off' ? 'Last online ' + f.last : '';
    return `<div class="xf-row ${cls} ${state.sel === f.id ? 'sel' : ''}" data-friend="${f.id}" title="${esc(f.name)}"><span class="c-name">${icon}${esc(f.name)}</span><span class="c-status">${esc(statusText(f))}</span><span class="c-info">${esc(info)}</span></div>`;
  }
  function sec(key, label, cls) {
    const c = state.collapsed[key];
    return `<div class="xf-sec ${cls} ${c ? 'collapsed' : ''}" data-sec="${key}"><i class="tri"></i>${label}</div>`;
  }
  function sorted(arr) {
    const { key, dir } = state.sort;
    return [...arr].sort((a, b) => (key === 'name' ? a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) : statusText(a).localeCompare(statusText(b))) * dir);
  }
  function renderFriends() {
    const playing = sorted(FRIENDS.filter(f => f.kind === 'playing'));
    const online = sorted(FRIENDS.filter(f => f.kind === 'online'));
    const fof = sorted(FRIENDS.filter(f => f.kind === 'fof'));
    const off = sorted(FRIENDS.filter(f => f.kind === 'off'));
    const s = state.sort;
    let h = `<div class="xf-grid"><div class="xf-head"><span class="c-name sort ${s.key === 'name' && s.dir < 0 ? 'desc' : ''}" data-sort="name">Name</span><span class="c-status" data-sort="status">Status</span><span class="c-info">Game Info</span></div>`;
    h += `<div class="fl-mock">Sample friends only. Real friends arrive in a later update.</div>`;
    h += playing.map(f => row(f)).join('') + online.map(f => row(f)).join('');
    h += sec('fof', 'Friends of&nbsp;<u>Friends Playing</u>', 's-fof');
    if (!state.collapsed.fof) h += fof.map(f => row(f, 'fof')).join('');
    h += sec('off', `Friends Offline`, 's-off');
    if (!state.collapsed.off) h += off.map(f => row(f, 'off')).join('');
    h += '</div>';
    list.innerHTML = h;
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
    if (g.paused) h += `<div class="xf-row st-row fl-muted"><span class="c-name st-g"><i class="noicon"></i><span class="nm">Tracking paused</span></span><span class="st-live"><a data-action="pause-toggle">resume</a></span></div>`;
    else if (np) { const id = ensureGame(np.game_id, np.name); h += `<div class="xf-row st-row" data-game="${esc(id)}" title="${esc(np.name)} · detected via ${esc(String(np.source).replace('_', ' '))}"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(np.name)}</span></span><span class="st-live timer" id="session">${fmtT(liveSeconds() * 1000)}</span></div>`; if (np.idle) h += `<div class="fl-note">Idle: time without input is not counted</div>`; }
    else h += `<div class="xf-row st-row fl-muted"><span class="c-name st-g"><i class="noicon"></i><span class="nm">Not in a game · watching</span></span><span class="st-live"></span></div>`;
    const change = g.last_week_seconds > 0 ? Math.round((g.week_seconds - g.last_week_seconds) / g.last_week_seconds * 100) : null;
    const chg = change === null ? '' : `<em class="${change < 0 ? 'down' : ''}" title="vs all of last week">${change < 0 ? '&#9660;' : '&#9650;'}${Math.abs(change)}%</em>`;
    h += `<div class="st-sum"><div><small>Today</small><b>${fmtHrs(g.today_seconds)} h</b></div><div><small>This week</small><b>${fmtHrs(g.week_seconds)} h</b>${chg}</div><div><small>Streak</small><b>${g.streak_days} day${g.streak_days === 1 ? '' : 's'}</b></div></div>`;
    h += `<div class="xf-sec s-fof"><i class="tri"></i>Top Games This Week</div>`;
    const top = g.top || []; const max = Math.max(1, ...top.map(r => r.seven_day_seconds));
    if (!top.length) h += `<div class="fl-empty">No games tracked yet. Start a game, or use <a data-action="tag-game">Fraglink → This is a game…</a></div>`;
    h += top.map(r => { const id = ensureGame(r.game_id, r.name); return `<div class="xf-row st-row" data-game="${esc(id)}" title="${esc(r.name)} · ${fmtDur(r.total_seconds)} tracked on this PC"><span class="c-name st-g"><i class="gi">${gameIcon(id)}</i><span class="nm">${esc(r.name.replace(/:.*/, ''))}</span></span><span class="st-w"><i class="st-bar"><b style="width:${Math.round(r.seven_day_seconds / max * 100)}%"></b></i>${hrs(r.seven_day_seconds).toFixed(1)}</span><span class="st-t">${fmtHrs(r.total_seconds)}h${tierMark(r.tier || 'tracked')}</span></div>`; }).join('');
    h += `<div class="st-legend">${tierMark('tracked')}Tracked on this PC · ${g.games_tracked} game${g.games_tracked === 1 ? '' : 's'} · ${fmtHrs(g.total_seconds)} h total</div>`;
    h += `<div class="st-link"><a data-action="tag-game">&raquo; This is a game…</a> &nbsp; <a data-action="my-stats">&raquo; Full stats</a></div></div>`;
    list.innerHTML = h;
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
      const kindTile = a.kind === 'milestone' ? '_ach' : a.kind === 'tagged' ? '_new' : ensureGame(a.game_id, a.game_name);
      h += `<div class="av-row" title="${esc(new Date(a.ts * 1000).toLocaleString())}"><i class="gi">${gameIcon(kindTile)}</i><div class="av-tx"><b class="me">You</b> ${activityHtml(a)}</div><span class="av-t">${esc(whenLabel(a.ts))}</span></div>`;
    }
    h += `<div class="st-link"><a data-action="website">&raquo; All activity on fraglink.gg (later)</a></div></div>`;
    list.innerHTML = h;
  }
  function render() {
    $$('.xf-tab, .xf-icontabs [data-tab]').forEach(t => t.classList.toggle('active', t.dataset.tab === state.tab));
    $('#list').classList.toggle('nohbar', state.tab !== 'friends');
    const sl = list.scrollLeft, st = list.scrollTop;
    if (state.tab === 'stats') renderStats(); else if (state.tab === 'activity') renderActivity(); else renderFriends();
    list.scrollLeft = sl; list.scrollTop = st; LS.update();
    renderSide();
  }

  /* ---------------- side panel ---------------- */
  function renderSide() {
    const el = SS.vp; requestAnimationFrame(SS.update);
    const f = FRIENDS.find(x => x.id === state.sel);
    if (f) {
      const playing = f.kind === 'playing' || f.kind === 'fof';
      el.innerHTML = `<h4>${esc(f.name)}</h4>
        <div class="act">${playing ? `<i class="gi">${gameIcon(f.game)}</i><div><b>${esc(GAMES[f.game].name)}</b><br>${esc(f.server)}<br><span style="color:var(--cyan)">${esc(f.info)}</span></div>` : `<div>${esc(statusText(f))}${f.last ? ' · last online ' + esc(f.last) : ''}</div>`}</div>
        ${playing ? '<a data-action="join">Join Game</a> · ' : ''}<a data-action="chat">Send Message</a> · <a data-action="friend-profile">Profile</a>
        <div class="sep"></div>
        <div><span style="color:#8ea3c8">Past 7 days:</span> <b>${fmtH(f.week)}</b><br><span style="color:#8ea3c8">Total logged:</span> <b>${f.total ? f.total + ' h' : 'hidden'}</b></div>
        <div class="sep"></div><a data-action="clear-sel">« Activity Report</a>`;
      return;
    }
    const act = FRIENDS.filter(x => x.kind === 'playing').map(x => `<div class="act"><i class="gi">${gameIcon(x.game)}</i><div><b>${esc(x.name)}</b> is playing <span style="color:var(--cyan)">${esc(GAMES[x.game].name)}</span></div></div>`).join('');
    el.innerHTML = `<h4>Activity Report</h4>${act}
      <div class="sep"></div><h4>My Games</h4>
      <ul class="games">${((LIVE.glance && LIVE.glance.top) || []).map(r => { const id = ensureGame(r.game_id, r.name); return `<li><i class="gi">${gameIcon(id)}</i><span>${esc(r.name.replace(/:.*/, ''))}</span><em>${fmtHrs(r.total_seconds)}h</em></li>`; }).join('') || '<li><span>No games tracked yet</span></li>'}</ul>`;
  }
  function toggleSide(force) {
    const app = $('#app');
    const open = force ?? !app.classList.contains('side-open');
    app.classList.toggle('side-open', open); requestAnimationFrame(SS.update);
    if (TAURI) FLB.scale(currentZoom(), open);
    $('#nub').title = open ? 'Hide Activity Report' : 'Show Activity Report';
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
      <div class="who"><b>${esc(f.name)}</b>${playing ? `<div class="playing">is playing ${gameIcon(f.game)}<span>${esc(GAMES[f.game].name)}</span></div><div class="srv">${esc(f.server)} · ${esc(f.info)}</div>` : `<div class="playing" style="color:${f.kind === 'off' ? '#bdbfbf' : '#fff'}">${esc(statusText(f))}</div><div class="srv">${f.last ? 'Last online ' + esc(f.last) : 'Not in game'}</div>`}</div>
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
    if (!$('#opt-toast').checked) return;
    const t = $('#toast'); toastFriend = f.id;
    $('#toast-body').innerHTML = `<i class="gi">${gameIcon(game)}</i><div><b>${esc(f.name)}</b> has started playing<br><span class="g">${esc(GAMES[game].name)}</span></div>`;
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
    return `<div class="wp-top"><div class="wp-logo"><img class="wp-wordmark" src="brand/fraglink-wordmark.svg" alt="Fraglink"><small>Gaming, tracked.</small></div>
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
  function notice(title, html) {
    $('#notice-title').textContent = title; $('#notice-body').innerHTML = html; $('#notice').hidden = false;
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
      $('#menu-pause').innerHTML = g.paused ? '<u>R</u>esume tracking' : '<u>P</u>ause tracking';
      if (state.tab !== 'friends') render(); else renderSide();
    } catch (e) { console.error('refresh failed', e); }
  }
  function refreshSoon() { clearTimeout(refreshTimer); refreshTimer = setTimeout(refresh, 250); }

  /* ---- options dialog ---- */
  async function openOptions() {
    $('#options').hidden = false;
    try {
      const st = await FLB.settings(); LIVE.settings = st;
      $('#opt-idle').value = String(st.idle_threshold_min); $('#opt-min').value = String(st.min_session_min);
      $('#opt-pause').checked = st.settings.paused === '1'; $('#opt-toast').checked = st.settings.toasts !== '0';
      $('#opt-autostart').checked = !!st.autostart;
      $('#opt-info').innerHTML = `Saved only on this PC:<br><code>${esc(st.data_dir)}</code><br>Fraglink ${esc(st.version)} · ${st.signatures} known games · ${st.steam_apps} Steam, ${st.epic_apps || 0} Epic, ${st.gog_apps || 0} GOG games found${st.sim ? '<br><b class="bad">Simulation mode: fake games</b>' : ''}`;
    } catch (e) { toastErr(e); }
  }
  $('#opt-idle').addEventListener('change', e => FLB.set('idle_threshold_min', e.target.value).catch(toastErr));
  $('#opt-min').addEventListener('change', e => FLB.set('min_session_min', e.target.value).catch(toastErr));
  $('#opt-pause').addEventListener('change', e => FLB.set('paused', e.target.checked ? '1' : '0').then(refreshSoon).catch(toastErr));
  $('#opt-toast').addEventListener('change', e => FLB.set('toasts', e.target.checked ? '1' : '0').catch(toastErr));
  $('#opt-autostart').addEventListener('change', e => FLB.autostart(e.target.checked).then(v => { e.target.checked = !!v; }).catch(err => { e.target.checked = !e.target.checked; toastErr(err); }));

  /* ---- games dialog: "this is a game" + exclusions ---- */
  let candSel = null;
  function prettyExe(exe) { return exe.replace(/\.exe$/i, '').replace(/[-_]+(win64|shipping|x64|dx11|dx12)\b/ig, '').replace(/[_-]+/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2').trim(); }
  async function openGames(focusTag) {
    $('#games-dlg').hidden = false; $('#games-title').textContent = focusTag ? 'This is a game…' : 'Manage games';
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
    'options': openOptions,
    'options-close': () => { $('#options').hidden = true; },
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
    'pause-toggle': async () => { try { await FLB.set('paused', LIVE.glance && LIVE.glance.paused ? '0' : '1'); refreshSoon(); } catch (e) { toastErr(e); } },
    'export': async () => { try { const p = await FLB.exportData(); notice('Export my data', `Saved:<br><code>${p.map(esc).join('</code><br><code>')}</code>`); } catch (e) { toastErr(e); } },
    'open-data': () => FLB.openData().catch(toastErr),
    'privacy': () => notice('What Fraglink reads', '<ul class="fl-ul"><li>The <b>list of running programs</b> (names and install paths) every 5 seconds, matched against a list of known games.</li><li>Steam\'s own <b>RunningAppID</b> value and its library files, the <b>Epic Games</b> install manifests and <b>GOG Galaxy</b>\'s install list, to recognise games you installed through them.</li><li>How long since you last <b>touched the mouse or keyboard</b> (a single Windows number, not what you typed), so idle time isn\'t counted.</li></ul>It never reads game memory, never injects anything into games and never hooks input, so it is safe with anti-cheat. Nothing leaves this PC in this version.'),
    'add-friend': () => notice('Add Friend', 'Friends arrive in a later update.'),
    'my-profile': () => notice('My Profile', LATER), 'friend-profile': () => notice('Profile', LATER),
    'my-stats': () => notice('Full stats', LATER), 'website': () => notice('fraglink.gg', LATER), 'estimate': () => notice('Estimate my history', LATER),
    'about': async () => { let v = ''; try { const st = await FLB.settings(); v = `${st.version}${st.sim ? ' (simulation mode)' : ''}`; } catch (e) {} notice('About Fraglink', `<b>Fraglink ${esc(v)}</b><br>Local hours tracker (milestone M1).<br>© 2026 Lucrona. All rights reserved.<br><br>Classic look inspired by 2005-era gaming IM clients; original Fraglink mark. Fonts: Russo One (SIL OFL 1.1), Wine Tahoma clone (LGPL).`); },
    'chat': () => openChat(state.sel),
    'join': () => notice('Join Game', 'Joining friends arrives with friends in a later update.'),
    'clear-sel': () => { state.sel = null; render(); },
    'download': () => notice('Download', 'You are already running Fraglink.'),
  };
  document.addEventListener('click', e => {
    const skin = e.target.closest('.skin-opt'); if (skin) { applySkin(skin.dataset.skin); closeMenus(); return; }
    const zo = e.target.closest('.zoom-opt'); if (zo) { setZoom(zo.dataset.zoom); closeMenus(); return; }
    const a = e.target.closest('[data-action]');
    if (a && !a.closest('#toast')) { closeMenus(); const fn = actions[a.dataset.action]; if (fn) { e.preventDefault(); fn(a); } return; }
    const t = e.target.closest('[data-tab]'); if (t) { state.tab = t.dataset.tab; render(); return; }
  });
  $$('input[name=skin]').forEach(r => r.addEventListener('change', () => applySkin(r.value)));
  list.addEventListener('click', e => {
    const s = e.target.closest('[data-sec]'); if (s) { state.collapsed[s.dataset.sec] = !state.collapsed[s.dataset.sec]; render(); return; }
    const so = e.target.closest('[data-sort]'); if (so) { const k = so.dataset.sort; state.sort = { key: k, dir: state.sort.key === k ? -state.sort.dir : 1 }; render(); return; }
    const hs = e.target.closest('[data-hsort]'); if (hs) { const k = hs.dataset.hsort; state.hoursSort = { key: k, dir: state.hoursSort.key === k ? -state.hoursSort.dir : -1 }; render(); return; }
    const w = e.target.closest('[data-web]'); if (w) { website(w.dataset.web); return; }
    const r = e.target.closest('[data-friend]'); if (r) { state.sel = r.dataset.friend; render(); }
  });
  list.addEventListener('dblclick', e => { const r = e.target.closest('[data-friend]'); if (r) openChat(r.dataset.friend); });
  // prototype: single click also opened chat; the app only selects (friends are sample data until M3)
  $('#list').addEventListener('keydown', e => {
    const rows = $$('[data-friend]', list); if (!rows.length) return;
    let i = rows.findIndex(r => r.dataset.friend === state.sel);
    if (e.key === 'ArrowDown') i = Math.min(rows.length - 1, i + 1); else if (e.key === 'ArrowUp') i = Math.max(0, i - 1);
    else if (e.key === 'Enter' && state.sel) { openChat(state.sel); return; } else return;
    e.preventDefault(); state.sel = rows[i].dataset.friend; render();
  });
  $('#status').addEventListener('change', e => {
    $('#username').textContent = ME.name + (e.target.value === 'online' ? '' : ' (' + e.target.selectedOptions[0].text + ')');
    FLB.set('status', e.target.value).catch(() => {});
  });

  /* drag: the app window is moved by the OS (Tauri); dialogs move inside it */
  const dragTargets = { app: '#app', chat: '#chat', options: '#options', web: '#web', 'games-dlg': '#games-dlg' };
  document.addEventListener('mousedown', e => {
    const h = e.target.closest('[data-drag]'); if (!h || e.target.closest('button,select,input,textarea,a')) return;
    if (TAURI && h.dataset.drag === 'app') { e.preventDefault(); FLB.drag(); return; }
    const win = $(dragTargets[h.dataset.drag]); if (!win) return;
    const zoomed = win !== $('#web'); const z = zoomed ? (parseFloat(getComputedStyle(win).zoom) || 1) : 1;
    const sx = e.clientX, sy = e.clientY, ox = win.offsetLeft, oy = win.offsetTop;
    const move = ev => { win.style.left = ox + (ev.clientX - sx) / z + 'px'; win.style.top = oy + (ev.clientY - sy) / z + 'px'; if (win.id === 'web') win.style.transform = 'none'; if (win.id === 'chat') chat.dataset.moved = 1; if (win.id === 'app') placeChat(); };
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

  /* ---------------- boot ---------------- */
  (async () => {
    let st = null;
    try { st = await FLB.settings(); LIVE.settings = st; } catch (e) {}
    const saved = st && st.settings || {};
    if (TAURI && saved.skin) state.skin = params.get('skin') || saved.skin;
    if (saved.status) { $('#status').value = saved.status; $('#status').dispatchEvent(new Event('change')); }
    setZoom(params.get('zoom') || (TAURI ? (saved.zoom || '1.5') : (document.body.classList.contains('compare') ? 1 : 2)));
    applySkin(state.skin);
    clock();
    if (params.get('side') === '1') toggleSide(true);
    if (params.get('sel')) { state.sel = params.get('sel'); render(); }
    await refresh();
    FLB.on('fraglink://events', refreshSoon);
    FLB.on('fraglink://paused', refreshSoon);
    FLB.on('fraglink://status', v => { $('#status').value = v; $('#username').textContent = ME.name + (v === 'online' ? '' : ' (' + $('#status').selectedOptions[0].text + ')'); });
    setInterval(refresh, 30000);
    if (!TAURI && params.get('toast') === '1') { const f = FRIENDS.find(x => x.id === 'rift'); Object.assign(f, { kind: 'playing', game: 'bf2', server: SERVERS[1], info: 'Karkand · 32/64' }); render(); toast(f, 'bf2'); }
    if (params.get('modal') === 'options') openOptions();
    if (params.get('modal') === 'games') openGames(true);
  })();
  if (params.get('compare') === '1') document.body.classList.add('compare');
})();
