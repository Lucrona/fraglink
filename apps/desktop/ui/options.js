/* Fraglink Options window. Changes are staged and written on Apply / OK;
   the Rust side broadcasts them so the main window follows (skin, zoom…).
   Account (sync) buttons act straight away, like the old dialog. */
(() => {
  'use strict';
  const FLB = window.FL;
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
  const params = new URLSearchParams(location.search);
  const pending = new Map();
  let st = null;

  function show(page) {
    if (!$(`section[data-page="${page}"]`)) page = 'general';
    $$('.op-cats button').forEach(b => b.classList.toggle('on', b.dataset.page === page));
    $$('section[data-page]').forEach(s => { s.hidden = s.dataset.page !== page; });
    if (page === 'account') renderSync();
  }

  function value(el) { return el.type === 'checkbox' ? (el.checked ? '1' : '0') : el.value; }
  function load() {
    const s = st.settings || {};
    const cur = {
      autostart: st.autostart ? '1' : '0',
      idle_threshold_min: String(st.idle_threshold_min), min_session_min: String(st.min_session_min),
      paused: s.paused === '1' ? '1' : '0',
      toasts: s.toasts === '0' ? '0' : '1', friend_toasts: s.friend_toasts === '0' ? '0' : '1', friend_toasts_dnd: s.friend_toasts_dnd === '0' ? '0' : '1',
      skin: ['dark', 'classic'].includes(s.skin) ? s.skin : 'dark', zoom: ['1', '1.5', '2'].includes(s.zoom) ? s.zoom : '1.5',
    };
    $$('[data-key]').forEach(el => {
      const v = cur[el.dataset.key];
      if (el.type === 'checkbox') el.checked = v === '1';
      else if (el.type === 'radio') el.checked = el.value === v;
      else el.value = v;
    });
    scaleBox();
    $('#about').innerHTML = [
      ['Version', `Fraglink ${esc(st.version)}${st.sim ? ' <span class="bad">(simulation mode: fake games)</span>' : ''}`],
      ['Data folder', `<code>${esc(st.data_dir)}</code>`],
      ['Log folder', `<code>${esc(st.log_dir || '-')}</code>`],
      ['Known games', `${st.signatures} (list ${esc(st.signatures_version)})`],
      ['Found on this PC', `${st.steam_apps} Steam, ${st.epic_apps || 0} Epic, ${st.gog_apps || 0} GOG games`],
    ].map(([k, v]) => `<b>${k}</b><span>${v}</span>`).join('') + '<b></b><span><button type="button" data-do="open-data">Open data folder</button></span>';
  }
  function scaleBox() { const r = $('input[name=skin]:checked'); $('#scale-box').hidden = !r || r.value !== 'classic'; }

  document.addEventListener('change', e => {
    const el = e.target.closest('[data-key]'); if (!el) return;
    pending.set(el.dataset.key, value(el));
    $('#apply').disabled = false;
    if (el.name === 'skin') scaleBox();
  });

  async function apply() {
    for (const [k, v] of pending) {
      try {
        if (k === 'autostart') await FLB.autostart(v === '1');
        else await FLB.set(k, v);
      } catch (e) { alert(`Could not save ${k}: ${e}`); return false; }
    }
    pending.clear(); $('#apply').disabled = true;
    return true;
  }
  const close = () => FLB.closeWindow ? FLB.closeWindow() : window.close();

  /* ---- Account: optional sync ---- */
  async function renderSync() {
    if (!FLB.syncStatus) return;
    let s; try { s = await FLB.syncStatus(); } catch (e) { $('#sync-box').textContent = String(e); return; }
    const when = t => t ? new Date(t * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : 'not yet';
    const err = s.pairing_error || s.last_error;
    let h;
    if (s.signed_in) {
      h = `Signed in as <b>${esc(s.user || '?')}</b> on <code>${esc(s.server)}</code><br>${s.pending} session(s) waiting · last sync ${esc(when(s.last_ok))}
        <div class="btns"><button type="button" data-do="sync-now">Sync now</button><button type="button" data-do="sync-signout">Sign out</button></div>`;
    } else if (s.pairing) {
      h = `In your browser, check the code and approve:<div class="code">${esc(s.pairing.user_code)}</div><span class="hint">Browser didn't open? Go to <code>${esc(s.pairing.verification_uri)}</code></span>
        <div class="btns"><button type="button" data-do="sync-cancel">Cancel</button></div>`;
    } else {
      h = `Off: everything stays on this PC. Sign in to back up your sessions and see your friends.
        <input id="sync-server" maxlength="200" value="${esc(s.server)}" title="Server address" />
        <div class="btns"><button type="button" data-do="sync-signin">Sign in to sync</button></div>`;
    }
    if (err) h += `<div class="bad">${esc(err)}</div>`;
    $('#sync-box').innerHTML = h;
  }
  const fail = e => alert(String(e));
  const acts = {
    ok: async () => { if (await apply()) close(); },
    cancel: close,
    apply,
    export: async () => { try { const p = await FLB.exportData(); $('#export-msg').innerHTML = 'Saved: <code>' + p.map(esc).join('</code>, <code>') + '</code>'; } catch (e) { fail(e); } },
    'open-data': () => FLB.openData().catch(fail),
    'sync-signin': () => FLB.syncSignIn($('#sync-server').value.trim()).then(renderSync).catch(fail),
    'sync-cancel': () => FLB.syncCancel().then(renderSync).catch(fail),
    'sync-now': () => FLB.syncNow().then(() => setTimeout(renderSync, 3000)).catch(fail),
    'sync-signout': () => FLB.syncSignOut().then(renderSync).catch(fail),
  };
  document.addEventListener('click', e => {
    const c = e.target.closest('.op-cats button'); if (c) { show(c.dataset.page); return; }
    const g = e.target.closest('[data-go]'); if (g) { show(g.dataset.go); return; }
    const d = e.target.closest('[data-do]'); if (d && acts[d.dataset.do]) acts[d.dataset.do]();
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') close();
    else if (e.key === 'Enter' && e.target.tagName !== 'INPUT') acts.ok();
  });

  (async () => {
    try { st = await FLB.settings(); load(); } catch (e) { fail(e); }
    show(params.get('page') || 'general');
    if (FLB.on) {
      FLB.on('fraglink://sync', () => { if (!$('section[data-page=account]').hidden) renderSync(); });
      FLB.on('fraglink://options-page', p => show(p));
    }
  })();
})();
