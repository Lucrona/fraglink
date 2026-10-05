/* Bottom-right toast window: "You / a friend started playing X", with the game's own icon. */
(() => {
  'use strict';
  const T = window.__TAURI__;
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const el = document.getElementById('toast'), body = document.getElementById('toast-body');
  let timer = null;
  const GENERIC = '<svg class="gi-gen" viewBox="0 0 16 16"><rect x=".5" y=".5" width="15" height="15" rx="2" fill="#39414f" stroke="#15191f"/><path d="M4.2 6h7.6c1.3 0 2.2 1 2.2 2.3l-.4 2.4c-.2 1-1.4 1.3-2.1.6L10.3 10H5.7l-1.2 1.3c-.7.7-1.9.4-2.1-.6L2 8.3C2 7 2.9 6 4.2 6z" fill="#aab4c4"/></svg>';
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
    // plain one-liner: "<name> logged on" / "<name> started playing <game>"
    body.innerHTML = (p.game || !friend ? `<i class="gi">${icon}</i>` : '') +
      `<div><b>${esc(p.title)}</b> ${esc(p.line)}${p.game ? ` <span class="g">${esc(p.game)}</span>` : ''}` +
      `${friend ? '<br><a class="more" href="#" id="toast-more">View More</a>' : ''}</div>`;
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
    if (!T) { document.body.className = 'toast-page skin-' + (localStorage.getItem('fl-skin') || 'dark'); const q = new URLSearchParams(location.search); show(q.get('logon') ? { kind: 'friend:nova', title: 'Nova', line: 'logged on', game: '' } : { kind: 'friend:nova', title: 'Nova', line: 'started playing', game: 'Dota 2' }); return; }
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
