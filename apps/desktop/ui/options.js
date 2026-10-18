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

  /** Neutral two-button confirm; Cancel and Confirm are equally weighted. */
  function confirmEqual(message) {
    return new Promise(resolve => {
      const m = document.createElement('div');
      m.className = 'confirm-modal';
      m.innerHTML = `<div class="confirm-box" role="dialog" aria-modal="true" aria-label="Confirm">
        <p>${esc(message)}</p>
        <div class="confirm-actions">
          <button type="button" data-a="cancel">Cancel</button>
          <button type="button" data-a="ok">Confirm</button>
        </div>
      </div>`;
      const done = ok => { m.remove(); resolve(!!ok); };
      const onKey = e => { if (e.key === 'Escape') finish(false); };
      const finish = ok => { document.removeEventListener('keydown', onKey); done(ok); };
      m.addEventListener('click', e => {
        const b = e.target.closest('[data-a]');
        if (b) finish(b.dataset.a === 'ok');
        else if (e.target === m) finish(false);
      });
      document.addEventListener('keydown', onKey);
      document.body.appendChild(m);
      m.querySelector('[data-a="cancel"]').focus();
    });
  }
  const SOURCES = { steam: 'Steam', epic: 'Epic', gog: 'GOG', battlenet: 'Battle.net', ea: 'EA app', ubisoft: 'Ubisoft Connect', xbox: 'Xbox', rsi: 'RSI Launcher', scan: 'found in your game folders', windows: 'installed', seen: 'seen running', manual: 'added by you', tracked: 'played before', signature: '', detectable: '' };
  const PAGES = { notifications: 'general', advanced: 'general', tracking: 'connections' }; // old page names

  // Options always uses the dark look (Dark is the only skin since 0.2.0-dev.20).
  const applyTheme = () => document.body.classList.add('dark');

  function show(page) {
    page = PAGES[page] || page;
    if (!$(`section[data-page="${page}"]`)) page = 'general';
    $$('.op-tabs button').forEach(b => b.classList.toggle('on', b.dataset.page === page));
    $$('section[data-page]').forEach(s => { s.hidden = s.dataset.page !== page; });
    if (page === 'account') renderSync();
    if (page === 'privacy') { renderBlocked(); renderPrivacy(); }
    if (page === 'connections') renderConnections();
    if (page === 'games' && !catalog && !detecting) detect();
  }

  const value = el => el.type === 'checkbox' ? (el.checked ? '1' : '0') : el.value;
  const on = (s, k, def = '1') => (s[k] === '0' || s[k] === '1') ? s[k] : def;
  function load() {
    const s = st.settings || {};
    const cur = {
      autostart: st.autostart ? '1' : '0', start_minimized: on(s, 'start_minimized', '0'), minimize_to_tray: on(s, 'minimize_to_tray', '0'),
      show_offline: on(s, 'show_offline'), show_fof: on(s, 'show_fof'), language: s.language || 'en-AU',
      toasts: on(s, 'toasts'), friend_toasts: on(s, 'friend_toasts'), friend_toasts_logon: on(s, 'friend_toasts_logon'), connection_alerts: on(s, 'connection_alerts', '0'), friend_toasts_dnd: on(s, 'friend_toasts_dnd'),
      share_game: on(s, 'share_game'), friend_request_toasts: on(s, 'friend_request_toasts'), share_fof: on(s, 'share_fof'),
      sound_signin: on(s, 'sound_signin', '0'), sound_signout: on(s, 'sound_signout', '0'), sound_pack: s.sound_pack || 'default',
      zoom: ['1', '1.5', '2'].includes(s.zoom) ? s.zoom : '1.5',
      theme: s.theme || 'blue', always_on_top: on(s, 'always_on_top', '0'),
      text_size: ['normal', 'large', 'larger'].includes(s.text_size) ? s.text_size : 'normal',
    };
    $$('[data-key]').forEach(el => {
      const v = cur[el.dataset.key] ?? '';
      if (el.type === 'checkbox') el.checked = v === '1';
      else if (el.type === 'radio') el.checked = el.value === v;
      else el.value = v;
    });
    $('#g-sort').value = sortBy;
    $('#about').innerHTML = [
      ['Version', `Fraglink ${esc(st.version)}${st.sim ? ' <span class="bad">(simulation mode)</span>' : ''}`],
      ['Data folder', `<code>${esc(st.data_dir)}</code>`],
      ['Log folder', `<code>${esc(st.log_dir || '-')}</code>`],
      ['Known games', `${st.signatures} (list ${esc(st.signatures_version)})`],
      ['Found on this PC', `${st.steam_apps} Steam, ${st.epic_apps || 0} Epic, ${st.gog_apps || 0} GOG games`],
    ].map(([k, v]) => `<b>${k}</b><span>${v}</span>`).join('') + '<b></b><span><button type="button" data-do="open-data">Open data folder</button></span>';
  }

  document.addEventListener('change', e => {
    const t = e.target;
    if (t.id === 'g-sort') { sortBy = SORTS.includes(t.value) ? t.value : 'az'; localStorage.setItem('fl_games_sort', sortBy); renderTree(); return; }
    if (t.matches('[data-pv], [data-pv-top]')) { savePrivacy(t); return; }
    const el = t.closest('[data-key]');
    $('#apply').disabled = false;
    if (el) { pending.set(el.dataset.key, value(el)); return; }
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
    const nt = {};
    if (pending.has('friend_request_toasts')) nt.friend_request_client = pending.get('friend_request_toasts') === '1';
    if (pending.has('friend_toasts') || pending.has('friend_toasts_logon')) nt.friend_online_client = $('[data-key=friend_toasts]').checked || $('[data-key=friend_toasts_logon]').checked;
    if (Object.keys(nt).length && FLB.notifications && await signedIn()) FLB.notifications(nt).catch(() => {});
    pending.clear(); $('#apply').disabled = true;
    return true;
  }
  const close = () => FLB.closeWindow ? FLB.closeWindow() : window.close();

  /* ---- Games tab ---- */
  const staged = (id, key, orig) => pending.has(key + id) ? pending.get(key + id) : orig;
  const isEx = g => staged(g.id, 'exclude:', g.excluded ? '1' : '0') === '1';
  let open = { inst: true, played: true };
  // Sort: A–Z (default), most played, recently played; remembered on this PC.
  const SORTS = ['az', 'most', 'recent'];
  let sortBy = SORTS.includes(localStorage.getItem('fl_games_sort')) ? localStorage.getItem('fl_games_sort') : 'az';
  const byName = (a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  const sorted = arr => arr.slice().sort(sortBy === 'most' ? (a, b) => (b.total_seconds || 0) - (a.total_seconds || 0) || byName(a, b)
    : sortBy === 'recent' ? (a, b) => (b.last_played || 0) - (a.last_played || 0) || byName(a, b) : byName);
  // Played or detected here before (stats or an exclusion), though not found installed now.
  const played = g => !g.installed && ((g.total_seconds || 0) > 0 || !!g.last_played || g.excluded || g.source === 'tracked' || g.source === 'manual');
  const shown = () => catalog ? catalog.filter(g => g.installed || played(g)) : [];
  function renderTree() {
    const t = $('#tree');
    if (!catalog) { t.innerHTML = ''; return; }
    const inst = sorted(catalog.filter(g => g.installed)), prev = sorted(catalog.filter(played));
    const leaf = g => `<div class="leaf ${g.id === gsel ? 'sel' : ''} ${isEx(g) ? 'ex' : ''}" data-g="${esc(g.id)}" title="${esc(SOURCES[g.source] ?? g.source)}">${iconHtml(g.id)}<span>${esc(g.name)}</span></div>`;
    const node = (k, label, arr) => `<div class="node" data-node="${k}"><span class="tw">${open[k] ? '−' : '+'}</span>${label} (${arr.length})</div>` + (open[k] ? arr.map(leaf).join('') : '');
    t.innerHTML = node('inst', 'Installed games', inst) + (prev.length ? node('played', 'Played before', prev) : '');
  }
  // Same icon order as everywhere: server, Steam, exe, then the generic icon.
  const icons = {};
  const GENERIC = '<svg viewBox="0 0 16 16"><rect x="1" y="3" width="14" height="10" rx="3" fill="#3b4a66" stroke="#0b1424"/><circle cx="5" cy="8" r="1.6" fill="#cfd8e8"/><circle cx="11" cy="7" r="1" fill="#cfd8e8"/><circle cx="12" cy="9.4" r="1" fill="#cfd8e8"/></svg>';
  const iconHtml = id => `<i class="gi">${icons[id] ? `<img src="${icons[id]}" alt="">` : GENERIC}</i>`;
  async function loadIcons() {
    if (!catalog || !FLB.gameIcons) return;
    const ids = shown().map(g => g.id).filter(id => !(id in icons)).slice(0, 60);
    if (!ids.length) return;
    try { const m = await FLB.gameIcons(ids, false); for (const id of ids) icons[id] = m && m[id] || null; renderTree(); renderDetail(); } catch (e) { console.warn('icons', e); }
  }
  function renderDetail() {
    const g = catalog && catalog.find(x => x.id === gsel);
    $('#gd-name').textContent = 'Select Game';
    $('#gd-title').textContent = g ? g.name : 'Select a game on the left';
    $('#gd-src').textContent = g ? (g.installed ? 'Installed' : 'Played before') + (SOURCES[g.source] ? ' · ' + SOURCES[g.source] : '') : '';
    $('#gd-icon').innerHTML = g ? iconHtml(g.id) : '';
    $$('#gd input, #gd button').forEach(x => { x.disabled = !g; });
    $('#gd-path').value = g ? staged(g.id, 'game_path:', g.game_path || '') : '';
    $('#gd-detect').value = $('#gd-detect').title = g ? g.detect_path : '';
    $('#gd-args').value = g ? staged(g.id, 'launch_args:', g.launch_args || '') : '';
    $('#gd-ex').checked = !!g && isEx(g);
    const mode = g ? staged(g.id, 'detect_mode:', (st.settings || {})['detect_mode:' + g.id] || 'auto') : 'auto';
    $('#gd-auto').checked = mode !== 'manual'; $('#gd-manual').checked = mode === 'manual';
  }
  async function detect(scan) {
    if (detecting) return; detecting = true;
    const bar = $('#det-bar'); let p = 0; bar.style.width = '0';
    $('#det-msg').textContent = scan === true ? 'Scanning your game folders…' : 'Detecting games…';
    $$('[data-do=detect], [data-do=scan]').forEach(b => { b.disabled = true; });
    const tick = setInterval(() => { p = Math.min(90, p + (scan === true ? 1 : 7)); bar.style.width = p + '%'; }, scan === true ? 400 : 120);
    try {
      catalog = await (scan === true && FLB.scanGames ? FLB.scanGames() : FLB.detectGames());
      const n = catalog.filter(g => g.installed).length;
      $('#det-msg').textContent = `Found ${n} installed game${n === 1 ? '' : 's'}.`;
    } catch (e) { $('#det-msg').textContent = (scan === true ? 'Scan failed: ' : 'Detection failed: ') + e; catalog = catalog || []; }
    clearInterval(tick); bar.style.width = '100%'; detecting = false;
    $$('[data-do=detect], [data-do=scan]').forEach(b => { b.disabled = false; });
    const list = shown();
    if ((!gsel || !list.some(x => x.id === gsel)) && list.length) { const f = list.find(x => x.id === params.get('game')) || list.find(x => x.installed) || list[0]; gsel = f.id; }
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
  // Profile privacy: the very same switches as Settings > Privacy on the website (saved at once).
  const signedIn = async () => { try { return !!(FLB.syncStatus && (await FLB.syncStatus()).signed_in); } catch (e) { return false; } };
  function paintPrivacy(a) {
    const pv = a.privacy || {};
    $$('[data-pv]').forEach(c => { c.checked = pv[c.dataset.pv] !== false; });
    const top = { private: a.public_profile === false, share_activity: a.share_activity !== false, show_friends: a.show_friends !== false };
    $$('[data-pv-top]').forEach(c => { c.checked = !!top[c.dataset.pvTop]; });
    // Save reply has no is_minor — keep the flag from Account info (beta.2).
    // Under-18 may switch to public after confirm; do not lock the checkbox.
    if ('is_minor' in a) minor = !!a.is_minor;
  }
  let minor = false;
  async function renderPrivacy() {
    const m = $('#pv-msg'), boxes = $$('#pv-box input');
    const signed = await signedIn();
    boxes.forEach(c => { c.disabled = !signed; });
    if (!signed) { boxes.forEach(c => { c.checked = false; }); m.textContent = 'Sign in (Account tab) to choose what others see.'; return; }
    try {
      const a = await FLB.accountInfo(); paintPrivacy(a);
      m.textContent = a.is_minor
        ? 'Saved as soon as you click. Under-18 accounts start private; you can make your profile public any time.'
        : 'Saved as soon as you click.';
    } catch (e) { m.textContent = String(e); }
  }
  async function savePrivacy(c) {
    const m = $('#pv-msg'), k = c.dataset.pv, top = c.dataset.pvTop;
    // Unchecking Private = going public. Minors get a short, neutral confirm.
    if (top === 'private' && !c.checked && minor) {
      if (!await confirmEqual('Your profile will be visible to everyone. You can switch back any time.')) {
        c.checked = true; return;
      }
    }
    const patch = k ? { privacy: { [k]: c.checked } } : top === 'private' ? { public_profile: !c.checked } : { [top]: c.checked };
    c.disabled = true;
    try { const r = await FLB.privacy(patch); paintPrivacy(r); m.textContent = 'Saved. It shows on the website straight away.'; }
    catch (e) { c.checked = !c.checked; m.textContent = String(e); }
    c.disabled = false;
  }

  /* ---- Connections: linked accounts, the same as Settings > Connections on the website ---- */
  const CONN = [['steam', 'Steam', 'S', '#1b2838', 'Verify your Steam account; it shows as a badge on your profile'],
    ['bnet', 'Battle.net', 'B', '#148eff', 'Show your BattleTag on your profile'],
    ['discord', 'Discord', 'D', '#5865f2', 'Show your Discord name on your profile']];
  async function renderConnections() {
    const box = $('#conn-list'), m = $('#conn-msg');
    const signed = await signedIn();
    let c = {};
    if (signed) { try { c = await FLB.connections(); } catch (e) { m.textContent = String(e); } }
    box.innerHTML = CONN.map(([p, name, ic, col, blurb]) => {
      const v = c[p] || {}, l = v.linked;
      const who = l ? `<small class="linked">${esc(l.display || l.uid)}${l.verified ? ' · verified' : ''}</small>` : `<small>${blurb}</small>`;
      const right = !signed ? '<button type="button" disabled>Connect</button><span class="soon">sign in first</span>'
        : l ? `<button type="button" data-unlink="${p}">Disconnect</button><span class="soon ok">linked</span>`
        : v.available ? `<button type="button" data-link="${p}">Connect</button><span class="soon"></span>`
        : '<button type="button" disabled>Connect</button><span class="soon">coming soon</span>';
      return `<div class="conn"><i class="conn-ic" style="--c:${col}">${ic}</i><div class="conn-tx"><b>${name}</b>${who}</div>${right}</div>`;
    }).join('');
  }
  $('#conn-list').addEventListener('click', async e => {
    const b = e.target.closest('button'); if (!b) return; const m = $('#conn-msg');
    const p = b.dataset.link || b.dataset.unlink, name = (CONN.find(x => x[0] === p) || [])[1];
    try {
      if (b.dataset.link) { b.disabled = true; await FLB.connectStart(p); m.textContent = `Finish signing in to ${name} in your browser, then come back here.`; setTimeout(() => { b.disabled = false; }, 4000); }
      else if (b.dataset.unlink) { if (!confirm(`Disconnect ${name}?`)) return; await FLB.connectUnlink(p); m.textContent = `${name} disconnected.`; renderConnections(); }
    } catch (x) { b.disabled = false; m.textContent = String(x); }
  });
  // Coming back from the browser: show the new state.
  addEventListener('focus', () => {
    if (!$('section[data-page=connections]').hidden) renderConnections();
    if (!$('section[data-page=privacy]').hidden) renderPrivacy();
  });
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
    $('#acct-pw').hidden = $('#acct-email').hidden = $('#acct-nick').hidden = $('#acct-web').hidden = !signedIn;
    if (!signedIn || !FLB.accountInfo) return;
    try {
      const a = await FLB.accountInfo();
      $('#nk-user').value = a.handle ? '@' + a.handle : ''; $('#nk-new').value = a.display_name || a.handle || '';
      const mf = a.mfa || {}; $('#mfa-line').textContent = mf.enabled ? `Two-step sign-in is on (${mf.backup_codes_left} backup codes left).` : 'Two-step sign-in is off. Turn it on in the website settings.';
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
  async function changeNickname() {
    const b = $('[data-do=nk-change]'); b.disabled = true; say('nk', 'Saving…');
    try {
      const r = await FLB.setNickname($('#nk-new').value);
      $('#nk-new').value = r.display_name;
      say('nk', r.changes_left == null ? 'Saved.' : `Saved. ${r.changes_left} more change${r.changes_left === 1 ? '' : 's'} today.`);
    } catch (e) { say('nk', errText(e), true); }
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
    detect: () => detect(),
    scan: () => detect(true),
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
    'nk-change': changeNickname,
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
    const w = e.target.closest('[data-web]'); if (w && FLB.openWebsite) FLB.openWebsite(w.dataset.web).catch(fail);
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
    // Notification switches the website also shows (Settings > Notifications > Desktop app): the
    // account's choice wins, so a change on the site shows up here.
    if (await signedIn()) {
      try {
        const n = (await FLB.accountInfo()).notify_prefs || {}, set = async (k, v) => { if (st.settings[k] !== v) { await FLB.set(k, v); st.settings[k] = v; } };
        if (typeof n.friend_request_client === 'boolean') await set('friend_request_toasts', n.friend_request_client ? '1' : '0');
        if (n.friend_online_client === false) { await set('friend_toasts', '0'); await set('friend_toasts_logon', '0'); }
        else if (n.friend_online_client === true && st.settings.friend_toasts === '0' && st.settings.friend_toasts_logon === '0') { await set('friend_toasts', '1'); await set('friend_toasts_logon', '1'); }
        load();
      } catch (e) { /* offline: keep this PC's choices */ }
    }
    if (FLB.tauri) document.body.classList.add('tauri');
    const ts = st && st.settings && st.settings.text_size; if (ts) document.body.style.zoom = { large: 1.1, larger: 1.2 }[ts] || '';
    applyTheme();
    renderDetail();
    show(params.get('page') || 'general');
    if (FLB.on) {
      FLB.on('fraglink://sync', () => { if (!$('section[data-page=account]').hidden) renderSync(); });
      FLB.on('fraglink://options-page', p => show(p));
      FLB.on('fraglink://settings', ([k, v]) => { if (k === 'text_size') document.body.style.zoom = { large: 1.1, larger: 1.2 }[v] || ''; });
    }
  })();
})();
