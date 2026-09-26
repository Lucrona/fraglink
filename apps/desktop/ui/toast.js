/* Bottom-right toast window: "You started playing X" (stand-in for friend toasts until M3). */
(() => {
  'use strict';
  const T = window.__TAURI__;
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const el = document.getElementById('toast'), body = document.getElementById('toast-body');
  let timer = null;
  function tile(name) {
    let h = 0; for (const ch of String(name)) h = (h * 31 + ch.codePointAt(0)) >>> 0;
    const hue = h % 360, words = String(name).replace(/[^A-Za-z0-9 ]+/g, ' ').trim().split(/\s+/).filter(w => !/^(the|of|a)$/i.test(w));
    const g = ((words[0] || '?')[0] + (words.length > 1 ? words[1][0] : '')).toUpperCase();
    return `<svg viewBox="0 0 16 15" width="24" height="24"><defs><linearGradient id="tg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue} 70% 72%)"/><stop offset="1" stop-color="hsl(${(hue + 40) % 360} 60% 28%)"/></linearGradient></defs><rect x=".5" y=".5" width="15" height="14" rx="1.5" fill="url(#tg)" stroke="rgba(0,0,0,.6)"/><rect x="1.5" y="1.2" width="13" height="5" rx="1" fill="#fff" opacity=".28"/><text x="8" y="11" text-anchor="middle" font-family="Tahoma,Verdana,sans-serif" font-weight="bold" font-size="${g.length > 1 ? 7 : 9}" fill="#fff">${esc(g)}</text></svg>`;
  }
  function close() { if (T) T.window.getCurrentWindow().close(); else el.hidden = true; }
  function show(p) {
    if (!p) return;
    body.innerHTML = `<i class="gi">${tile(p.game)}</i><div><b>${esc(p.title)}</b> ${esc(p.line)}<br><span class="g">${esc(p.game)}</span></div>`;
    el.hidden = false; el.classList.remove('out');
    clearTimeout(timer); timer = setTimeout(() => { el.classList.add('out'); setTimeout(close, 300); }, 6000);
  }
  document.getElementById('toast-close').addEventListener('click', e => { e.stopPropagation(); close(); });
  el.addEventListener('click', () => { if (T) T.core.invoke('toast_clicked'); else close(); });
  (async () => {
    if (!T) { show({ title: 'You', line: 'have started playing', game: 'World of Warcraft' }); return; }
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
