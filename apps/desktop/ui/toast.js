/* Bottom-right toast window: "You / a friend started playing X", with the game's own icon. */
(() => {
  'use strict';
  const SHOW_REALM = false; // realm (toast detail line) hidden for now, dev.18
  const T = window.__TAURI__;
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const el = document.getElementById('toast'), body = document.getElementById('toast-body');
  let timer = null;
  const GENERIC = '<svg class="gi-gen" viewBox="0 0 16 16"><rect x=".5" y=".5" width="15" height="15" rx="2" fill="#39414f" stroke="#15191f"/><path d="M4.2 6h7.6c1.3 0 2.2 1 2.2 2.3l-.4 2.4c-.2 1-1.4 1.3-2.1.6L10.3 10H5.7l-1.2 1.3c-.7.7-1.9.4-2.1-.6L2 8.3C2 7 2.9 6 4.2 6z" fill="#aab4c4"/></svg>';
  function avatar(n) {
    n = String(n || '?'); let h = 0; for (const c of n) h = (h * 31 + c.codePointAt(0)) >>> 0;
    return `<i class="av" style="--av:hsl(${h % 360} 28% 34%)">${esc((n.match(/[A-Za-z0-9]/) || ['?'])[0].toUpperCase())}</i>`;
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
    const lead = p.game_id || !friend ? `<i class="gi">${icon}</i>` : avatar(p.title);
    const req = /^request:/.test(p.kind || '');
    const sub = req ? p.game : (SHOW_REALM ? p.detail || '' : '');
    // Steam-style: bold name / action ("is now playing", "played 1h 33m of") / game name
    // on its own accent line (wraps to 2 lines, then …)
    body.innerHTML = `${lead}<div class="tx"><b>${esc(p.title)}</b><span class="ln">${esc(p.line)}</span>` +
      `${!req && p.game ? `<span class="g">${esc(p.game)}</span>` : ''}${sub ? `<small>${esc(sub)}</small>` : ''}</div>`;
    el.hidden = false; el.classList.remove('out');
    clearTimeout(timer); timer = setTimeout(() => { el.classList.add('out'); setTimeout(close, 300); }, 6000);
  }
  // "Track <name> as a game?" with Yes / Not a game (the question also waits in the main window)
  function ask(p, icon) {
    body.innerHTML = `<i class="gi">${icon}</i><div>Track <b>${esc(p.title)}</b> as a game?<div class="tq"><button type="button" data-yes="1">Yes</button><button type="button" data-yes="0">Not a game</button></div></div>`;
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
    if (!T) { document.body.className = 'toast-page skin-' + (localStorage.getItem('fl-skin') || 'dark'); el.style.cssText = 'inset:auto;right:8px;bottom:8px;width:226px;box-sizing:border-box;animation:none'; // preview: native window width, height grows
      const q = new URLSearchParams(location.search), t = q.get('t') || (q.get('logon') ? 'online' : 'playing');
      show(({ played: { kind: 'friend:you', title: 'You', line: 'played 1h 33m of', game_id: 'wow', game: 'World of Warcraft: Forever - Beta', icon: q.get('icon') || '' },
        long: { kind: 'friend:dizz', title: 'Dizz', line: 'is now playing', game_id: 'x', game: 'The Elder Scrolls V: Skyrim Special Edition Anniversary Upgrade Bundle Deluxe', icon: '' },
        online: { kind: 'friend:nova', title: 'Nova', line: 'logged on', game_id: '', game: '' },
        request: { kind: 'request:riftwalker', title: 'Riftwalker', line: 'wants to be your friend', game_id: '', game: '@riftwalker' },
        playing: { kind: 'friend:dizz', title: 'Dizz', line: 'is now playing', game_id: 'wow', game: 'World of Warcraft', detail: 'Frostmourne', icon: q.get('icon') || '' } })[t]); return; }
    try {
      const st = await window.FL.settings();
      const skin = st.settings.skin || 'classic';
      document.body.className = 'toast-page skin-' + skin;
      document.documentElement.style.setProperty('--z', st.settings.zoom || '1.5');
    } catch (e) {}
    show(await T.core.invoke('toast_payload'));
    T.event.listen('fraglink://toast', e => show(e.payload));
  })();
})();
