/* Bottom-right toast window: "You / a friend started playing X", with the game's own icon. */
(() => {
  'use strict';
  const SHOW_REALM = false; // realm (toast detail line) hidden for now, dev.18
  const T = window.__TAURI__;
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const el = document.getElementById('toast'), body = document.getElementById('toast-body');
  let timer = null;
  const GENERIC = '<svg class="gi-gen" viewBox="0 0 16 16"><rect x=".5" y=".5" width="15" height="15" rx="2" fill="#39414f" stroke="#15191f"/><path d="M4.2 6h7.6c1.3 0 2.2 1 2.2 2.3l-.4 2.4c-.2 1-1.4 1.3-2.1.6L10.3 10H5.7l-1.2 1.3c-.7.7-1.9.4-2.1-.6L2 8.3C2 7 2.9 6 4.2 6z" fill="#aab4c4"/></svg>';
  // The friend's picture, drawn like the website: uploaded image (data URL from the app),
  // preset tile, or the Fraglink mark on one of six muted colours.
  const GLYPHS = ['\u2694', '\u25C9', '\u25B2', '\u2605', '\u25C6', '\u2726', '\u2663', '\u2699', '\u2620', '\u265B', '\u26A1', '\u271A'];
  function avatar(p, cls) {
    const c = (p.avatar_color || 0) % 6, k = cls || '';
    if (p.avatar && /^data:image\/(webp|png|jpeg|svg\+xml)[;,]/.test(p.avatar)) return `<i class="av fav c${c} ${k}"><img src="${p.avatar}" alt=""></i>`;
    if (p.avatar_preset != null) return `<i class="av fav p c${p.avatar_preset % 6} ${k}">${GLYPHS[p.avatar_preset % 12]}</i>`;
    return `<i class="av fav c${c} ${k}"><img class="mk" src="brand/v28-mark.svg" alt=""></i>`;
  }
  function close() { if (T) T.window.getCurrentWindow().close(); else el.hidden = true; }
  function show(p) {
    if (!p) return;
    const icon = p.icon && /^data:image\/png;base64,/.test(p.icon) ? `<img class="gi-img" src="${p.icon}" alt="">` : GENERIC;
    const news = /^news:/.test(p.kind || '');
    if (news) {
      // staff announcement (the website's banner), shown once
      body.innerHTML = `<i class="gi"><img class="gi-img" src="favicon-32.png" alt=""></i><div><b>${esc(p.title)}</b> ${esc(p.line)}${p.game_id ? '<br><a class="more" href="#">More</a>' : ''}</div>`;
      el.hidden = false; el.classList.remove('out');
      clearTimeout(timer); timer = setTimeout(() => { el.classList.add('out'); setTimeout(close, 300); }, 15000);
      return;
    }
    if (/^track:/.test(p.kind || '')) return ask(p, icon);
    const friend = /^(friend|request):/.test(p.kind || '');
    // icon (game, or a letter avatar for a friend with no game), bold name, one line, then a dim line (realm / @username)
    // playing: the game icon with the friend's picture in its corner; otherwise the picture
    // 0.3.0-beta.9 (option J): Steam 2024 layout. Friend's picture + a thin status bar
    // (Fraglink blue in game, light blue online / request); the game icon rides in its corner.
    const req = /^request:/.test(p.kind || '');
    const st = req ? 'st-req' : p.game_id ? 'st-game' : 'st-on';
    const lead = !friend ? `<i class="gi">${icon}</i>` : `<span class="jl">${avatar(p)}${p.game_id && p.icon ? `<i class="jg">${icon}</i>` : ''}<i class="jb ${st}"></i></span>`;
    const sub = req ? p.game : (SHOW_REALM ? p.detail || '' : '');
    // Steam-style: bold name / action ("is now playing", "played 1h 33m of") / game name
    // on its own accent line (wraps to 2 lines, then …)
    body.innerHTML = `${lead}<div class="tx"><b>${esc(p.title)}</b><span class="ln">${esc(p.line)}</span>` +
      `${!req && p.game ? `<span class="g">${esc(p.game)}</span>` : ''}${sub ? `<small>${esc(sub)}</small>` : ''}</div>`;
    el.hidden = false; el.classList.remove('out');
    clearTimeout(timer); timer = setTimeout(() => { el.classList.add('out'); setTimeout(close, 300); }, 6000);
  }
  // beta.5, Discord-style: "Playing <name>?" with Add it! (and a quiet Not a game).
  // The close button only hides it; the question also waits in the main window.
  function ask(p, icon) {
    body.innerHTML = `<i class="gi">${icon}</i><div class="tx"><span class="ln">Playing</span><b>${esc(p.title)}?</b><small>Fraglink doesn't know this one yet.</small><div class="tq"><button type="button" class="add-it" data-yes="1">Add it!</button><button type="button" class="not-game" data-yes="0">Not a game</button></div></div>`;
    body.querySelectorAll('button[data-yes]').forEach(b => b.addEventListener('click', e => {
      e.stopPropagation();
      if (!T) return close();
      T.core.invoke('track_prompt_answer', { exe: p.game, yes: b.dataset.yes === '1', name: p.title }).catch(close);
    }));
    el.hidden = false; el.classList.remove('out');
    clearTimeout(timer); timer = setTimeout(() => { el.classList.add('out'); setTimeout(close, 300); }, 30000);
  }
  document.getElementById('toast-close').addEventListener('click', e => { e.stopPropagation(); close(); });
  el.addEventListener('click', e => { e.preventDefault(); if (T) T.core.invoke('toast_clicked'); else close(); });
  (async () => {
    if (!T) { document.body.className = 'toast-page skin-dark'; el.style.cssText = 'inset:auto;right:4px;bottom:4px;width:283px;box-sizing:border-box;left:auto;animation:none'; // preview: native window width, height grows
      const q = new URLSearchParams(location.search), t = q.get('t') || (q.get('logon') ? 'online' : 'playing');
      if (t === 'track') { ask({ kind: 'track:OddGame.exe', title: 'OddGame', game: 'OddGame.exe' }, GENERIC); return; }
      show(({ played: { kind: 'friend:you', title: 'You', line: 'played 1h 33m of', game_id: 'wow', game: 'World of Warcraft: Forever - Beta', icon: q.get('icon') || '' },
        long: { kind: 'friend:dizz', title: 'Dizz', line: 'is now playing', game_id: 'x', game: 'The Elder Scrolls V: Skyrim Special Edition Anniversary Upgrade Bundle Deluxe', icon: '' },
        online: { kind: 'friend:nova', title: 'Nova', line: 'logged on', game_id: '', game: '', avatar_color: 3 },
        request: { kind: 'request:riftwalker', title: 'Riftwalker', line: 'wants to be your friend', game_id: '', game: '@riftwalker', avatar_preset: 3, avatar_color: 2 },
        playing: { kind: 'friend:dizz', title: 'Dizz', line: 'is now playing', game_id: 'wow', game: 'World of Warcraft', detail: 'Frostmourne', avatar_color: 0, avatar: 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#b0632a"/><circle cx="32" cy="25" r="12" fill="#f1d9c0"/><path d="M10 64c2-15 12-22 22-22s20 7 22 22z" fill="#2b2f3a"/></svg>'), icon: q.get('icon') || '' } })[t]); return; }
    try {
      const st = await window.FL.settings();
      document.body.className = 'toast-page skin-dark';
      document.documentElement.style.setProperty('--z', st.settings.zoom || '1.5');
    } catch (e) {}
    show(await T.core.invoke('toast_payload'));
    T.event.listen('fraglink://toast', e => show(e.payload));
  })();
})();
