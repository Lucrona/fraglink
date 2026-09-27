/* Fraglink backend bridge.
   In the Tauri app this calls the Rust core via window.__TAURI__ (withGlobalTauri).
   Opened as a plain file in a browser it falls back to a small in-memory mock so the
   UI can still be previewed and screenshotted without the app. No network calls. */
(() => {
  'use strict';
  const T = window.__TAURI__;
  if (T && T.core) {
    const inv = (cmd, args) => T.core.invoke(cmd, args);
    window.FL = {
      tauri: true,
      glance: () => inv('get_glance'),
      activity: (limit = 50) => inv('get_activity', { limit }),
      games: () => inv('get_games'),
      candidates: () => inv('get_candidates'),
      tag: (exe, name) => inv('tag_process', { exe, name }),
      untag: (exe) => inv('untag_process', { exe }),
      exclude: (gameId, excluded) => inv('set_game_excluded', { gameId, excluded }),
      settings: () => inv('get_settings'),
      set: (key, value) => inv('set_setting', { key, value: String(value) }),
      autostart: (enabled) => inv('set_autostart', { enabled }),
      exportData: () => inv('export_data'),
      openData: () => inv('open_data_dir'),
      hide: () => inv('hide_main'),
      quit: () => inv('quit_app'),
      scale: (zoom, sideOpen) => inv('set_window_scale', { zoom: Number(zoom), sideOpen: !!sideOpen }),
      syncStatus: () => inv('sync_status'),
      syncSignIn: (server) => inv('sync_sign_in', { server }),
      syncCancel: () => inv('sync_cancel'),
      syncNow: () => inv('sync_now'),
      syncSignOut: () => inv('sync_sign_out'),
      friends: () => inv('friends_state'),
      friendsRefresh: () => inv('friends_refresh'),
      friendAdd: (handle) => inv('friend_add', { handle }),
      friendRespond: (handle, accept) => inv('friend_respond', { handle, accept }),
      friendRemove: (handle) => inv('friend_remove', { handle }),
      friendBlock: (handle, block) => inv('friend_block', { handle, block }),
      friendMute: (handle, mute) => inv('friend_mute', { handle, mute }),
      serverStats: () => inv('get_server_stats'),
      gameIcons: (ids) => inv('game_icons', { ids }),
      on: (evt, cb) => T.event.listen(evt, e => cb(e.payload)),
      drag: () => T.window.getCurrentWindow().startDragging(),
    };
    return;
  }

  /* ---------------- browser preview mock ---------------- */
  const now = () => Math.floor(Date.now() / 1000);
  const day = (off = 0) => { const d = new Date(Date.now() - off * 864e5); return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'); };
  const t0 = now() - (47 * 60 + 12);
  const games = [
    { id: 'wow', name: 'World of Warcraft', source: 'signature', steam_appid: null, excluded: false, total_seconds: 312 * 3600, sessions: 140, last_played: now() },
    { id: 'hl2', name: 'Half-Life 2', source: 'signature', steam_appid: 220, excluded: false, total_seconds: 41 * 3600, sessions: 30, last_played: now() - 86400 },
    { id: 'cs2', name: 'Counter-Strike 2', source: 'signature', steam_appid: 730, excluded: false, total_seconds: 96 * 3600, sessions: 70, last_played: now() - 2 * 86400 },
    { id: 'steam:1145350', name: 'Hades II', source: 'steam', steam_appid: 1145350, excluded: false, total_seconds: 12 * 3600, sessions: 9, last_played: now() - 3 * 86400 },
    { id: 'wc3', name: 'Warcraft III', source: 'signature', steam_appid: null, excluded: false, total_seconds: 544 * 3600, sessions: 300, last_played: now() - 20 * 86400 },
  ];
  const seven = { wow: 6.8, hl2: 3.1, cs2: 2.2, 'steam:1145350': 1.4, wc3: 0 };
  const activity = [
    { id: 6, ts: t0, kind: 'started', game_id: 'wow', game_name: 'World of Warcraft', payload: {} },
    { id: 5, ts: now() - 86400 - 3600, kind: 'milestone', game_id: 'wow', game_name: 'World of Warcraft', payload: { hours: 250 } },
    { id: 4, ts: now() - 86400 - 7200, kind: 'stopped', game_id: 'hl2', game_name: 'Half-Life 2', payload: { active_seconds: 5400 } },
    { id: 3, ts: now() - 3 * 86400, kind: 'first_time', game_id: 'steam:1145350', game_name: 'Hades II', payload: {} },
    { id: 2, ts: now() - 3 * 86400, kind: 'tagged', game_id: 'manual:myindie', game_name: 'My Indie', payload: { exe_name: 'MyIndie.exe' } },
    { id: 1, ts: now() - 12 * 86400, kind: 'stopped', game_id: 'cs2', game_name: 'Counter-Strike 2', payload: { active_seconds: 7300 } },
  ];
  const settings = { skin: localStorage.getItem('fl-skin') || 'classic', zoom: '2', toasts: '1', idle_threshold_min: '15', min_session_min: '2', status: 'online' };
  let paused = false;
  const ok = v => Promise.resolve(v);
  window.FL = {
    tauri: false,
    glance: () => {
      const active = now() - t0;
      const top = games.filter(g => !g.excluded).map(g => ({ game_id: g.id, name: g.name, seven_day_seconds: Math.round((seven[g.id] || 0) * 3600) + (g.id === 'wow' ? active : 0), total_seconds: g.total_seconds + (g.id === 'wow' ? active : 0), tier: 'tracked', last_played: g.last_played }))
        .sort((a, b) => b.seven_day_seconds - a.seven_day_seconds || b.total_seconds - a.total_seconds).slice(0, 5);
      return ok({
        now_playing: paused ? null : { game_id: 'wow', name: 'World of Warcraft', session_id: 6, started_at: t0, active_seconds: active, idle_trimmed_seconds: 0, idle: false, source: 'signature' },
        paused, today: day(), today_seconds: 5760 + active, today_by_game: [], week_seconds: 13.5 * 3600 + active, last_week_seconds: 11.4 * 3600,
        seven_day_seconds: 13.5 * 3600 + active, last_7_days: [6, 5, 4, 3, 2, 1, 0].map(i => ({ day: day(i), seconds: i === 0 ? 5760 + active : (i % 3) * 3600 })),
        streak_days: 6, total_seconds: games.reduce((a, g) => a + g.total_seconds, 0), top, games_tracked: games.length,
      });
    },
    activity: () => ok(activity),
    games: () => ok(games.map(g => ({ ...g }))),
    candidates: () => ok([{ exe: 'MyIndie.exe', path: 'C:\\Games\\MyIndie\\MyIndie.exe' }, { exe: 'Obscure2004.exe', path: 'D:\\Old\\Obscure2004.exe' }]),
    tag: (exe, name) => { games.push({ id: 'manual:' + name.toLowerCase().replace(/[^a-z0-9]+/g, '-'), name, source: 'manual', excluded: false, total_seconds: 0, sessions: 0 }); return ok('manual:x'); },
    untag: () => ok(),
    exclude: (id, ex) => { const g = games.find(x => x.id === id); if (g) g.excluded = ex; return ok(); },
    settings: () => ok({ settings, autostart: false, sim: false, data_dir: '(browser preview: no data is stored)', log_dir: '', version: 'preview', signatures: 170, signatures_version: 'preview', steam_apps: 0, idle_threshold_min: 15, min_session_min: 2 }),
    set: (k, v) => { settings[k] = String(v); if (k === 'paused') paused = v === '1' || v === true; return ok(); },
    autostart: (e) => ok(e),
    exportData: () => ok(['(preview) fraglink-export-20260926-200000.zip']),
    openData: () => ok(),
    hide: () => ok(),
    quit: () => ok(),
    scale: () => ok(),
    syncStatus: () => ok({ ...sync, pending: 3 }),
    syncSignIn: (server) => { sync.server = server; sync.pairing = { user_code: 'KXRT-7QPM', verification_uri: server + '/pair', verification_uri_complete: server + '/pair?code=KXRT-7QPM', expires_at: now() + 600 }; return ok(); },
    syncCancel: () => { sync.pairing = null; return ok(); },
    syncNow: () => ok(),
    syncSignOut: () => { sync.signed_in = false; sync.user = null; return ok(); },
    friends: () => ok(sync.signed_in ? { signed_in: true, connected: true, me: sync.user, list: mockFriends, muted } : { signed_in: false, connected: false, me: null, list: null, muted: [] }),
    friendsRefresh: () => ok(),
    friendAdd: (h) => { mockFriends.outgoing.push({ handle: h, display_name: h, created_at: new Date().toISOString() }); return ok('pending'); },
    friendRespond: (h, a) => { const r = mockFriends.incoming.find(x => x.handle === h); mockFriends.incoming = mockFriends.incoming.filter(x => x !== r); if (a && r) mockFriends.friends.push({ handle: h, display_name: r.display_name, since: new Date().toISOString(), presence: { status: 'online' } }); return ok(); },
    friendRemove: (h) => { mockFriends.friends = mockFriends.friends.filter(f => f.handle !== h); mockFriends.outgoing = mockFriends.outgoing.filter(f => f.handle !== h); return ok(); },
    friendBlock: (h, b) => { if (b) { mockFriends.friends = mockFriends.friends.filter(f => f.handle !== h); mockFriends.blocked.push(h); } else mockFriends.blocked = mockFriends.blocked.filter(x => x !== h); return ok(); },
    friendMute: (h, m) => { const i = muted.indexOf(h.toLowerCase()); if (m && i < 0) muted.push(h.toLowerCase()); if (!m && i >= 0) muted.splice(i, 1); return ok(); },
    serverStats: () => ok(sync.signed_in ? { fetched_at: now(), games: [
      { game_id: 'wc3', name: 'Warcraft III', tracked_min: 0, imported_min: 544 * 60, estimated_min: 0, display_min: 544 * 60, display_tier: 'imported' },
      { game_id: 'wow', name: 'World of Warcraft', tracked_min: 312 * 60, imported_min: 0, estimated_min: 0, display_min: 312 * 60, display_tier: 'tracked' },
      { game_id: 'steam:730', name: 'Counter-Strike 2', tracked_min: 118 * 60, imported_min: 0, estimated_min: 0, display_min: 118 * 60, display_tier: 'tracked' },
      { game_id: 'manual:ut99', name: 'Unreal Tournament', tracked_min: 0, imported_min: 0, estimated_min: 1500 * 60, display_min: 0, display_tier: 'estimated' },
    ], overall: { verified_min: 974 * 60, estimated_min: 1500 * 60, estimated_display: '~1,500h' } } : null),
    // preview: stand-in icons drawn on a canvas (no real game art is bundled)
    gameIcons: (ids) => ok(Object.fromEntries(ids.map(id => [id, /^(wc3|manual:)/.test(id) ? null : placeholderIcon(id)]))),
    on: () => ok(() => {}),
    drag: () => ok(),
  };
  function placeholderIcon(id) {
    const c = document.createElement('canvas'); c.width = c.height = 32; const x = c.getContext('2d');
    let h = 0; for (const ch of id) h = (h * 31 + ch.codePointAt(0)) >>> 0;
    const g = x.createLinearGradient(0, 0, 32, 32); g.addColorStop(0, `hsl(${h % 360} 65% 60%)`); g.addColorStop(1, `hsl(${(h + 40) % 360} 70% 25%)`);
    x.fillStyle = '#14141c'; x.fillRect(0, 0, 32, 32); x.fillStyle = g; x.fillRect(1, 1, 30, 30);
    x.fillStyle = 'rgba(255,255,255,.3)'; x.fillRect(1, 1, 30, 12);
    return c.toDataURL('image/png');
  }
  const iso = (s) => new Date((now() - s) * 1000).toISOString();
  const muted = [];
  const mockFriends = {
    friends: [
      { handle: 'nova', display_name: 'Nova', since: iso(90 * 86400), presence: { status: 'online', game_id: 'steam:570', game_name: 'Dota 2', session_started_at: iso(38 * 60) } },
      { handle: 'grimm', display_name: 'Grimm', since: iso(40 * 86400), presence: { status: 'away', game_id: null, game_name: null } },
      { handle: 'kestrel', display_name: 'Kestrel', since: iso(12 * 86400), presence: { status: 'online', game_id: null, game_name: null } },
      { handle: 'pixelmoth', display_name: 'PixelMoth', since: iso(5 * 86400), presence: { status: 'offline', game_id: null, game_name: null } },
    ],
    incoming: [{ handle: 'oldmate', display_name: 'Old Mate', created_at: iso(3600) }],
    outgoing: [{ handle: 'zed', display_name: 'Zed', created_at: iso(7200) }],
    blocked: [],
  };
  const sync = { signed_in: /[?&]signed=1/.test(location.search), user: /[?&]signed=1/.test(location.search) ? 'Lucrona' : null, server: 'http://localhost:8787', last_ok: null, last_error: null, pairing: null, pairing_error: null };
})();
