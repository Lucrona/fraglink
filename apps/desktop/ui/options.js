/* Fraglink Options: a tabbed dialog. Changes are staged and written on OK;
   the Rust side broadcasts them so the main window follows. Account, blocked
   users and "Add game manually" act straight away. */
(() => {
  'use strict';
  const FLB = window.FL;
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const params = new URLSearchParams(location.search);
  const pending = new Map();
  let st = null, catalog = null, gsel = null, detecting = false;
  const fail = e => alert(String(e && e.message || e));
  const SOURCES = { steam: 'Steam', epic: 'Epic', gog: 'GOG', battlenet: 'Battle.net', ea: 'EA app', ubisoft: 'Ubisoft Connect', xbox: 'Xbox', windows: 'installed', seen: 'seen running', manual: 'added by you', tracked: 'played before', signature: '', detectable: '' };
  const PAGES = { notifications: 'general', advanced: 'general', tracking: 'connections' }; // old page names

  // Dark and Phoenix skins get the dark 2013 look; Classic and Stream keep the plain system look.
  const applyTheme = skin => document.body.classList.toggle('dark', skin === 'dark' || skin === 'phoenix');

  function show(page) {
    page = PAGES[page] || page;
    if (!$(`section[data-page="${page}"]`)) page = 'general';
    $$('.op-tabs button').forEach(b => b.classList.toggle('on', b.dataset.page === page));
    $$('section[data-page]').forEach(s => { s.hidden = s.dataset.page !== page; });
    if (page === 'account') renderSync();
    if (page === 'privacy') { renderBlocked(); renderShare(); }
    if (page === 'games' && !catalog && !detecting) detect();
  }

  const value = el => el.type === 'checkbox' ? (el.checked ? '1' : '0') : el.value;
  const on = (s, k, def = '1') => (s[k] === '0' || s[k] === '1') ? s[k] : def;
  function load() {
    const s = st.settings || {};
    const cur = {
      autostart: st.autostart ? '1' : '0', start_minimized: on(s, 'start_minimized', '0'), minimize_to_tray: on(s, 'minimize_to_tray', '0'),
      show_offline: on(s, 'show_offline'), show_fof: on(s, 'show_fof'), nickname: s.nickname || '', language: s.language || 'en-AU',
      toasts: on(s, 'toasts'), friend_toasts: on(s, 'friend_toasts'), friend_toasts_logon: on(s, 'friend_toasts_logon'), connection_alerts: on(s, 'connection_alerts', '0'), friend_toasts_dnd: on(s, 'friend_toasts_dnd'),
      share_game: on(s, 'share_game'), share_profile: on(s, 'share_profile'), share_fof: on(s, 'share_fof'),
      sound_signin: on(s, 'sound_signin', '0'), sound_signout: on(s, 'sound_signout', '0'), sound_pack: s.sound_pack || 'default',
      skin: ['dark', 'classic'].includes(s.skin) ? s.skin : 'dark', zoom: ['1', '1.5', '2'].includes(s.zoom) ? s.zoom : '1.5',
      theme: s.theme || 'blue', always_on_top: on(s, 'always_on_top', '0'),
      text_size: ['normal', 'large', 'larger'].includes(s.text_size) ? s.text_size : 'normal',
    };
    $$('[data-key]').forEach(el => {
      const v = cur[el.dataset.key] ?? '';
      if (el.type === 'checkbox') el.checked = v === '1';
      else if (el.type === 'radio') el.checked = el.value === v;
      else el.value = v;
    });
    scaleBox();
    $('#about').innerHTML = [
      ['Version', `Fraglink ${esc(st.version)}${st.sim ? ' <span class="bad">(simulation mode)</span>' : ''}`],
      ['Data folder', `<code>${esc(st.data_dir)}</code>`],
      ['Log folder', `<code>${esc(st.log_dir || '-')}</code>`],
      ['Known games', `${st.signatures} (list ${esc(st.signatures_version)})`],
      ['Found on this PC', `${st.steam_apps} Steam, ${st.epic_apps || 0} Epic, ${st.gog_apps || 0} GOG games`],
    ].map(([k, v]) => `<b>${k}</b><span>${v}</span>`).join('') + '<b></b><span><button type="button" data-do="open-data">Open data folder</button></span>';
  }
  function scaleBox() { const r = $('input[name=skin]:checked'); $('#scale-box').hidden = !r || r.value !== 'classic'; }

  document.addEventListener('change', e => {
    const t = e.target;
    if (t.id === 'share-activity') {
      FLB.setShareActivity(t.checked).then(() => { $('#share-msg').textContent = t.checked ? 'Friends can see your recent games and hours this week.' : 'Friends only see what you are playing right now.'; })
        .catch(err => { t.checked = !t.checked; $('#share-msg').textContent = String(err); });
      return;
    }
    const el = t.closest('[data-key]');
    $('#apply').disabled = false;
    if (el) { pending.set(el.dataset.key, value(el)); if (el.name === 'skin') scaleBox(); return; }
    if (!gsel) return;
    if (t.id === 'gd-path') pending.set('game_path:' + gsel, t.value.trim());
    else if (t.id === 'gd-args') pending.set('launch_args:' + gsel, t.value.trim());
    else if (t.name === 'gd-mode') { pending.set('detect_mode:' + gsel, t.value); $('#gd-path').placeholder = t.value === 'manual' ? 'Choose the game\'s .exe' : ''; }
    else if (t.id === 'gd-ex') { pending.set('exclude:' + gsel, t.checked ? '1' : '0'); renderTree(); }
  });

  async function apply() {
    for (const [k, v] of pending) {
      try {
        if (k === 'autostart') await FLB.autostart(v === '1');
        else if (k.startsWith('exclude:')) await FLB.exclude(k.slice(8), v === '1');
        else await FLB.set(k, v);
      } catch (e) { alert(`Could not save ${k}: ${e}`); return false; }
    }
    pending.clear(); $('#apply').disabled = true;
    return true;
  }
  const close = () => FLB.closeWindow ? FLB.closeWindow() : window.close();

  /* ---- Games tab ---- */
  const staged = (id, key, orig) => pending.has(key + id) ? pending.get(key + id) : orig;
  const isEx = g => staged(g.id, 'exclude:', g.excluded ? '1' : '0') === '1';
  let open = { inst: true, not: false };
  function renderTree() {
    const t = $('#tree');
    if (!catalog) { t.innerHTML = ''; return; }
    const inst = catalog.filter(g => g.installed), not = catalog.filter(g => !g.installed);
    const leaf = g => `<div class="leaf ${g.id === gsel ? 'sel' : ''} ${isEx(g) ? 'ex' : ''}" data-g="${esc(g.id)}" title="${esc(SOURCES[g.source] ?? g.source)}">${iconHtml(g.id)}<span>${esc(g.name)}</span></div>`;
    const node = (k, label, arr) => `<div class="node" data-node="${k}"><span class="tw">${open[k] ? '−' : '+'}</span>${label} (${arr.length})</div>` + (open[k] ? arr.map(leaf).join('') : '');
    t.innerHTML = node('inst', 'Installed games', inst) + node('not', 'Not installed', not);
  }
  // Same icon order as everywhere: server, Steam, exe, then the generic icon.
  const icons = {};
  const GENERIC = '<svg viewBox="0 0 16 16"><rect x="1" y="3" width="14" height="10" rx="3" fill="#3b4a66" stroke="#0b1424"/><circle cx="5" cy="8" r="1.6" fill="#cfd8e8"/><circle cx="11" cy="7" r="1" fill="#cfd8e8"/><circle cx="12" cy="9.4" r="1" fill="#cfd8e8"/></svg>';
  const iconHtml = id => `<i class="gi">${icons[id] ? `<img src="${icons[id]}" alt="">` : GENERIC}</i>`;
  async function loadIcons() {
    if (!catalog || !FLB.gameIcons) return;
    const ids = catalog.map(g => g.id).filter(id => !(id in icons));
    if (!ids.length) return;
    try { const m = await FLB.gameIcons(ids, false); for (const id of ids) icons[id] = m && m[id] || null; renderTree(); renderDetail(); } catch (e) { console.warn('icons', e); }
  }
  function renderDetail() {
    const g = catalog && catalog.find(x => x.id === gsel);
    $('#gd-name').textContent = 'Select Game';
    $('#gd-title').textContent = g ? g.name : 'Select a game on the left';
    $('#gd-src').textContent = g ? (g.installed ? 'Installed' : 'Not installed') + (SOURCES[g.source] ? ' · ' + SOURCES[g.source] : '') : '';
    $('#gd-icon').innerHTML = g ? iconHtml(g.id) : '';
    $$('#gd input, #gd button').forEach(x => { x.disabled = !g; });
    $('#gd-path').value = g ? staged(g.id, 'game_path:', g.game_path || '') : '';
    $('#gd-detect').value = $('#gd-detect').title = g ? g.detect_path : '';
    $('#gd-args').value = g ? staged(g.id, 'launch_args:', g.launch_args || '') : '';
    $('#gd-ex').checked = !!g && isEx(g);
    const mode = g ? staged(g.id, 'detect_mode:', (st.settings || {})['detect_mode:' + g.id] || 'auto') : 'auto';
    $('#gd-auto').checked = mode !== 'manual'; $('#gd-manual').checked = mode === 'manual';
  }
  async function detect() {
    if (detecting) return; detecting = true;
    const bar = $('#det-bar'); let p = 0; bar.style.width = '0';
    $('#det-msg').textContent = 'Detecting games…';
    const tick = setInterval(() => { p = Math.min(90, p + 7); bar.style.width = p + '%'; }, 120);
    try {
      catalog = await FLB.detectGames();
      const n = catalog.filter(g => g.installed).length;
      $('#det-msg').textContent = `Found ${n} installed game${n === 1 ? '' : 's'} (${catalog.length} known).`;
    } catch (e) { $('#det-msg').textContent = 'Detection failed: ' + e; catalog = catalog || []; }
    clearInterval(tick); bar.style.width = '100%'; detecting = false;
    if (!gsel && catalog.length) { const f = catalog.find(x => x.id === params.get('game')) || catalog.find(x => x.installed) || catalog[0]; gsel = f.id; }
    renderTree(); renderDetail(); loadIcons();
  }
  async function loadCands() {
    try {
      const c = await FLB.candidates();
      $('#cands').innerHTML = c.length ? c.map(x => `<option value="${esc(x.exe)}" title="${esc(x.path)}">${esc(x.exe)}  (${esc(x.path)})</option>`).join('') : '<option disabled>Nothing unknown is running. Start the game, then Refresh.</option>';
    } catch (e) { fail(e); }
  }
  const prettyExe = exe => exe.replace(/\.exe$/i, '').replace(/[-_]+(win64|shipping|x64|dx11|dx12)\b/ig, '').replace(/[_-]+/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2').trim();

  /* ---- Privacy: blocked users ---- */
  async function renderBlocked() {
    const lb = $('#blocked');
    try {
      const f = await FLB.friends();
      if (!f.signed_in) { lb.innerHTML = ''; lb.disabled = true; $('#block-msg').textContent = 'Sign in (Account tab) to manage blocked users.'; $$('[data-do^=block]').forEach(b => { b.disabled = true; }); return; }
      const list = (f.list && f.list.blocked) || [];
      lb.disabled = false; $$('[data-do^=block]').forEach(b => { b.disabled = false; });
      lb.innerHTML = list.map(h => { const n = typeof h === 'string' ? h : h.handle; return `<option value="${esc(n)}">${esc(n)}</option>`; }).join('');
    } catch (e) { $('#block-msg').textContent = String(e); }
  }

  /* ---- Privacy: share recent games + weekly hours with friends (server setting, saved at once) ---- */
  async function renderShare() {
    const cb = $('#share-activity'), m = $('#share-msg');
    let signed = false; try { signed = !!(FLB.syncStatus && (await FLB.syncStatus()).signed_in); } catch (e) {}
    cb.disabled = !signed;
    if (!signed) { cb.checked = false; m.textContent = 'Sign in (Account tab) to choose what friends see.'; return; }
    try { const a = await FLB.accountInfo(); cb.checked = a.share_activity !== false; m.textContent = ''; } catch (e) { m.textContent = String(e); }
  }
  /* ---- Account: optional sync ---- */
  async function renderSync() {
    if (!FLB.syncStatus) return;
    let s; try { s = await FLB.syncStatus(); } catch (e) { $('#sync-box').textContent = String(e); return; }
    const when = t => t ? new Date(t * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : 'not yet';
    const err = s.pairing_error || s.last_error;
    let h;
    if (s.signed_in) {
      h = `Signed in as <b>${esc(s.user || '?')}</b> on <code>${esc(s.server)}</code><br>Hours count while you're connected · last sync ${esc(when(s.last_ok))}
        <div class="btns"><button type="button" data-do="sync-now">Sync now</button><button type="button" data-do="sync-signout">Sign out</button></div>`;
    } else if (s.pairing) {
      h = `In your browser, check the code and approve:<div class="code">${esc(s.pairing.user_code)}</div><span class="hint">Browser didn't open? Go to <code>${esc(s.pairing.verification_uri)}</code></span>
        <div class="btns"><button type="button" data-do="sync-cancel">Cancel</button></div>`;
    } else {
      h = `Off: nothing is tracked. Sign in to count your hours and see your friends.
        <input id="sync-server" maxlength="200" value="${esc(s.server)}" title="Server address" />
        <div class="btns"><button type="button" data-do="sync-signin">Sign in</button></div>`;
    }
    if (err) h += `<div class="bad">${esc(err)}</div>`;
    $('#sync-box').innerHTML = h;
    renderAccount(!!s.signed_in);
  }

  /* ---- Account: change password / email (signed in only) ---- */
  async function renderAccount(signedIn) {
    $('#acct-pw').hidden = $('#acct-email').hidden = !signedIn;
    if (!signedIn || !FLB.accountInfo) return;
    try {
      const a = await FLB.accountInfo();
      $('#em-cur').value = a.email ? a.email + (a.email_verified ? '' : ' (not verified)') : '(none)';
    } catch (e) { $('#em-cur').value = ''; say('em', String(e), true); }
  }
  const say = (k, text, bad) => { const m = $(`#${k}-msg`); m.textContent = text; m.className = 'msg ' + (bad ? 'bad' : 'good'); };
  const errText = e => String(e && e.message || e);
  async function changePassword() {
    const cur = $('#pw-cur').value, n1 = $('#pw-new').value, n2 = $('#pw-new2').value;
    if (!cur) return say('pw', 'Enter your current password.', true);
    if (n1.length < 10) return say('pw', 'The new password needs at least 10 characters.', true);
    if (n1 !== n2) return say('pw', "The new passwords don't match.", true);
    if (n1 === cur) return say('pw', 'The new password is the same as the current one.', true);
    const b = $('[data-do=pw-change]'); b.disabled = true; say('pw', 'Changing…');
    try {
      const r = await FLB.changePassword(cur, n1);
      ['#pw-cur', '#pw-new', '#pw-new2'].forEach(s => { $(s).value = ''; });
      const n = r && r.signed_out_other_sessions || 0;
      say('pw', `Password changed.${n ? ` Signed out ${n} other session${n > 1 ? 's' : ''}.` : ''}`);
    } catch (e) { say('pw', errText(e), true); }
    b.disabled = false;
  }
  async function changeEmail() {
    const email = $('#em-new').value.trim(), cur = $('#em-pw').value;
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) return say('em', 'Enter a valid email address.', true);
    if (!cur) return say('em', 'Enter your current password.', true);
    const b = $('[data-do=em-change]'); b.disabled = true; say('em', 'Changing…');
    try {
      const r = await FLB.changeEmail(cur, email);
      $('#em-new').value = ''; $('#em-pw').value = '';
      $('#em-cur').value = (r && r.email || email) + ' (not verified)';
      say('em', 'Email changed.');
    } catch (e) { say('em', errText(e), true); }
    b.disabled = false;
  }

  const acts = {
    ok: async () => { if (await apply()) close(); },
    cancel: close,
    'win-min': () => FLB.minimizeWindow && FLB.minimizeWindow(),
    apply: async () => { await apply(); if (catalog) { catalog = null; detect(); } },
    advanced: () => { $('#adv').hidden = !$('#adv').hidden; },
    export: async () => { try { const p = await FLB.exportData(); $('#export-msg').innerHTML = 'Saved: <code>' + p.map(esc).join('</code>, <code>') + '</code>'; } catch (e) { fail(e); } },
    'open-data': () => FLB.openData().catch(fail),
    detect,
    browse: async () => { if (!gsel) return; try { const p = await FLB.pickExe(); if (p) { $('#gd-path').value = p; pending.set('game_path:' + gsel, p); pending.set('detect_mode:' + gsel, 'manual'); $('#gd-manual').checked = true; $('#apply').disabled = false; } } catch (e) { fail(e); } },
    'add-manual': () => { $('#manual').hidden = !$('#manual').hidden; if (!$('#manual').hidden) loadCands(); },
    'man-refresh': loadCands,
    'man-add': async () => {
      const exe = $('#cands').value, name = $('#man-name').value.trim();
      if (!exe || !name) { alert('Pick a running program and type the game name.'); return; }
      try { await FLB.tag(exe, name); $('#man-name').value = ''; $('#manual').hidden = true; catalog = null; await detect(); } catch (e) { fail(e); }
    },
    'block-add': async () => { const h = (prompt('Fraglink username to block:') || '').trim(); if (!h) return; try { await FLB.friendBlock(h, true); renderBlocked(); } catch (e) { fail(e); } },
    'block-remove': async () => { const h = $('#blocked').value; if (!h) return; try { await FLB.friendBlock(h, false); renderBlocked(); } catch (e) { fail(e); } },
    'pw-change': changePassword,
    'em-change': changeEmail,
    'sync-signin': () => FLB.syncSignIn($('#sync-server').value.trim()).then(renderSync).catch(fail),
    'sync-cancel': () => FLB.syncCancel().then(renderSync).catch(fail),
    'sync-now': () => FLB.syncNow().then(() => setTimeout(renderSync, 3000)).catch(fail),
    'sync-signout': () => FLB.syncSignOut().then(renderSync).catch(fail),
  };
  document.addEventListener('click', e => {
    const c = e.target.closest('.op-tabs button'); if (c) { show(c.dataset.page); return; }
    const n = e.target.closest('[data-node]'); if (n) { open[n.dataset.node] = !open[n.dataset.node]; renderTree(); return; }
    const l = e.target.closest('[data-g]'); if (l) { gsel = l.dataset.g; renderTree(); renderDetail(); return; }
    const d = e.target.closest('[data-do]'); if (d && acts[d.dataset.do]) acts[d.dataset.do]();
  });
  $('#cands').addEventListener('change', e => { if (!$('#man-name').value) $('#man-name').value = prettyExe(e.target.value); });
  // custom title bar: drag the window, resize from the edges
  document.addEventListener('mousedown', e => {
    if (e.button !== 0 || !FLB.tauri) return;
    const r = e.target.closest('[data-resize]'); if (r) { e.preventDefault(); FLB.resize(r.dataset.resize); return; }
    if (e.target.closest('#op-title') && !e.target.closest('button')) { e.preventDefault(); FLB.drag(); }
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') close();
    else if (e.key === 'Enter' && !['INPUT', 'SELECT', 'BUTTON'].includes(e.target.tagName)) acts.ok();
  });

  (async () => {
    try { st = await FLB.settings(); load(); } catch (e) { fail(e); }
    if (FLB.tauri) document.body.classList.add('tauri');
    const ts = st && st.settings && st.settings.text_size; if (ts) document.body.style.zoom = { large: 1.1, larger: 1.2 }[ts] || '';
    applyTheme((st && st.settings && st.settings.skin) || params.get('skin') || 'dark');
    renderDetail();
    show(params.get('page') || 'general');
    if (FLB.on) {
      FLB.on('fraglink://sync', () => { if (!$('section[data-page=account]').hidden) renderSync(); });
      FLB.on('fraglink://options-page', p => show(p));
      FLB.on('fraglink://settings', ([k, v]) => { if (k === 'skin') applyTheme(v); if (k === 'text_size') document.body.style.zoom = { large: 1.1, larger: 1.2 }[v] || ''; });
    }
  })();
})();
