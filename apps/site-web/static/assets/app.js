/* Fraglink website: plain JS, no build step. Talks to the server's JSON API with a
   bearer token (kept in localStorage after sign-in). Pages pick their code via <body data-page>. */
(function () {
"use strict";
const API = "/api/v1", TK = "fl_token", UK = "fl_user";
/* Every form posts here if it's ever submitted before its script is ready: never a GET (which
   would put passwords or tokens in the URL and server logs). The server ignores the body. */
const FA = 'method="post" action="/api/v1/form-fallback"';
const TZ = (() => { try { return Intl.DateTimeFormat().resolvedOptions().timeZone || ""; } catch (e) { return ""; } })();
const DISCORD = "https://discord.gg/QbaYRtvADj";
const $ = (s, r) => (r || document).querySelector(s);
const $$ = (s, r) => Array.from((r || document).querySelectorAll(s));
const esc = s => String(s == null ? "" : s).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const token = () => localStorage.getItem(TK);
/* No inline handlers (the site's Content-Security-Policy blocks them): broken images and a couple
   of buttons are handled here instead, by data attributes. */
document.addEventListener("error", e => {
  const t = e.target;
  if (!t || t.tagName !== "IMG") return;
  if (t.hasAttribute("data-hide-broken")) t.style.visibility = "hidden";
  else if (t.hasAttribute("data-broken-remove")) t.remove();
  else if (t.hasAttribute("data-broken-parent")) { if (t.parentNode) t.parentNode.classList.add("is-broken"); }
  else if (t.hasAttribute("data-broken-swap")) { const s = document.createElement("span"); s.className = ("mtile-none " + t.getAttribute("data-broken-swap")).trim(); s.textContent = "none"; t.replaceWith(s); }
}, true);
document.addEventListener("click", e => {
  const b = e.target.closest && e.target.closest("[data-wall],[data-reload]");
  if (!b) return;
  if (b.hasAttribute("data-reload")) { location.reload(); return; }
  const m = b.closest("section,div").parentNode.querySelector("[data-wmore]");
  if (m) m.hidden = false;
  b.parentNode.remove();
});
// Wowhead tooltips: hover-capable devices only; script loaded the first time a wowhead.com link is on the page.
const WH = (() => {
  const ok = matchMedia("(hover: hover) and (pointer: fine)").matches;
  let st = 0; // 0 none, 1 loading, 2 ready
  const seen = new WeakSet();
  const has = () => { let n = false; document.querySelectorAll('a[href^="https://www.wowhead.com/"],a[href^="https://wowhead.com/"]').forEach(a => { if (!seen.has(a)) { seen.add(a); n = true; } }); return n; };
  const refresh = () => { try { window.$WowheadPower && window.$WowheadPower.refreshLinks(); } catch (e) {} };
  const scan = () => {
    if (!ok || !has()) return;
    if (st === 2) return refresh();
    if (st) return; st = 1;
    window.whTooltips = { colorLinks: false, iconizeLinks: false, renameLinks: false };
    const s = document.createElement("script"); s.src = "https://wow.zamimg.com/js/tooltips.js"; s.async = true;
    s.onload = () => { st = 2; refresh(); }; s.onerror = () => { st = 0; };
    document.head.appendChild(s);
  };
  let t = 0; const later = () => { clearTimeout(t); t = setTimeout(scan, 300); };
  const mine = n => n.nodeType !== 1 || !!(n.closest && n.closest(".wowhead-tooltip,[class^=wowhead]"));
  if (ok) addEventListener("load", () => { later(); new MutationObserver(ms => { if (ms.some(m => [...m.addedNodes].some(n => !mine(n)))) later(); }).observe(document.body, { childList: true, subtree: true }); });
  // Keep the tooltip beside the cursor (Wowhead places it once per link); flip left/up near viewport edges.
  let mx = 0, my = 0, raf = 0;
  const place = () => { raf = 0; const t = document.querySelector(".wowhead-tooltip"); if (!t || t.offsetParent === null && getComputedStyle(t).position !== "fixed") return;
    const w = t.offsetWidth, h = t.offsetHeight, vw = document.documentElement.clientWidth, vh = innerHeight, g = 16;
    let x = mx + g, y = my + g; if (x + w > vw - 4) x = Math.max(4, mx - g - w); if (y + h > vh - 4) y = Math.max(4, my - g - h);
    const fx = getComputedStyle(t).position === "fixed"; t.style.left = (x + (fx ? 0 : scrollX)) + "px"; t.style.top = (y + (fx ? 0 : scrollY)) + "px"; };
  if (ok) document.addEventListener("mousemove", e => { if (!e.target.closest || !e.target.closest('a[href*="wowhead.com/"]')) return; mx = e.clientX; my = e.clientY; if (!raf) raf = requestAnimationFrame(place); }, true);
  if (ok) addEventListener("load", () => new MutationObserver(() => { if (!raf) raf = requestAnimationFrame(place); }).observe(document.body, { childList: true }));
  return { ok, scan };
})();
const me = () => { try { return JSON.parse(localStorage.getItem(UK)); } catch (e) { return null; } };
/* Zone for every day and time shown: the one saved in Settings > Account > Time zone, else the
   browser's (the server resolves weeks the same way, see week.rs `zone`). */
const okZone = z => { if (!z) return false; try { new Intl.DateTimeFormat("en", { timeZone: z }); return true; } catch (e) { return false; } };
const SAVED0 = (() => { const z = (me() || {}).timezone; return okZone(z) ? z : null; })();
const ZONE = SAVED0 || TZ || undefined;
function setSession(t, u) { localStorage.setItem(TK, t); localStorage.setItem(UK, JSON.stringify(u)); }
function clearSession() { localStorage.removeItem(TK); localStorage.removeItem(UK); }


/** Neutral two-button confirm: Cancel and Confirm are equal weight (no primary). */
function confirmEqual(message) {
  return new Promise(resolve => {
    const m = document.createElement("div");
    m.className = "confirm-modal";
    m.innerHTML = `<div class="confirm-box panel" role="dialog" aria-modal="true" aria-label="Confirm">
      <p>${esc(message)}</p>
      <div class="confirm-actions">
        <button type="button" class="btn" data-a="cancel">Cancel</button>
        <button type="button" class="btn" data-a="ok">Confirm</button>
      </div>
    </div>`;
    const done = ok => { m.remove(); resolve(!!ok); };
    const onKey = e => { if (e.key === "Escape") finish(false); };
    const finish = ok => { document.removeEventListener("keydown", onKey); done(ok); };
    m.addEventListener("click", e => {
      const b = e.target.closest("[data-a]");
      if (b) finish(b.dataset.a === "ok");
      else if (e.target === m) finish(false);
    });
    document.addEventListener("keydown", onKey);
    document.body.appendChild(m);
    m.querySelector('[data-a="cancel"]').focus();
  });
}

async function api(path, opt) {
  opt = opt || {};
  const h = {}, t = token();
  if (t) h.Authorization = "Bearer " + t;
  if (opt.body !== undefined) h["Content-Type"] = "application/json";
  let r;
  try {
    r = await fetch(API + path, { method: opt.method || "GET", headers: h, cache: "no-store",
      body: opt.body !== undefined ? JSON.stringify(opt.body) : undefined });
  } catch (e) { const er = new Error("Can't reach Fraglink right now. Check your connection and try again."); er.status = 0; throw er; }
  const txt = await r.text(); let data = null;
  try { data = txt ? JSON.parse(txt) : null; } catch (e) { data = null; }
  // Only a dead sign-in token signs you out ("unauthorized"). Other 401s are a wrong code or
  // password typed into a form (bad_code, bad_credentials) and must keep the session.
  if (r.status === 401 && t && !opt.keepSession && (!data || data.error === "unauthorized")) { clearSession(); }
  if (!r.ok) {
    const er = new Error((data && data.message) || ("Something went wrong (HTTP " + r.status + ")."));
    er.status = r.status; er.code = data && data.error; throw er;
  }
  return data;
}

/* ---------- formatting ---------- */
const nf = new Intl.NumberFormat("en");
function hours(min, opts) {
  min = min || 0;
  if (min <= 0) return opts && opts.zero ? "0 hours" : "-";
  if (min < 60) return "<1 hour";
  const h = Math.round(min / 60);
  return nf.format(h) + (h === 1 ? " hour" : " hours");
}
function dur(sec) {
  sec = Math.max(0, Math.round(sec || 0));
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60);
  if (h >= 1) return h + (h === 1 ? " hour" : " hours") + (m > 0 ? " " + m + " min" : "");
  return Math.max(1, m) + " min";
}
function since(iso) { return dur((Date.now() - new Date(iso).getTime()) / 1000); }
const dayFmt = new Intl.DateTimeFormat("en-AU", { weekday: "long", timeZone: ZONE });
const dateFmt = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", timeZone: ZONE });
const fullDate = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", year: "numeric", timeZone: ZONE });
/* Calendar day ("2026-11-04") of an instant in ZONE; a plain day string from the server is
   already a day (in the same zone) and is kept as is. */
const keyFmt = new Intl.DateTimeFormat("en-CA", { year: "numeric", month: "2-digit", day: "2-digit", timeZone: ZONE });
const isDay = d => typeof d === "string" && /^\d{4}-\d{2}-\d{2}$/.test(d);
const dayKey = d => isDay(d) ? d : keyFmt.format(new Date(d));
const dayNo = k => { const [y, m, d] = k.split("-").map(Number); return Date.UTC(y, m - 1, d) / 86400000; };
/* A plain day as a Date at noon UTC, formatted in UTC so it never shifts a day. */
const utcDay = { w: new Intl.DateTimeFormat("en-AU", { weekday: "long", timeZone: "UTC" }), d: new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", timeZone: "UTC" }), f: new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" }) };
const fullDay = d => isDay(d) ? utcDay.f.format(new Date(d + "T12:00:00Z")) : fullDate.format(new Date(d));
/* Friends' activity rows from the server: "playing" first, then one per friend/game/day. */
function actDay(i) { return i.kind === "playing" ? "Now" : dayLabel(i.day || i.at); }
function actText(i) {
  const g = isUnlisted(i.game_id) ? "a game" : `<b>${esc(i.game_name)}</b>`;
  return i.kind === "playing" ? `is playing ${g} now` : `played ${g} for ${dur(i.duration_s)}`;
}
/* "Today", "Yesterday", a weekday or a date, counted in ZONE. */
function dayLabel(d) {
  const k = dayKey(d), diff = Math.round(dayNo(dayKey(Date.now())) - dayNo(k));
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  const x = new Date(k + "T12:00:00Z");
  if (diff > 0 && diff < 7) return utcDay.w.format(x);
  return utcDay.d.format(x);
}
function lastPlayed(iso) {
  if (!iso) return "-";
  const k = dayKey(iso), diff = Math.round(dayNo(dayKey(Date.now())) - dayNo(k)), x = new Date(k + "T12:00:00Z");
  if (diff === 0) return "Today";
  if (diff > 0 && diff < 7) return utcDay.w.format(x).slice(0, 3);
  return utcDay.d.format(x);
}
const initial = s => esc((s || "?").trim().charAt(0).toUpperCase() || "?");
const tint = s => "c" + ((Array.from(s || "").reduce((a, c) => a + c.charCodeAt(0), 0) % 3) + 1);
const isUnlisted = g => !g || g === "unlisted";
const statusText = p => !p ? "Offline" : p.status === "online" ? "Online" : p.status === "away" ? "Away" : "Offline";
function presenceLine(p) {
  if (!p || p.status === "offline") return "";
  if (p.game_name) return isUnlisted(p.game_id) ? "Playing a game" : "Playing " + esc(p.game_name);
  return "";
}
function qs(name) { return new URLSearchParams(location.search).get(name); }
function safeNext(n) { return n && n.startsWith("/") && !n.startsWith("//") && !n.includes("\\") ? n : null; }

/* ---------- game icons ---------- */
const iconCache = {};
async function loadIcons(ids) {
  ids = Array.from(new Set(ids.filter(i => i && !isUnlisted(i) && !(i in iconCache))));
  for (let i = 0; i < ids.length; i += 50) {
    const chunk = ids.slice(i, i + 50);
    try {
      const m = await api("/games/icons?ids=" + encodeURIComponent(chunk.join(",")));
      chunk.forEach(id => {
        const v = m && m[id];
        let src = null;
        if (v && v.icon_path) src = v.icon_path; // served from our own domain (/icons/games/...)
        else if (v && v.icon_url) src = v.icon_url;
        iconCache[id] = src;
      });
    } catch (e) { chunk.forEach(id => { iconCache[id] = null; }); }
  }
}
function icon(id, name, cls) {
  const src = iconCache[id];
  if (src) return `<img class="${cls || ""}" src="${esc(src)}" alt="" loading="lazy" width="20" height="20" data-hide-broken>`;
  return `<span class="gi ${cls || ""}">${isUnlisted(id) ? "?" : initial(name)}</span>`;
}

/* ---------- v2 helpers: avatars, game art, bot check ---------- */
const PRESET_GLYPHS = ["\u2694", "\u25C9", "\u25B2", "\u2605", "\u25C6", "\u2726", "\u2663", "\u2699", "\u2620", "\u265B", "\u26A1", "\u271A"];
/* Avatar crop editor: square frame, zoom (slider + wheel), drag to pan, live 184/64/32
   previews. Nothing is sent until Save; the server crops the original (x, y, size in
   source pixels), so GIFs/PNGs keep full quality and transparency. */
function cropEditor(file, onSave) {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file), img = new Image();
    img.onerror = () => { URL.revokeObjectURL(url); reject(new Error("That image couldn't be opened.")); };
    img.onload = () => {
      const W = img.naturalWidth, H = img.naturalHeight;
      if (W < 16 || H < 16) { URL.revokeObjectURL(url); return reject(new Error("That image is too small (at least 16 x 16).")); }
      const V = 320, minSide = Math.min(W, H);
      let side = minSide, cx = W / 2, cy = H / 2; // crop square in source pixels
      const maxZoom = Math.max(1, minSide / 16);
      const m = document.createElement("div");
      m.className = "crop-modal";
      m.innerHTML = `<div class="crop-box panel" role="dialog" aria-modal="true" aria-label="Crop your picture"><h2>Crop your picture</h2><div class="body">
        <div class="crop-main"><div class="crop-stage"><canvas width="${V}" height="${V}" data-c></canvas><div class="crop-frame"></div></div>
        <div class="crop-side"><div class="crop-prev"><div><canvas width="184" height="184" data-p="184"></canvas><small>184 profile</small></div>
          <div><canvas width="64" height="64" data-p="64"></canvas><small>64 panels</small><canvas width="32" height="32" data-p="32" style="margin-top:10px"></canvas><small>32 lists</small></div></div></div></div>
        <label class="crop-zoom">Zoom <input type="range" min="1" max="${maxZoom.toFixed(2)}" step="0.01" value="1" data-z></label>
        <p class="hint">Drag to move · scroll or use the slider to zoom. Nothing changes until you press Save.</p>
        <div class="row"><button class="btn dl" type="button" data-save>Save</button><button class="btn" type="button" data-cancel>Cancel</button><span class="crop-err"></span></div></div></div>`;
      document.body.appendChild(m);
      const cv = m.querySelector("[data-c]"), ctx = cv.getContext("2d"), z = m.querySelector("[data-z]");
      const clamp = () => {
        side = Math.min(minSide, Math.max(16, side));
        cx = Math.min(W - side / 2, Math.max(side / 2, cx)); cy = Math.min(H - side / 2, Math.max(side / 2, cy));
      };
      const paint = (c, n) => {
        const g = c.getContext("2d"); g.imageSmoothingQuality = "high";
        g.fillStyle = "#1b1f26"; g.fillRect(0, 0, n, n);
        g.drawImage(img, cx - side / 2, cy - side / 2, side, side, 0, 0, n, n);
      };
      const draw = () => {
        clamp(); paint(cv, V);
        m.querySelectorAll("[data-p]").forEach(c => paint(c, +c.dataset.p));
        z.value = (minSide / side).toFixed(2);
      };
      z.addEventListener("input", () => { side = minSide / +z.value; draw(); });
      cv.addEventListener("wheel", e => { e.preventDefault(); side *= e.deltaY > 0 ? 1.08 : 1 / 1.08; draw(); }, { passive: false });
      let drag = null;
      cv.addEventListener("pointerdown", e => { drag = { x: e.clientX, y: e.clientY, cx, cy }; cv.setPointerCapture(e.pointerId); });
      cv.addEventListener("pointermove", e => { if (!drag) return; const k = side / V; cx = drag.cx - (e.clientX - drag.x) * k; cy = drag.cy - (e.clientY - drag.y) * k; draw(); });
      cv.addEventListener("pointerup", () => { drag = null; });
      const close = () => { m.remove(); URL.revokeObjectURL(url); document.removeEventListener("keydown", esc); };
      const esc = e => { if (e.key === "Escape") { close(); resolve(false); } };
      document.addEventListener("keydown", esc);
      m.querySelector("[data-cancel]").addEventListener("click", () => { close(); resolve(false); });
      m.addEventListener("click", e => { if (e.target === m) { close(); resolve(false); } });
      m.querySelector("[data-save]").addEventListener("click", async e => {
        const b = e.target; b.disabled = true;
        const s = Math.max(16, Math.min(minSide, Math.round(side)));
        const crop = { size: s, x: Math.max(0, Math.min(W - s, Math.round(cx - s / 2))), y: Math.max(0, Math.min(H - s, Math.round(cy - s / 2))) };
        try { await onSave(crop); close(); resolve(true); }
        catch (x) { b.disabled = false; m.querySelector(".crop-err").textContent = x.message; }
      });
      draw();
    };
    img.src = url;
  });
}

/* One public base for profile + signature links (server's FRAGLINK_SITE_URL). */
const siteBase = () => ((me() && me().site_url) || location.origin).replace(/\/$/, "");
const sigUrl = h => siteBase() + "/api/v1/sig/" + encodeURIComponent(h) + ".png";
/** Signature <img> with a friendly placeholder if the PNG 404s (private, test, or network). */
const sigPreview = (handle, cls = "sigimg") => {
  const src = "/api/v1/sig/" + encodeURIComponent(handle) + ".png" + sigBust();
  return `<span class="sigprev"><img class="${cls}" src="${src}" alt="" width="460" height="100" data-broken-parent><span class="sigph">Signature preview will show here for public profiles. New accounts get a default card straight away.</span></span>`;
};
const sigBust = () => "?t=" + Math.floor(Date.now() / 30000);
function avatar(a, size, extra) {
  a = a || { kind: "default", color: 0 };
  const st = `width:${size}px;height:${size}px`, x = extra || "";
  if (a.kind === "upload" && a.url_184) {
    const src = size > 64 ? a.url_184 : size > 32 ? a.url_64 : a.url_32;
    return `<span class="av up ${x}" style="${st}"><img src="${esc(src)}" alt="" loading="lazy"></span>`;
  }
  if (a.kind === "preset") return `<span class="av p c${a.preset % 6} ${x}" style="${st};font-size:${Math.round(size * 0.46)}px" aria-hidden="true">${PRESET_GLYPHS[a.preset % 12]}</span>`;
  return `<span class="av c${(a.color || 0) % 6} ${x}" style="${st}"><img class="mk" src="/assets/mark.svg" alt=""></span>`;
}
const artUrl = appid => appid ? `/api/v1/art/${encodeURIComponent(appid)}.jpg` : null;
/* Banner art by game id (server picks Steam / Discord directory / admin upload). */
const gartUrl = id => id && !isUnlisted(id) ? `/api/v1/game-art/${encodeURIComponent(id)}.jpg` : null;
function artHue(s) { let h = 0; for (const c of String(s || "")) h = (h * 31 + c.charCodeAt(0)) >>> 0; return h % 6; }
const ART_GRAD = ["#1d3a5f,#0c1522", "#3d2357,#120c1c", "#1f4a3c,#0b1813", "#5a3216,#1a0f07", "#4a1c26,#170a0d", "#26405a,#101820"];
const artGrad = name => { const [a, b] = ART_GRAD[artHue(name)].split(","); return `linear-gradient(135deg,${a},${b})`; };
/* Art with a tasteful fallback: gradient + the game's icon, covered by the picture when it loads. */
function gameArt(id, name, cls) {
  const u = gartUrl(id);
  return `<div class="artbox ${cls || ""}" style="background:${artGrad(name)}"><span class="ai">${iconCache[id] ? icon(id, name) : `<img src="/assets/mark.svg" alt="" style="opacity:.35">`}</span>${u ? `<img class="ov" src="${u}" alt="" loading="lazy" data-broken-remove>` : ""}</div>`;
}
const artBg = (id, name) => { const u = gartUrl(id); return `${u ? `url(${u}) center/cover no-repeat, ` : ""}${artGrad(name)}`; };
function turnstileToken(form) {
  const i = form.querySelector('[name="cf-turnstile-response"]');
  return i ? i.value : null;
}
function turnstileReset() { try { if (window.turnstile) window.turnstile.reset(); } catch (e) { /* ignore */ } }
function loadTurnstile() {
  if (!document.querySelector(".cf-turnstile") || window.turnstile) return;
  const s = document.createElement("script");
  s.src = "https://challenges.cloudflare.com/turnstile/v0/api.js"; s.async = true; s.defer = true;
  document.head.appendChild(s);
}
const plural = (n, w) => nf.format(n) + " " + w + (n === 1 ? "" : "s");
function hrsShort(min) { min = min || 0; if (min <= 0) return "0"; if (min < 60) return "<1"; return nf.format(Math.round(min / 60)); }

/* ---------- header / footer ---------- */
function renderHeader() {
  const u = me(), page = document.body.dataset.page;
  const nav = $(".nav");
  if (nav) {
    const items = [["home", "/", "Home"]];
    if (u) items.push(["profile", "/u/" + encodeURIComponent(u.handle), "Profile"], ["friends", "/friends", "Friends"]);
    items.push(["download", "/download", "Download"], ["faq", "/faq", "FAQ"]);
    const mine = page === "profile" && u && location.pathname.toLowerCase() === "/u/" + u.handle.toLowerCase();
    nav.innerHTML = items.map(([k, href, label]) => {
      const on = (k === page && (k !== "profile" || mine)) ? "on" : "";
      const badge = (k === "friends" && LIVE_FRIENDS.incoming > 0)
        ? `<span class="nav-badge" title="${LIVE_FRIENDS.incoming} friend request${LIVE_FRIENDS.incoming === 1 ? "" : "s"}">${LIVE_FRIENDS.incoming > 99 ? "99+" : LIVE_FRIENDS.incoming}</span>`
        : "";
      return `<a href="${href}" class="${on}">${label}${badge}</a>`;
    }).join("");
  }
  const right = $(".top .right");
  if (!right) return;
  if (u) {
    right.innerHTML = `<form class="find" role="search"><input class="search" type="search" name="q" placeholder="Find a player" aria-label="Find a player" autocomplete="off"></form>
      <div class="who-menu"><button type="button" aria-haspopup="true">${avatar(u.avatar, 20, "mini-av")}<span title="@${esc(u.handle)}">${esc(u.display_name || u.handle)}</span> ▾</button>
      <div class="menu" role="menu"><a href="/u/${encodeURIComponent(u.handle)}">My profile</a><a href="/friends">Friends</a><a href="/settings">Account settings</a>${u.staff_role ? '<a href="/admin">Admin</a>' : ""}<hr><button type="button" data-logout>Sign out</button></div></div>`;
    const wm = $(".who-menu", right);
    $("button", wm).addEventListener("click", e => { e.stopPropagation(); wm.classList.toggle("open"); });
    document.addEventListener("click", () => wm.classList.remove("open"));
    $("[data-logout]", right).addEventListener("click", logout);
    $(".find", right).addEventListener("submit", e => {
      e.preventDefault(); const q = e.target.q.value.trim().replace(/^@/, "");
      if (q) location.href = "/u/" + encodeURIComponent(q);
    });
  } else {
    right.innerHTML = `<a class="btn" href="/login${page === "login" ? "" : "?next=" + encodeURIComponent(location.pathname)}">Sign in</a><a class="btn dl" href="/download">Get Fraglink</a>`;
  }
}
async function logout() {
  try { await api("/auth/logout", { method: "POST", keepSession: true }); } catch (e) { /* token may already be gone */ }
  clearSession(); location.href = "/";
}
function requireLogin() {
  if (token()) return true;
  location.replace("/login?next=" + encodeURIComponent(location.pathname + location.search));
  return false;
}
function showErr(el, e) { el.className = "alert err"; el.textContent = e.message || String(e); el.hidden = false; }
function showOk(el, msg) { el.className = "alert ok"; el.textContent = msg; el.hidden = false; }

/* ---------- latest release ---------- */
async function latest() {
  const r = await fetch("/updates/latest.json", { cache: "no-store" });
  if (!r.ok) throw new Error("manifest " + r.status);
  const j = await r.json();
  const p = j.platforms && (j.platforms["windows-x86_64"] || Object.values(j.platforms)[0]);
  return { version: String(j.version || "").replace(/-(\d+)$/, "-dev.$1"), raw: j.version, url: p && p.url, date: j.pub_date, notes: j.notes };
}
async function fillDownload() {
  const btns = $$("[data-dl]"), vers = $$("[data-ver]");
  /* staff can point the links somewhere else in Admin > Site settings (empty = built in) */
  const links = await featuresReady.then(() => LINKS).catch(() => LINKS);
  $$("[data-link]").forEach(a => { if (links[a.dataset.link]) a.href = links[a.dataset.link]; });
  if (links.link_download) btns.forEach(b => { b.href = links.link_download; b.removeAttribute("aria-disabled"); });
  try {
    const l = await latest();
    btns.forEach(b => { if (l.url && !links.link_download) b.href = l.url; b.removeAttribute("aria-disabled"); });
    vers.forEach(v => { v.textContent = "Version " + l.version + (l.date ? " · " + fullDay(l.date) : "") + " · Windows 10/11, 64-bit"; });
  } catch (e) {
    vers.forEach(v => { v.textContent = "Download's not available right now. Try again in a few minutes."; });
  }
}


/* ---------- live friends (WebSocket with ?token=, 20s poll fallback) ---------- */
const LIVE_FRIENDS = { incoming: 0, friends: 0, online: 0, list: null, _ws: null, _poll: null, _onChange: [] };
function onFriendsLive(fn) { LIVE_FRIENDS._onChange.push(fn); }
function friendsLiveNotify() {
  renderHeader();
  LIVE_FRIENDS._onChange.forEach(fn => { try { fn(LIVE_FRIENDS); } catch (e) { console.error(e); } });
}
function applyFriendsSnapshot(fl) {
  if (!fl) return;
  LIVE_FRIENDS.list = fl;
  LIVE_FRIENDS.incoming = (fl.incoming && fl.incoming.length) || fl.incoming_count || 0;
  LIVE_FRIENDS.friends = (fl.friends && fl.friends.length) || fl.all || 0;
  const online = fl.online != null ? fl.online
    : (fl.friends || []).filter(f => f.presence && f.presence.status !== "offline").length;
  LIVE_FRIENDS.online = online;
  friendsLiveNotify();
}
async function refreshFriendsLive() {
  if (!token()) return;
  try {
    const fl = await api("/friends");
    applyFriendsSnapshot(fl);
  } catch (e) { /* signed out or offline */ }
}
function friendsWsUrl() {
  const t = token(); if (!t) return null;
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return proto + "//" + location.host + "/api/v1/ws?token=" + encodeURIComponent(t);
}
function startFriendsLive() {
  if (!token() || LIVE_FRIENDS._ws || LIVE_FRIENDS._poll) return;
  refreshFriendsLive();
  const url = friendsWsUrl();
  let usePoll = !url || typeof WebSocket === "undefined";
  if (!usePoll) {
    try {
      const ws = new WebSocket(url);
      LIVE_FRIENDS._ws = ws;
      let ping = null;
      ws.onopen = () => {
        ping = setInterval(() => { try { ws.send(JSON.stringify({ type: "ping" })); } catch (e) {} }, 30000);
      };
      ws.onmessage = ev => {
        let m; try { m = JSON.parse(ev.data); } catch (e) { return; }
        if (!m || !m.type) return;
        if (m.type === "hello" && m.friends) applyFriendsSnapshot(m.friends);
        else if (m.type === "friend_request" || m.type === "friend_accepted" || m.type === "friend_removed" || m.type === "presence") {
          // Re-fetch so counts and lists stay exact (presence alone doesn't change request counts).
          refreshFriendsLive();
        }
      };
      ws.onclose = () => {
        LIVE_FRIENDS._ws = null;
        if (ping) clearInterval(ping);
        // fall back to polling if the socket drops
        if (token() && !LIVE_FRIENDS._poll) {
          LIVE_FRIENDS._poll = setInterval(() => {
            if (document.visibilityState === "visible") refreshFriendsLive();
          }, 20000);
        }
      };
      ws.onerror = () => { try { ws.close(); } catch (e) {} };
    } catch (e) { usePoll = true; LIVE_FRIENDS._ws = null; }
  }
  if (usePoll && !LIVE_FRIENDS._poll) {
    LIVE_FRIENDS._poll = setInterval(() => {
      if (document.visibilityState === "visible") refreshFriendsLive();
    }, 20000);
  }
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && token()) refreshFriendsLive();
  });
}

/* ---------- pages ---------- */
/* provider logos (Simple Icons paths): profile badges and the Battle.net verify page */
const BRAND = { steam: ["Steam", "#66c0f4", "M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z"], bnet: ["Battle.net", "#148eff", "M18.94 8.296C15.9 6.892 11.534 6 7.426 6.332c.206-1.36.714-2.308 1.548-2.508 1.148-.275 2.4.48 3.594 1.854.782.102 1.71.28 2.355.429C12.747 2.013 9.828-.282 7.607.565c-1.688.644-2.553 2.97-2.448 6.094-2.2.468-3.915 1.3-5.013 2.495-.056.065-.181.227-.137.305.034.058.146-.008.194-.04 1.274-.89 2.904-1.373 5.027-1.676.303 3.333 1.713 7.56 4.055 10.952-1.28.502-2.356.536-2.946-.087-.812-.856-.784-2.318-.19-4.04a26.764 26.764 0 0 1-.807-2.254c-2.459 3.934-2.986 7.61-1.143 9.11 1.402 1.14 3.847.725 6.502-.926 1.505 1.672 3.083 2.74 4.667 3.094.084.015.287.043.332-.034.034-.06-.08-.124-.131-.149-1.408-.657-2.64-1.828-3.964-3.515 2.735-1.929 5.691-5.263 7.457-8.988 1.076.86 1.64 1.773 1.398 2.595-.336 1.131-1.615 1.84-3.403 2.185a27.697 27.697 0 0 1-1.548 1.826c4.634.16 8.08-1.22 8.458-3.565.286-1.786-1.295-3.696-4.053-5.17.696-2.139.832-4.04.346-5.588-.029-.08-.106-.27-.196-.27-.068 0-.067.13-.063.187.135 1.547-.263 3.2-1.062 5.19zm-8.533 9.869c-1.96-3.145-3.09-6.849-3.082-10.594 3.702-.124 7.474.748 10.714 2.627-1.743 3.269-4.385 6.1-7.633 7.966h.001z"], discord: ["Discord", "#5865f2", "M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z"] };
const pages = {};

/* ---------- /changelog: All / App / Website tabs, 8 entries a page ---------- */
/* Entries staff publish in Admin > Changelog come from the API and slot in by date. */
const clDate = s => { const t = Date.parse(s); return isNaN(t) ? 0 : t; };
function clSection(e) {
  const list = (h, items) => items && items.length ? `<h4>${h}</h4><ul>${items.map(i => `<li>${esc(i)}</li>`).join("")}</ul>` : "";
  const sec = document.createElement("section");
  sec.className = "panel"; sec.dataset.k = e.area; sec.dataset.date = e.date;
  sec.innerHTML = `<h2><span class="cl-tag ${esc(e.area)}">${esc(e.area)}</span>${esc(e.label)}</h2><div class="body">${e.version ? `<h3>${esc(e.version)}</h3>` : ""}${list("New", e.new)}${list("Changes", e.changes)}${list("Fixes", e.fixes)}</div>`;
  return sec;
}
pages.changelog = async () => {
  const pager = $(".cl-pager");
  if (!pager) return;
  try {
    const r = await fetch(API + "/changelog");
    const d = r.ok ? await r.json() : { entries: [] };
    if (d.entries && d.entries.length) {
      const fresh = d.entries.map(e => ({ el: clSection(e), t: Date.parse(e.date + "T00:00:00"), db: 1 }));
      const old = $$(".changelog section[data-k]").map(el => ({ el, t: clDate(el.querySelector("h2").lastChild.textContent.trim()), db: 0 }));
      [...fresh, ...old].sort((a, b) => b.t - a.t || b.db - a.db).forEach(x => pager.before(x.el));
    }
  } catch (e) { /* the shipped entries still show */ }
  const PER = 8, secs = $$(".changelog section[data-k]"), tabs = $$(".cl-tabs button");
  let f = "All", p = 1;
  const render = () => {
    const shown = secs.filter(s => { s.hidden = true; return f === "All" || s.dataset.k === f; });
    const n = Math.ceil(shown.length / PER); if (p > n) p = n || 1;
    shown.slice((p - 1) * PER, p * PER).forEach(s => { s.hidden = false; });
    pager.innerHTML = "";
    if (n > 1) for (let i = 1; i <= n; i++) {
      const a = document.createElement("a"); a.href = "#"; a.textContent = i;
      if (i === p) { a.className = "on"; a.setAttribute("aria-current", "page"); }
      a.addEventListener("click", e => { e.preventDefault(); p = i; render(); $(".changelog").scrollIntoView({ block: "start" }); });
      pager.appendChild(a);
    }
  };
  tabs.forEach(b => b.addEventListener("click", () => { f = b.dataset.f; p = 1; tabs.forEach(x => { x.classList.toggle("on", x === b); x.setAttribute("aria-pressed", x === b); }); render(); }));
  render();
};

/* ---------- /connect/wow: upload FraglinkPlayed.lua by hand, same checks as the app ---------- */
pages.wowupload = async () => {
  const el = $("#wu");
  if (!token()) { location.href = "/login?next=" + encodeURIComponent(location.pathname); return; }
  const plural = (n, one, many) => `${nf.format(n)} ${n === 1 ? one : (many || one + "s")}`;
  const STEPS = [["read", "Reading the file"], ["send", "Sending it securely to Fraglink"], ["check", "Checking signatures, BattleTag and /played history"], ["match", "Matching your Battle.net characters"]];
  const PATHS = {
    wow: "World of Warcraft\\_retail_\\WTF\\Account\\<ACCOUNT>\\SavedVariables\\FraglinkPlayed.lua",
    wow_forever: "World of Warcraft\\_forever_\\WTF\\Account\\<ACCOUNT>\\SavedVariables\\FraglinkPlayed.lua",
  };
  const asked = new URLSearchParams(location.search);
  const raw = (asked.get("version") || asked.get("game") || "retail").toLowerCase();
  let game = raw === "forever" || raw === "wow_forever" ? "wow_forever" : "wow";
  el.innerHTML = `<div class="wu-page">
    <div class="bn-tabs" role="tablist" aria-label="Which version to upload"><button type="button" data-ver="wow">Retail</button><button type="button" data-ver="wow_forever">Forever</button></div>
    <h2 data-title>Upload Retail /played</h2>
    <p class="sub">Log in to each character with the addon on, then /reload or log out so WoW saves the file.</p>
    <div class="wu-paths" data-paths></div>
    <form class="wu-form" data-form>
      <input type="hidden" name="game" value="wow">
      <div class="wu-row"><input type="file" name="f" accept=".lua" required aria-label="FraglinkPlayed.lua"> <button class="btn" type="submit">Upload Retail /played</button></div>
    </form>
    <p class="sub">Got more than one WoW account? Upload each one's file.</p>
  </div>
    <ol class="bv-steps" data-steps hidden>${STEPS.map(([k, t]) => `<li data-s="${k}"><span class="bv-ic"></span><b>${t}</b><small></small></li>`).join("")}</ol>
    <div data-res></div>`;
  const step = (k, state, note) => { const li = $(`[data-s="${k}"]`, el); li.className = state; if (note != null) $("small", li).textContent = note; };
  const res = $("[data-res]", el), form = $("[data-form]", el), steps = $("[data-steps]", el);
  const setGame = next => {
    game = next === "wow_forever" ? "wow_forever" : "wow";
    form.game.value = game;
    const label = game === "wow_forever" ? "Forever" : "Retail";
    $("[data-title]", el).textContent = `Upload ${label} /played`;
    $("button[type=submit]", form).textContent = `Upload ${label} /played`;
    const order = game === "wow_forever" ? ["wow_forever", "wow"] : ["wow", "wow_forever"];
    const names = { wow: "Retail", wow_forever: "Forever" };
    $("[data-paths]", el).innerHTML = order.map(id => `<p class="${id === game ? "on" : ""}"><span>${names[id]}</span><code>${esc(PATHS[id])}</code></p>`).join("");
    $$("[data-ver]", el).forEach(b => { const on = b.dataset.ver === game; b.classList.toggle("on", on); b.setAttribute("aria-selected", on ? "true" : "false"); });
  };
  setGame(game);
  el.addEventListener("click", e => { const b = e.target.closest("[data-ver]"); if (b) setGame(b.dataset.ver); });
  const R = { wow_checksum: "Edited after the game saved it", wow_battletag: "From a different Battle.net account", wow_old_addon: "Addon too old, update it",
    wow_decrease: "/played went down", wow_impossible: "/played grew faster than real time", wow_clock: "Saved with a wrong clock" };
  let rows = [], sortBy = "played", dir = -1, showAll = false, totals = null;
  const CAP = 25;
  const STATUS = { v: [0, "vbadge", "Verified"], s: [1, "srbadge", "Not verified"], r: [2, "badbadge", "Rejected"] };
  const draw = () => {
    if (!totals) return;
    const key = { name: r => r.name.toLowerCase(), realm: r => r.realm.toLowerCase(), level: r => r.level || 0, played: r => r.played_min, status: r => STATUS[r.st][0] }[sortBy];
    const list = rows.slice().sort((a, c) => { const x = key(a), y = key(c); return (x < y ? -1 : x > y ? 1 : 0) * dir || c.played_min - a.played_min; });
    const shown = showAll ? list : list.slice(0, CAP);
    const th = (k, t) => `<th><button type="button" data-sort="${k}" class="wu-sort${sortBy === k ? " on" : ""}">${t}${sortBy === k ? (dir < 0 ? " ▾" : " ▴") : ""}</button></th>`;
    const cls = c => c ? c.charAt(0) + c.slice(1).toLowerCase().replace(/knight$/, " Knight").replace(/hunter$/, " Hunter") : "";
    res.innerHTML = `<h3>Results</h3>
      <div class="wu-tot"><div><b>${hours(totals.verified, { zero: true })}</b><small>Verified</small></div>${totals.unverified ? `<div class="wu-nc"><b>${hours(totals.unverified)}</b><small>Not verified</small></div>` : ""}</div>
      ${rows.length ? `<div class="wu-scroll"><table class="wu-t"><thead><tr>${th("name", "Character")}${th("realm", "Realm")}${th("level", "Class / level")}${th("played", "Hours played")}${th("status", "Status")}</tr></thead><tbody>
      ${shown.map(r => `<tr><td><b>${esc(r.name)}</b></td><td>${esc(r.realm)} <span class="sub">${esc(r.region.toUpperCase())}${r.game_id === "wow_forever" ? " · Forever" : ""}</span></td>
        <td>${esc([cls(r.class), r.level ? "level " + r.level : ""].filter(Boolean).join(" · ")) || `<span class="sub">–</span>`}</td>
        <td class="num">${hours(r.played_min)}${r.chg ? `<br><small class="sub">${esc(r.chg)}</small>` : ""}</td><td><span class="${STATUS[r.st][1]}">${STATUS[r.st][2]}</span>${r.why ? `<br><small class="sub">${esc(r.why)}</small>` : r.st === "s" ? `<br><small class="sub"><a href="/connect/battlenet/verify">Verify WoW characters</a></small>` : ""}</td></tr>`).join("")}</tbody></table></div>
      ${list.length > CAP ? `<p><button class="btn sm" type="button" data-more>${showAll ? "Show fewer" : `Show all ${plural(list.length, "character")}`}</button></p>` : ""}` : `<p class="sub">No characters uploaded yet.</p>`}
      <p><a class="btn" href="/connect/battlenet/verify">Verify characters</a> <a class="btn" href="/settings#wow">Back to World of Warcraft</a></p>`;
  };
  res.addEventListener("click", e => {
    const s = e.target.closest("[data-sort]");
    if (s) { const k = s.dataset.sort; dir = sortBy === k ? -dir : (k === "played" || k === "level" ? -1 : 1); sortBy = k; draw(); return; }
    if (e.target.closest("[data-more]")) { showAll = !showAll; draw(); }
  });
  form.addEventListener("submit", async e => {
    e.preventDefault();
    const f = form.f.files[0]; if (!f) return;
    const btn = $("button[type=submit]", form); btn.disabled = true;
    steps.hidden = false; res.innerHTML = ""; STEPS.forEach(([k]) => step(k, "", ""));
    const fail = (k, x) => { step(k, "bad", x.message || String(x)); btn.disabled = false; };
    step("read", "run");
    if (f.size > 1024 * 1024) return fail("read", new Error("That file is over 1 MB. Pick FraglinkPlayed.lua from SavedVariables."));
    let text;
    try { text = await f.text(); } catch (x) { return fail("read", new Error("Couldn't read that file.")); }
    if (!/FraglinkPlayedDB/.test(text)) return fail("read", new Error("That isn't a FraglinkPlayed.lua file."));
    step("read", "ok", `${f.name} · ${nf.format(Math.ceil(f.size / 1024))} KB`);
    step("send", "run");
    let u;
    try { u = await api("/hours/wow_played/file", { method: "POST", body: { game_id: form.game.value, text } }); }
    catch (x) { return fail(x.code === "no_characters" || x.code === "too_big" ? "read" : "send", x); }
    step("send", "ok", "Received");
    const rej = u.rejected.length, unch = u.unchanged.length;
    const gained = (u.changes || []).reduce((a, c) => a + (c.delta_s || 0), 0);
    step("check", rej ? "warn" : "ok", [plural(u.found, "character") + " in the file", u.accepted ? `${nf.format(u.accepted)} updated` : "", gained > 0 ? `+${(gained / 3600).toFixed(1)} hours since last upload` : u.accepted && u.changes && u.changes.length && !u.changes.some(c => c.first) ? "no new hours since last upload" : "", unch ? `${nf.format(unch)} already up to date` : "", rej ? `${nf.format(rej)} rejected` : ""].filter(Boolean).join(" · "));
    step("match", "run");
    let m;
    try { m = await api("/me/wow"); } catch (x) { return fail("match", x); }
    const game = form.game.value, mine = m.characters.filter(c => c.game_id === game);
    const nv = mine.filter(c => c.verified).length;
    step("match", m.bnet === "linked" ? "ok" : "warn", m.bnet === "none" ? "Battle.net isn't linked." : m.bnet === "reconnect" ? "Reconnect Battle.net to verify characters." : `${nf.format(nv)} of ${plural(mine.length, "character")} verified`);
    const ck = c => [c.region, c.realm, c.name].join("|").toLowerCase();
    const chg = new Map((u.changes || []).map(c => [ck(c), c.first ? "First upload" : c.delta_s > 0 ? `+${(c.delta_s / 3600).toFixed(1)} hours since last upload` : "No change since last upload"]));
    rows = mine.map(c => ({ ...c, st: c.verified ? "v" : "s", chg: chg.get(ck(c)) }))
      .concat(u.rejected.map(r => ({ ...r, game_id: game, st: "r", why: R[r.reason] || r.detail })));
    totals = { verified: mine.filter(c => c.verified).reduce((a, c) => a + c.played_min, 0), unverified: mine.filter(c => !c.verified).reduce((a, c) => a + c.played_min, 0) };
    showAll = false; draw();
    btn.disabled = false; form.reset(); setGame(game);
  });
};

/* ---------- /connect/battlenet/verify: the real verification steps, shown as they run ---------- */
/* ---------- WoW icons (Blizzard render CDN through /api/v1/wow/icons, cached by the server) ---------- */
// Faction comes from Blizzard, never from a character's race or class.
const factionCrest = value => {
  const faction = String(value || "").toUpperCase();
  if (faction !== "HORDE" && faction !== "ALLIANCE") return "";
  const name = faction === "HORDE" ? "Horde" : "Alliance";
  const path = faction === "HORDE"
    ? '<path d="M12 2 8 7l2 3-3 4-2-7-3 6 4 7 4-2 2 4 2-4 4 2 4-7-3-6-2 7-3-4 2-3Zm0 6 2 7-2 3-2-3Z" fill-rule="evenodd"/>'
    : '<path d="M12 1 3 4v8c0 5 4 8 9 11 5-3 9-6 9-11V4Zm-1 4 6 2-2 3 3 2-3 2 1 4-4 2-4-3 2-3-3-1 1-4 3-1Z" fill-rule="evenodd"/>';
  return `<svg class="faction-crest ${faction.toLowerCase()}" viewBox="0 0 24 24" role="img" aria-label="${name}"><title>${name}</title>${path}</svg>`;
};
const groupWowRealms = rows => {
  const groups = new Map();
  for (const c of rows) {
    const realm = (c.realm || "").trim() || "Unknown server", region = (c.region || "").toUpperCase();
    const key = realm.toLowerCase() + "|" + region;
    if (!groups.has(key)) groups.set(key, { realm, region, rows: [] });
    groups.get(key).rows.push(c);
  }
  const realms = [...groups.values()];
  const regions = new Map();
  for (const g of realms) {
    const key = g.realm.toLowerCase();
    if (!regions.has(key)) regions.set(key, new Set());
    regions.get(key).add(g.region);
  }
  return realms.sort((a, b) => b.rows.length - a.rows.length || a.realm.localeCompare(b.realm, "en", { sensitivity: "base" }) || a.region.localeCompare(b.region)).map(g => ({ ...g, showRegion: regions.get(g.realm.toLowerCase()).size > 1 }));
};
const WOW = (() => {
  const S = { WARRIOR: [["Arms", "ability_warrior_savageblow"], ["Fury", "ability_warrior_innerrage"], ["Protection", "ability_warrior_defensivestance"]],
    PALADIN: [["Holy", "spell_holy_holybolt"], ["Protection", "ability_paladin_shieldofthetemplar"], ["Retribution", "spell_holy_auraoflight"]],
    HUNTER: [["Beast Mastery", "ability_hunter_bestialdiscipline"], ["Marksmanship", "ability_hunter_focusedaim"], ["Survival", "ability_hunter_camouflage"]],
    ROGUE: [["Assassination", "ability_rogue_deadlybrew"], ["Outlaw", "ability_rogue_waylay"], ["Subtlety", "ability_stealth"]],
    PRIEST: [["Discipline", "spell_holy_powerwordshield"], ["Holy", "spell_holy_guardianspirit"], ["Shadow", "spell_shadow_shadowwordpain"]],
    DEATHKNIGHT: [["Blood", "spell_deathknight_bloodpresence"], ["Frost", "spell_deathknight_frostpresence"], ["Unholy", "spell_deathknight_unholypresence"]],
    SHAMAN: [["Elemental", "spell_nature_lightning"], ["Enhancement", "spell_shaman_improvedstormstrike"], ["Restoration", "spell_nature_magicimmunity"]],
    MAGE: [["Arcane", "spell_holy_magicalsentry"], ["Fire", "spell_fire_firebolt02"], ["Frost", "spell_frost_frostbolt02"]],
    WARLOCK: [["Affliction", "spell_shadow_deathcoil"], ["Demonology", "spell_shadow_metamorphosis"], ["Destruction", "spell_shadow_rainoffire"]],
    MONK: [["Brewmaster", "spell_monk_brewmaster_spec"], ["Mistweaver", "spell_monk_mistweaver_spec"], ["Windwalker", "spell_monk_windwalker_spec"]],
    DRUID: [["Balance", "spell_nature_starfall"], ["Feral", "ability_druid_catform"], ["Guardian", "ability_racial_bearform"], ["Restoration", "spell_nature_healingtouch"]],
    DEMONHUNTER: [["Havoc", "ability_demonhunter_specdps"], ["Vengeance", "ability_demonhunter_spectank"], ["Devourer", "classicon_demonhunter"]],
    EVOKER: [["Devastation", "classicon_evoker_devastation"], ["Preservation", "classicon_evoker_preservation"], ["Augmentation", "classicon_evoker_augmentation"]] };
  const NAMES = { DEATHKNIGHT: "Death Knight", DEMONHUNTER: "Demon Hunter" };
  const RACE = { "Human": "c:human", "Dwarf": "c:dwarf", "Night Elf": "c:nightelf", "Gnome": "c:gnome", "Draenei": "c:draenei", "Pandaren": "c:pandaren",
    "Orc": "c:orc", "Undead": "c:undead", "Tauren": "c:tauren", "Troll": "c:troll", "Blood Elf": "c:bloodelf", "Goblin": "r:goblin", "Worgen": "r:worgen",
    "Void Elf": "a:voidelf", "Lightforged Draenei": "a:lightforgeddraenei", "Dark Iron Dwarf": "a:darkirondwarf", "Mechagnome": "a:mechagnome",
    "Nightborne": "a:nightborne", "Highmountain Tauren": "a:highmountaintauren", "Mag'har Orc": "a:magharorc", "Zandalari Troll": "a:zandalaritroll",
    "Vulpera": "a:vulpera", "Kul Tiran": "x:ability_racial_childofthesea", "Dracthyr": "x:ability_racial_wingbuffet" };
  const url = n => `/api/v1/wow/icons/${n}.jpg`;
  const key = c => String(c || "").toUpperCase().replace(/[^A-Z]/g, "");
  return {
    url, specs: c => S[key(c)] || [],
    cls: c => NAMES[key(c)] || (c ? key(c).charAt(0) + key(c).slice(1).toLowerCase() : ""),
    color: c => ({ MAGE: "#69ccf0", HUNTER: "#abd473", WARRIOR: "#c79c6e", ROGUE: "#fff569", PRIEST: "#ffffff", PALADIN: "#f58cba", DRUID: "#ff7d0a", WARLOCK: "#9482c9", SHAMAN: "#0070de", MONK: "#00ff96", DEATHKNIGHT: "#c41f3b", DEMONHUNTER: "#a330c9", EVOKER: "#33937f" }[key(c)] || "#8a939e"),
    clsIcon: c => key(c) ? url("classicon_" + key(c).toLowerCase()) : "",
    specIcon: (c, s) => { const x = (S[key(c)] || []).find(p => p[0] === s); return x ? url(x[1]) : ""; },
    raceIcon: (race, gender, c) => {
      const g = gender === "female" ? "female" : "male", r = race === "Pandaren" && g === "male" ? "x:pandarenracial_quiveringpain" : RACE[race];
      if (!r) return key(c) ? url("classicon_" + key(c).toLowerCase()) : "";
      const [t, n] = r.split(":");
      return url(t === "c" ? `achievement_character_${n}_${g}` : t === "r" ? `race_${n}_${g}` : t === "a" ? `achievement_alliedrace_${n}` : n);
    },
  };
})();
const imgOk = "data-hide-broken";

/* ---------- /connect/battlenet: the Battle.net account, verified WoW characters, spec picks, hide toggles ---------- */
pages.bnet = () => wowManager($("#bn"));
async function wowManager(el) {
  if (!el) return;
  if (!token()) { location.href = "/login?next=" + encodeURIComponent(location.pathname + location.search + location.hash); return; }
  const embedded = el.dataset.embed === "1";
  const Hx = embedded ? "h3" : "h2";
  const Sec = embedded ? "bn-sec" : "panel bn-box";
  const b = BRAND.bnet, logo = s => `<svg viewBox="0 0 24 24" width="${s}" height="${s}" fill="${b[1]}" aria-hidden="true"><path d="${b[2]}"/></svg>`;
  let s;
  try { s = await api("/me/bnet"); } catch (x) { el.innerHTML = `<p class="empty" style="padding:30px 0">${esc(x.message)}</p>`; return; }
  const ago = iso => { const m = Math.round((Date.now() - new Date(iso)) / 60000); return m < 1 ? "just now" : m < 60 ? plural(m, "minute") + " ago" : m < 1440 ? plural(Math.round(m / 60), "hour") + " ago" : plural(Math.round(m / 1440), "day") + " ago"; };
  const show = s.profile_show === "wow" || s.profile_show === "wow_forever" || s.profile_show === "none" ? s.profile_show : "both";
  const def = s.default_flavour === "wow_forever" ? "wow_forever" : "wow";
  const handle = (me() || {}).handle || "";
  const flavSeg = `<div class="bn-def"><span class="bn-k">Show on profile</span><div class="seg" role="radiogroup" aria-label="Show on profile"><button type="button" data-show="wow" aria-pressed="${show === "wow"}">Retail only</button><button type="button" data-show="wow_forever" aria-pressed="${show === "wow_forever"}">Forever only</button><button type="button" data-show="both" aria-pressed="${show === "both"}">Both</button><button type="button" data-show="none" aria-pressed="${show === "none"}" title="Hides WoW from your profile.">None</button></div><div data-opener ${show === "both" ? "" : "hidden"}><span class="bn-k">Profile opens on</span><div class="seg" role="radiogroup" aria-label="Profile opens on"><button type="button" data-def="wow" aria-pressed="${def === "wow"}">Retail</button><button type="button" data-def="wow_forever" aria-pressed="${def === "wow_forever"}">Forever</button></div></div></div>`;
  const head = s.linked
    ? `<div class="bn-acct"><span class="bn-av">${logo(26)}</span><div><div class="bn-nm">${esc(s.battletag)} ${s.needs_reconnect ? `<span class="srbadge">Reconnect</span>` : `<span class="vbadge">Verified</span>`}</div>
        <div class="sub">${[s.linked_at ? "Linked " + fullDate.format(new Date(s.linked_at)) : "", s.regions.length ? s.regions.map(r => r.toUpperCase()).join(", ") : "", s.last_verified ? "checked " + ago(s.last_verified) : ""].filter(Boolean).join(" · ")}</div></div>
        <div class="bn-acts"><a class="btn" href="/connect/battlenet/verify">Check account</a><button class="btn bn-unlink" type="button" data-unlink>Unlink</button></div></div>`
    : `<div class="bn-acct"><span class="bn-av">${logo(26)}</span><div><div class="bn-nm">Not linked</div><div class="sub">Link Battle.net to verify Retail characters.</div></div>
        <div class="bn-acts"><button class="btn" type="button" data-link>Link Battle.net</button></div></div>`;
  el.innerHTML = `<section class="${Sec}"><${Hx}>Battle.net</${Hx}><div data-msg></div>${head}</section>
    <section class="${Sec}"><${Hx}>Profile page display${handle ? `<a class="bn-view" href="/u/${encodeURIComponent(handle)}">View profile</a>` : ""}</${Hx}>
      ${flavSeg}${s.linked ? `<div data-feat></div>` : ""}</section>
    <section class="${Sec}"><div class="bn-tabs" data-tabs role="tablist"></div></section>
    <section class="${Sec}"><${Hx}>Add characters</${Hx}>
      <div class="bn-addblock"><div data-addbox></div><div class="bn-upload"><div><b>Upload /played</b><p class="sub">From the Fraglink /played addon.</p></div><div class="bn-upload-actions"><a class="btn" data-upload href="/connect/wow?version=${def === "wow_forever" ? "forever" : "retail"}">Upload ${def === "wow_forever" ? "Forever" : "Retail"} /played</a><a class="sub" href="https://www.curseforge.com/wow/addons/fraglink-played" target="_blank" rel="noopener">Get the addon</a></div></div></div></section>
    <section class="${Sec}"><${Hx}>Your characters</${Hx}>
      <div class="bn-tools"><input type="search" data-q placeholder="Search" aria-label="Search name or realm" maxlength="64">
        <select data-sort aria-label="Sort"><option value="hours">Hours</option><option value="level">Level</option><option value="name">Name</option><option value="realm">Realm</option></select>
        <select data-class aria-label="Class"><option value="">Class</option></select>
        <select data-vis aria-label="Shown or hidden"><option value="">Shown and hidden</option><option value="shown">Shown</option><option value="hidden">Hidden</option></select>
        <label class="bn-sw"><span>Hide all</span><input type="checkbox" data-hideall ${s.hide_all ? "checked" : ""}><i class="bn-tg"></i></label></div>
      <div class="bn-headrow" aria-hidden="true"><span></span><span>Name</span><span class="c-realm">Realm</span><span class="c-cls">Level / class</span><span class="c-hrs">Hours</span><span class="c-imp">Imported</span><span></span></div>
      <div data-list><p class="empty bn-empty">Loading…</p></div>
      <div class="bn-more"><span class="sub" data-cnt></span><button class="btn" type="button" data-more hidden>Show more</button></div></section>
    <section class="${Sec}" data-review hidden><${Hx}>Held for review</${Hx}><div class="bn-review" data-rev></div></section>`;
  const msgBox = $("[data-msg]", el), list = $("[data-list]", el), more = $("[data-more]", el), cnt = $("[data-cnt]", el);
  const st = { q: "", sort: "hours", class: "", vis: "", items: [], data: null, seq: 0, game: def, extra: "" };
  const regions = `<select name="region" aria-label="Region"><option value="us">US</option><option value="eu">EU</option><option value="kr">KR</option><option value="tw">TW</option></select>`;
  const addForm = game => game === "wow_forever"
    ? `<form class="bn-add" data-addchar>${regions}<input name="first" placeholder="First name" aria-label="First name" required maxlength="32"><input name="surname" placeholder="Surname" aria-label="Surname" required maxlength="32"><button class="btn" type="submit">Add character</button></form>`
    : `<form class="bn-add" data-addchar>${regions}<input name="realm" placeholder="Realm" aria-label="Realm" required maxlength="64"><input name="name" placeholder="Name" aria-label="Character name" required maxlength="24"><button class="btn" type="submit">Add character</button></form>`;
  const row = c => {
    const hid = c.hidden, specs = WOW.specs(c.class);
    const was = [c.former_name && c.former_name.toLowerCase() !== c.name.toLowerCase() ? c.former_name : "", c.former_realm && c.former_realm !== c.realm ? c.former_realm : ""].filter(Boolean).join(" · ");
    const cls = [Number(c.level) > 0 ? "Level " + c.level : "", WOW.cls(c.class)].filter(Boolean).join(" ");
    const imp = c.played_min > 0 && c.uploaded_at ? ago(c.uploaded_at) : "—";
    return `<div class="bn-row${hid ? " hid" : ""}" data-id="${c.id}"><img src="${WOW.raceIcon(c.race, c.gender, c.class)}" alt="" ${imgOk}>
      <div class="bn-nm">${esc(c.name)}${factionCrest(c.faction)}${c.verified ? `<span class="bn-tick" title="On the linked Battle.net account">✓</span>` : ""}${was ? `<div class="sub">formerly ${esc(was)}</div>` : ""}</div>
      <div class="c-realm sub">${c.realm ? esc(c.realm) + " " : ""}${esc((c.region || "").toUpperCase())}</div>
      <div class="c-cls">${esc(cls || "—")}</div>
      <div class="c-hrs num">${hours(c.played_min, { zero: true })}</div>
      <div class="c-imp sub">${imp}</div>
      <div class="bn-menu"><button type="button" class="bn-morebtn" data-menu aria-label="Actions for ${esc(c.name)}">···</button><div class="bn-pop" hidden>
        <label class="bn-sw"><span>${hid ? "Hidden" : "Shown"}</span><input type="checkbox" data-hide ${hid ? "" : "checked"} ${st.data && st.data.hide_all ? "disabled" : ""}><i class="bn-tg"></i></label>
        ${c.game_id === "wow" ? `<label class="bn-sw"><span>Guild</span><input type="checkbox" data-guild ${c.show_guild ? "checked" : ""}><i class="bn-tg"></i></label>` : ""}
        ${specs.length ? `<div class="bn-specs" role="group" aria-label="Spec">${specs.map(p => `<button type="button" class="${p[0] === c.spec ? "on" : ""}" data-spec="${esc(p[0])}">${esc(p[0])}</button>`).join("")}</div>` : ""}
      </div></div></div>`;
  };
  const render = () => {
    const d = st.data, label = { wow: "Retail", wow_forever: "Forever" };
    $("[data-tabs]", el).innerHTML = ["wow", "wow_forever"].map(id => `<button type="button" role="tab" data-tab="${id}" aria-selected="${id === st.game}" class="${id === st.game ? "on" : ""}">${label[id]}</button>`).join("");
    $("[data-addbox]", el).innerHTML = addForm(st.game);
    const up = $("[data-upload]", el); if (up) { up.href = "/connect/wow?version=" + (st.game === "wow_forever" ? "forever" : "retail"); up.textContent = "Upload " + (st.game === "wow_forever" ? "Forever" : "Retail") + " /played"; }
    const filtered = d && d.matched === 0 && (st.q || st.class || st.vis);
    list.innerHTML = st.items.length ? st.items.map(row).join("")
      : `<p class="empty bn-empty">${filtered ? "No characters match." : "No characters yet. Add one above, or upload a /played file."}</p>`;
    cnt.textContent = d && d.matched ? plural(d.matched, "character") : "";
    const left = d ? d.matched - st.items.length : 0;
    more.hidden = left <= 0; more.textContent = "Show more";
  };
  const load = async append => {
    const seq = ++st.seq, p = new URLSearchParams({ q: st.q, sort: st.sort, class: st.class, vis: st.vis, game: st.game, offset: append ? st.items.length : 0, limit: 25 });
    let d; try { d = await api("/me/wow/list?" + p); } catch (x) { if (seq === st.seq) showErr(msgBox, x); return; }
    if (seq !== st.seq) return;
    if (!st.data) { const cs = $("[data-class]", el); d.classes.slice().sort((a, b) => WOW.cls(a).localeCompare(WOW.cls(b))).forEach(c => cs.insertAdjacentHTML("beforeend", `<option value="${esc(c)}">${esc(WOW.cls(c))}</option>`)); }
    st.data = d; st.items = append ? st.items.concat(d.items) : d.items; render();
  };
  let tq; $("[data-q]", el).oninput = e => { clearTimeout(tq); tq = setTimeout(() => { st.q = e.target.value.trim(); load(false); }, 250); };
  [["sort", "[data-sort]"], ["class", "[data-class]"], ["vis", "[data-vis]"]].forEach(([k, sel]) => { $(sel, el).onchange = e => { st[k] = e.target.value; load(false); }; });
  more.onclick = () => load(true);
  $("[data-hideall]", el).onchange = async e => {
    const on = e.target.checked; e.target.disabled = true;
    try { await api("/me/wow/hide_all", { method: "POST", body: { hide_all: on } }); await load(false); }
    catch (x) { e.target.checked = !on; showErr(msgBox, x); } finally { e.target.disabled = false; }
  };
  list.addEventListener("change", async e => {
    const gb = e.target.closest("[data-guild]");
    if (gb) { const c = st.items.find(x => x.id === gb.closest(".bn-row").dataset.id), on = gb.checked; gb.disabled = true;
      try { await api(`/me/wow/${c.id}/guild`, { method: "POST", body: { show_guild: on } }); c.show_guild = on; }
      catch (x) { gb.checked = !on; showErr(msgBox, x); } finally { gb.disabled = false; } return; }
    const cb = e.target.closest("[data-hide]"); if (!cb) return;
    const r = cb.closest(".bn-row"), c = st.items.find(x => x.id === r.dataset.id), hidden = !cb.checked; cb.disabled = true;
    try { await api("/me/wow/" + c.id, { method: "POST", body: { hidden } }); c.hidden = c.own_hidden = hidden;
      if (hidden) el.dispatchEvent(new CustomEvent("wowhid", { detail: c.id }));
      if (st.vis) await load(false); else { r.outerHTML = row(c); } }
    catch (x) { cb.checked = !cb.checked; showErr(msgBox, x); cb.disabled = false; }
  });
  list.addEventListener("click", async e => {
    const bt = e.target.closest("[data-spec]"); if (!bt) return;
    const r = bt.closest(".bn-row"), c = st.items.find(x => x.id === r.dataset.id); bt.disabled = true;
    try { const x = await api(`/me/wow/${c.id}/spec`, { method: "POST", body: { spec: bt.dataset.spec } }); c.spec = x.spec; c.spec_picked = true; r.outerHTML = row(c); }
    catch (x) { bt.disabled = false; showErr(msgBox, x); }
  });
  el.addEventListener("submit", async e => {
    const f = e.target.closest("[data-addchar]"); if (!f) return;
    e.preventDefault();
    const btn = $("button[type=submit]", f); btn.disabled = true;
    try {
      const body = f.surname ? { region: f.region.value, game: "wow_forever", name: (f.first.value.trim() + " " + f.surname.value.trim()).replace(/\s+/g, " ") } : { region: f.region.value, realm: f.realm.value.trim(), name: f.name.value.trim(), game: "wow" };
      const r = await api("/me/wow/armory", { method: "POST", body });
      showOk(msgBox, r.verified ? `${r.name} added.` : `${r.name} added.`);
      if (f.name) f.name.value = "";
      if (f.first) f.first.value = "";
      if (f.surname) f.surname.value = "";
      await load(false);
    } catch (x) { showErr(msgBox, x); }
    finally { btn.disabled = false; }
  });
  el.addEventListener("click", async e => {
    const sh = e.target.closest("[data-show]");
    if (sh) {
      const showTo = sh.dataset.show;
      $$("[data-show]", el).forEach(b => { b.disabled = true; });
      try {
        await api("/me/wow/profile_show", { method: "POST", body: { show: showTo } });
        $$("[data-show]", el).forEach(b => b.setAttribute("aria-pressed", b === sh ? "true" : "false"));
        const opener = $("[data-opener]", el); if (opener) opener.hidden = showTo !== "both";
        if (el._feat) el._feat();
      } catch (x) { showErr(msgBox, x); }
      finally { $$("[data-show]", el).forEach(b => { b.disabled = false; }); }
      return;
    }
    const df = e.target.closest("[data-def]");
    if (df) {
      const flavour = df.dataset.def;
      $$("[data-def]", el).forEach(b => { b.disabled = true; });
      try {
        await api("/me/wow/default_flavour", { method: "POST", body: { flavour } });
        $$("[data-def]", el).forEach(b => b.setAttribute("aria-pressed", b === df ? "true" : "false"));
      } catch (x) { showErr(msgBox, x); }
      finally { $$("[data-def]", el).forEach(b => { b.disabled = false; }); }
      return;
    }
    const tab = e.target.closest("[data-tab]");
    if (tab) { st.game = tab.dataset.tab; load(false); return; }
    const menuBtn = e.target.closest("[data-menu]");
    if (menuBtn) {
      const pop = menuBtn.nextElementSibling;
      $$(".bn-pop", el).forEach(p => { if (p !== pop) p.hidden = true; });
      pop.hidden = !pop.hidden;
      return;
    }
    if (!e.target.closest(".bn-pop")) $$(".bn-pop", el).forEach(p => p.hidden = true);
    const t = e.target.closest("[data-unlink],[data-link]"); if (!t) return;
    try {
      if (t.matches("[data-link]")) { t.disabled = true; const r = await api("/connect/bnet/start", { method: "POST" }); location.href = r.url; return; }
      if (!confirm("Unlink Battle.net? You can link it again later.")) return;
      t.disabled = true; await api("/connect/bnet", { method: "DELETE" }); location.reload();
    } catch (x) { t.disabled = false; showErr(msgBox, x); }
  });
  load(false);
  api("/me/wow/review").then(d => {
    const box = $("[data-review]", el); if (!box || !d.items || !d.items.length) return;
    $("[data-rev]", box).innerHTML = d.items.map(i => `<div class="sub" style="padding:4px 0"><b>${esc(i.detail || i.reason)}</b></div>`).join("");
    box.hidden = false;
  }).catch(() => {});
  // Featured top 3 per WoW version: search picker (fine with 1000+ characters), up/down, remove
  const fbox = $("[data-feat]", el);
  if (fbox) {
    let F; try { F = await api("/me/wow/featured"); } catch (x) { fbox.innerHTML = `<p class="empty">${esc(x.message)}</p>`; return; }
    const lbl = c => esc(c.name);
    const shownGames = () => {
      const on = $("[data-show][aria-pressed=true]", el);
      const which = on ? on.dataset.show : show;
      if (which === "wow") return [["wow", "Retail"]];
      if (which === "wow_forever") return [["wow_forever", "Forever"]];
      return [["wow", "Retail"], ["wow_forever", "Forever"]];
    };
    const draw = () => { fbox.innerHTML = `<div class="bn-feat"><span class="bn-k">First on profile</span>${shownGames().map(([g, nm]) => `<div class="bn-featrow" data-g="${g}"><span class="bn-k">${nm}</span><span class="bn-featlist">${(F[g] || []).map((c, i) => `<span data-i="${i}">${lbl(c)} <button type="button" data-up ${i ? "" : "disabled"}>Up</button><button type="button" data-dn ${i < F[g].length - 1 ? "" : "disabled"}>Down</button><button type="button" data-rm>Remove</button></span>`).join("") || `<span class="sub">Most played</span>`}</span>${(F[g] || []).length < 3 ? `<input type="search" data-fq placeholder="Search" aria-label="Search a ${nm} character to show first" maxlength="64"><div data-fr></div>` : ""}</div>`).join("")}</div>`; };
    const save = async (g, next) => {
      try { await api("/me/wow/featured", { method: "POST", body: { game_id: g, ids: next.map(c => c.id) } }); F[g] = next; draw(); }
      catch (x) { showErr(msgBox, x); }
    };
    draw();
    el._feat = draw;
    el.addEventListener("wowhid", e => { ["wow", "wow_forever"].forEach(g => { if (F[g]) F[g] = F[g].filter(c => c.id !== e.detail); }); draw(); });
    let tf;
    fbox.addEventListener("input", e => {
      const q = e.target.closest("[data-fq]"); if (!q) return;
      const box = q.closest("[data-g]"), g = box.dataset.g, out = $("[data-fr]", box);
      clearTimeout(tf); tf = setTimeout(async () => {
        if (!q.value.trim()) { out.innerHTML = ""; return; }
        const p = new URLSearchParams({ q: q.value.trim(), vis: "shown", sort: "hours", limit: 50 });
        let d; try { d = await api("/me/wow/list?" + p); } catch (x) { showErr(msgBox, x); return; }
        const have = new Set(F[g].map(c => c.id)), hits = d.items.filter(c => c.game_id === g && !have.has(c.id)).slice(0, 8);
        out._hits = hits;
        out.innerHTML = hits.length ? hits.map((c, i) => `<button class="btn sm" type="button" data-add="${i}" style="display:block;margin:4px 0;text-align:left">${lbl(c)}</button>`).join("") : `<p class="sub">No shown characters match.</p>`;
      }, 250);
    });
    fbox.addEventListener("click", e => {
      const b = e.target.closest("button"); if (!b) return;
      const box = b.closest("[data-g]"), g = box.dataset.g, cur = F[g].slice();
      if (b.matches("[data-add]")) { const c = $("[data-fr]", box)._hits[+b.dataset.add]; return save(g, cur.concat([c]).slice(0, 3)); }
      const i = +b.closest("[data-i]").dataset.i;
      if (b.matches("[data-rm]")) cur.splice(i, 1);
      else { const j = b.matches("[data-up]") ? i - 1 : i + 1; [cur[i], cur[j]] = [cur[j], cur[i]]; }
      save(g, cur);
    });
  }
};

pages.bnetverify = async () => {
  const el = $("#bv");
  if (!token()) { location.href = "/login?next=" + encodeURIComponent(location.pathname); return; }
  const b = BRAND.bnet, logo = `<svg viewBox="0 0 24 24" width="34" height="34" fill="${b[1]}" aria-hidden="true"><path d="${b[2]}"/></svg>`;
  const STEPS = [["connect", "Connecting to Battle.net"], ["fetch", "Fetching your characters"], ["match", "Matching /played data"], ["sig", "Checking signatures"]];
  el.innerHTML = `<div class="bv-head"><span class="bv-shield">${logo}<i class="bv-lock">🔒</i></span><div><h2>Verify WoW characters</h2>
    <p class="sub">Checks which characters are on the Battle.net account you linked.</p></div></div>
    <ol class="bv-steps">${STEPS.map(([k, t]) => `<li data-s="${k}"><span class="bv-ic"></span><b>${t}</b><small></small></li>`).join("")}</ol>
    <div data-res></div><div data-act class="bv-act"></div>`;
  const step = (k, state, note) => { const li = $(`[data-s="${k}"]`, el); li.className = state; if (note != null) $("small", li).textContent = note; };
  const act = $("[data-act]", el), res = $("[data-res]", el);
  const fail = (k, x) => { step(k, "bad", x.message || String(x)); act.innerHTML = `<button class="btn blue" type="button" data-again>Try again</button> <a class="btn" href="/connect/battlenet">Back to Battle.net</a>`; };
  const reconnect = (why) => {
    step("connect", "bad", why);
    act.innerHTML = `<button class="btn blue" type="button" data-go>${logo.replace(/34/g, "16")} Connect Battle.net</button><p class="sub">You'll sign in on Battle.net's own page and allow “World of Warcraft profile”. Fraglink never sees your password.</p>`;
  };
  act.onclick = async e => {
    if (e.target.closest("[data-again]")) { location.href = location.pathname; return; }
    const g = e.target.closest("[data-go]"); if (!g) return; g.disabled = true;
    try { const r = await api("/connect/bnet/start", { method: "POST" }); location.href = r.url; } catch (x) { g.disabled = false; fail("connect", x); }
  };
  // 1. a fresh Battle.net sign-in (with wow.profile) leaves a short-lived token for this
  step("connect", "run");
  let f;
  try { f = await api("/wow/verify/fetch", { method: "POST" }); } catch (x) { return fail("connect", x); }
  if (f.need_connect) return reconnect(new URLSearchParams(location.search).get("connected") ? "Battle.net didn't grant access to your WoW characters." : "Sign in to Battle.net to start.");
  history.replaceState(null, "", location.pathname);
  step("connect", "ok", "Connected");
  // 2. the character list came back with the fetch call
  step("fetch", "ok", `${f.characters} character${f.characters === 1 ? "" : "s"} on your account`);
  // 3. match against the uploaded /played entries
  step("match", "run");
  let m;
  try { m = await api("/wow/verify/match", { method: "POST" }); } catch (x) { return fail("match", x); }
  step("match", "ok", m.uploaded ? `${m.verified} of ${m.uploaded} uploaded characters matched` : "No /played data yet. Install the Fraglink /played addon from CurseForge (curseforge.com/projects/1766775).");
  // 4. signatures
  step("sig", "run");
  let s;
  try { s = await api("/wow/verify/signatures"); } catch (x) { return fail("sig", x); }
  step("sig", s.refused.length ? "warn" : "ok", `${s.signed_ok} signed entr${s.signed_ok === 1 ? "y" : "ies"} valid${s.refused.length ? ` · ${s.refused.length} refused` : ""}`);
  const me_ = await api("/me/wow");
  const R = { wow_checksum: "edited after the game wrote it", wow_battletag: "from a different Battle.net account", wow_old_addon: "addon too old" };
  res.innerHTML = `<h3>Results</h3><ul class="bv-res">${me_.characters.map(c => `<li><span class="${c.verified ? "vbadge" : "srbadge"}">${c.verified ? "✓ Verified" : "◆ Not verified, not counted"}</span>
      <b>${esc(c.name)}</b> <span class="sub">${esc(c.realm)} · ${esc(c.region.toUpperCase())}${c.game_id === "wow_forever" ? " · Forever" : ""} · ${Math.floor(c.played_min / 60)}h</span></li>`).join("") || `<li class="sub">No characters uploaded yet.</li>`}
    ${s.refused.map(r => `<li><span class="badbadge">✕ Refused</span> <b>${esc(r.character)}</b> <span class="sub">${esc(R[r.reason] || r.reason)}</span></li>`).join("")}</ul>
    <p class="sub">Verified means the character is on your linked Battle.net account.</p>`;
  act.innerHTML = `<a class="btn" href="/connect/battlenet">Done</a>`;
};

/* ---------- Home V4.2 (signed out: hero + carousel; signed in: your week + friends) ----------
   Your hours, the daily bars, the per-game rows and the friends leaderboard all come from one
   server call (/home/week), so they always use the same 7 days and time zone and add up. */
const tFmt = new Intl.DateTimeFormat("en-AU", { hour: "numeric", minute: "2-digit", timeZone: ZONE });
const h1 = t => (Math.max(0, t || 0) / 10).toFixed(1) + " h";
const hm = s => { s = Math.max(0, Math.round(s || 0)); const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60); return h ? `${h}h${m ? " " + String(m).padStart(2, "0") + "m" : ""}` : `${Math.max(1, m)}m`; };
const ago42 = iso => { const d = dayLabel(iso); return `${d === "Today" || d === "Yesterday" ? d.toLowerCase() : d}, ${tFmt.format(new Date(iso))}`; };
/* Bar tooltip: pinned to the bar (not the mouse), kept inside the chart box so it never covers the
   panel heading or spills out of the chart; hover, keyboard focus and tap all show it. */
function wkTips(wk) {
  const place = d => { const t = d.querySelector(".tip"), b = d.querySelector("i"); if (!t || !b) return;
    const W = wk.clientWidth, tw = t.offsetWidth, th = t.offsetHeight, x = d.offsetLeft + d.offsetWidth / 2 - tw / 2;
    t.style.left = Math.round(Math.max(0, Math.min(W - tw, x))) + "px";
    t.style.top = Math.round(Math.max(0, b.offsetTop - th - 4)) + "px"; };
  $$(".d", wk).forEach(d => { d.addEventListener("mouseenter", () => place(d)); d.addEventListener("focus", () => place(d));
    d.addEventListener("click", () => { const on = !d.classList.contains("tipon"); $$(".d.tipon", wk).forEach(x => x.classList.remove("tipon")); if (on) { place(d); d.classList.add("tipon"); } }); });
  document.addEventListener("click", e => { if (!wk.contains(e.target)) $$(".d.tipon", wk).forEach(x => x.classList.remove("tipon")); });
}
async function loadPoll42() {
  const box = document.getElementById("poll42"); if (!box) return;
  let d; try { d = await api("/poll"); } catch (e) { d = null; }
  const p = d && d.poll, sec = box.closest(".f2p");
  if (!p) { sec.remove(); const two = $("#h42-two"); if (two && two.children.length === 1) two.classList.add("one"); return; }
  const can = d.signed_in && !p.closed;
  box.innerHTML = `<b class="q">${esc(p.question)}</b>${p.options.map(o => `<div class="opt"><label><input type="radio" name="fp" value="${o.id}" ${p.my_vote === o.id ? "checked" : ""} ${can ? "" : "disabled"}> ${esc(o.label)}</label><div class="bar"><i style="width:${o.pct}%"></i></div><span>${o.pct}%</span></div>`).join("")}
    <div class="go">${can ? `<a href="#" id="fp-vote">${p.my_vote ? "Change vote" : "Vote"}</a>` : p.closed ? `<span class="dimt">Poll closed</span>` : `<a href="/login?next=/">Sign in to vote</a>`}</div>`;
  const vb = document.getElementById("fp-vote");
  if (vb) vb.onclick = async e => { e.preventDefault();
    const c = box.querySelector("input[name=fp]:checked"); if (!c || vb.dataset.busy) return; vb.dataset.busy = 1;
    try { await api("/poll/vote", { method: "POST", body: { option_id: Number(c.value) } }); loadPoll42(); } catch (x) { delete vb.dataset.busy; alert(x.message); } };
}
/* Home: Friends' activity shows 5 rows; with two columns, up to 10, as many as end level with
   the right column (Friends online + leaderboard). The panel itself stretches to the bottom. */
function fitAct42() {
  const p = document.querySelector("#h42-in .hub .o3"); if (!p) return;
  const rows = $$(".feed .act", p), days = $$(".feed .day", p), L = p.parentElement, R = L.nextElementSibling;
  const sync = () => { days.forEach(d => { let n = d.nextElementSibling, any = false;
      while (n && !n.classList.contains("day")) { if (!n.hidden) { any = true; break; } n = n.nextElementSibling; }
      d.hidden = !any; });
    const vis = rows.filter(r => !r.hidden); rows.forEach(r => r.classList.toggle("lv", r === vis[vis.length - 1])); };
  rows.forEach((r, i) => { r.hidden = i >= 5; });
  const two = R && R.lastElementChild && getComputedStyle(L).display === "flex";
  if (two) { p.style.flex = "none";
    const end = R.lastElementChild.getBoundingClientRect().bottom;
    for (let i = 5; i < rows.length; i++) { rows[i].hidden = false; sync();
      if (p.getBoundingClientRect().bottom > end + 0.5) { rows[i].hidden = true; break; } }
    p.style.flex = ""; }
  sync();
}
pages.home = async () => {
  fillDownload();
  const u = token() ? me() : null;
  if (u) { $("#h42-out").hidden = true; $("#h42-crumb").textContent = "Your Fraglink"; }
  latest().then(l => { if (!l || !l.version) return; $("#h42-ver").textContent = l.version; $("#h42-date").textContent = l.date ? " · " + fullDay(l.date) : ""; $("#h42-rel").hidden = false; }).catch(() => {});
  let h;
  try { h = await api("/home?tz=" + encodeURIComponent(TZ)); } catch (e) { return; }
  let wk = null, fl = null, act = null;
  if (u) [wk, fl, act] = await Promise.all([
    api("/home/week?tz=" + encodeURIComponent(TZ)).catch(() => null),
    api("/friends/page?show=online&limit=10").catch(() => null),
    api("/friends/activity?per=session&tz=" + encodeURIComponent(TZ)).catch(() => null)]);
  const ids = h.most_played.map(g => g.game_id);
  if (fl) fl.friends.forEach(f => f.presence && ids.push(f.presence.game_id));
  if (act) act.items.forEach(i => ids.push(i.game_id));
  await loadIcons(ids);
  const s = h.stats, prof = n => `/u/${encodeURIComponent(n.handle)}`, nm = n => esc(n.display_name || n.handle);
  const panel = (title, body, more, cls) => `<section class="panel f2p${cls ? " " + cls : ""}"><h2>${title}${more || ""}</h2>${body}</section>`;
  const gname = (id, n) => isUnlisted(id) ? "a game" : esc(n);
  // ---- crumb + community counters (Admin > Site settings: C1 header counters, C4 bar, off)
  const dg = (n, c) => `<span class="d${c ? " " + c : ""}">${nf.format(n).split("").map(ch => /\d/.test(ch) ? `<i>${ch}</i>` : `<i class="c">${esc(ch)}</i>`).join("")}</span>`;
  const cs = h.counts_style || "off";
  let top = `<div class="crumb"><div class="wrap"><span>You are here: <a href="/">Home</a> » ${u ? "Your Fraglink" : "Welcome to Fraglink"}</span><span class="sp"></span>${cs === "c1" ? `<span class="odo"><span class="l">Registered<br>players</span>${dg(s.registered)}<span class="l">Online<br>now</span>${dg(s.online, "g")}</span>` : ""}</div></div>`;
  if (cs === "c4") top = `<div class="hbar"><div class="wrap"><span>⚡ <b>${nf.format(s.registered)}</b> registered player${s.registered === 1 ? "" : "s"}</span><span class="on">● <b>${nf.format(s.online)}</b> online now</span><span>🎮 <b>${nf.format(s.in_game)}</b> in game</span></div></div>` + top;
  $("#f2-top").innerHTML = top;
  // ---- signed in: your week + recent games + friends
  if (u) {
    let inner = "";
    if (wk) {
      const mx = Math.max(0, ...wk.days.map(d => d.seconds));
      const bars = wk.days.map(d => { const day = `${d.label}${d.today ? " (today)" : ""}`;
        return `<div class="d" tabindex="0" data-day="${esc(day)}"><span class="tip">${esc(day)} · <small>${d.seconds ? `${h1(d.tenths)} (${d.hm})` : "no games"}</small></span>${d.seconds ? `<i style="height:${Math.max(2, Math.round(d.seconds / mx * 36))}px"></i>` : `<i class="z"></i>`}${esc(d.dow)}</div>`; }).join("");
      const tg = wk.top_game;
      inner += panel("Your Fraglink", `<div class="mebar m42" id="me42"><div><div class="k">Welcome back</div><h1>${nm(u)}</h1></div>
        <div><b class="big" id="hw">${h1(wk.tenths)}</b><span>Hours this week</span></div>
        <div><b class="big" id="mp">${tg ? esc(tg.name) : "—"}</b><span id="mps">${tg ? `Most played this week · ${h1(tg.tenths)}` : "Nothing played this week yet"}</span></div>
        <div class="ch"><div class="wk42${wk.seconds ? "" : " empty"}" id="wk42">${bars}<div class="none">No games tracked this week yet</div></div></div></div>`, ` <a class="more" href="${prof(u)}">My profile »</a>`);
    }
    // Recent games: this week's games, most recent first; the rows add up to "Hours this week".
    let rg = "";
    if (wk && wk.games.length) {
      const g = wk.games, show = g.length > 4 ? g.slice(0, 3) : g, rest = g.slice(show.length);
      const art = (id, n) => `<span class="a" style="background:${artBg(id, n)}"></span>`;
      rg = show.map(x => `<div class="rg">${art(x.game_id, x.name)}<div class="w"><b>${gname(x.game_id, x.name)}</b><small>Last played ${ago42(x.last_played_at)}</small></div><div class="h">${h1(x.tenths)}<small>this week</small></div></div>`).join("");
      if (rest.length) rg += `<div class="rg more"><span class="a" style="background:${artGrad("other")}"></span><div class="w"><b>${rest.length} other games</b><small>Last played ${ago42(rest[0].last_played_at)}</small></div><div class="h">${h1(rest.reduce((a, x) => a + x.tenths, 0))}<small>this week</small></div></div>`;
    }
    const rgP = rg ? panel("Your recent games", rg, ` <a class="more" href="${prof(u)}">All my games »</a>`, "o2") : "";
    // Friends' activity: one row per session (live ones first), grouped by day. Latest 5; on
    // two columns up to 10, as many as fit beside Friends online + leaderboard (fitAct42).
    let feed = "";
    if (act && act.items.length) { let last = "";
      act.items.slice(0, 10).forEach(i => { const live = i.kind === "playing", d = live ? "Today" : dayLabel(i.day || i.at);
        if (d !== last) { feed += `<div class="day">${d}</div>`; last = d; }
        feed += `<div class="act">${icon(i.game_id, i.game_name, "g")}<span class="who42"><a href="${prof(i)}">${nm(i)}</a> ${live
          ? `is playing <b>${gname(i.game_id, i.game_name)}</b>${i.started_at ? ` <span class="done">· started ${tFmt.format(new Date(i.started_at))}</span>` : ""}</span><span class="pnow">● playing now</span>`
          : `<span class="done">played</span> <b>${gname(i.game_id, i.game_name)}</b> <span class="done">for ${hm(i.duration_s)}</span></span><span class="w">${tFmt.format(new Date(i.at))}</span>`}</div>`; }); }
    const actP = feed ? panel("Friends' activity", `<div class="feed">${feed}</div>`, ` <a class="more" href="/friends">View all »</a>`, "o3") : "";
    // Friends online: playing / online / away counts; a duration only when the client reported a session start.
    let frP = "";
    if (fl) { const rk = p => p.game_name ? 0 : p.status === "online" ? 1 : 2;
      const on = fl.friends.filter(f => f.presence && f.presence.status !== "offline").sort((a, b) => rk(a.presence) - rk(b.presence) || String(a.display_name || a.handle).localeCompare(String(b.display_name || b.handle)));
      const bd = fl.breakdown || {}, n = (bd.playing || 0) + (bd.online || 0) + (bd.away || 0) || fl.online || 0;
      const tm = p => { if (!p.game_name || !p.session_started_at) return ""; const sec = (Date.now() - new Date(p.session_started_at)) / 1000; return sec >= 0 && sec <= 86400 ? `<span class="tm" title="Started ${tFmt.format(new Date(p.session_started_at))}">${hm(sec)}</span>` : ""; };
      const rows = on.map(f => { const p = f.presence, st = p.game_name ? "playing" : p.status;
        return `<div class="fr"><div class="fa ${tint(f.handle)}">${initial(f.display_name || f.handle)}</div><div class="who"><div class="nm"><span><a href="${prof(f)}">${nm(f)}</a></span><span class="st ${st}">${st}</span></div>${p.game_name
          ? `<div class="gm pl">${icon(p.game_id, p.game_name)}${isUnlisted(p.game_id) ? "Playing a game" : esc(p.game_name)}${tm(p)}</div>` : `<div class="gm">${st === "away" ? "Away for a bit" : "Online, not in a game"}</div>`}</div></div>`; }).join("");
      const extra = n - on.length;
      frP = panel(`Friends online (${n})`, on.length ? `<div class="fhd"><span class="g"><b>${bd.playing || 0}</b> playing</span><span class="o"><b>${bd.online || 0}</b> online</span><span class="a"><b>${bd.away || 0}</b> away</span></div>${rows}${extra > 0 ? `<div class="fr"><a href="/friends">+${extra} more »</a></div>` : ""}`
        : `<div class="in empty">${fl.all ? "None of your friends are online." : `No friends yet. <a href="/friends">Add someone</a>.`}</div>`, ` <a class="more" href="/friends">All friends »</a>`, "o1 fr42"); }
    // Friends leaderboard: same week as above; your row is your "Hours this week".
    let lbP = "";
    const ba = (wk && wk.board_all) || [];
    if (wk && (wk.board.some(r => !r.me) || ba.some(r => !r.me))) {
      const top6 = b => { const mi = b.findIndex(r => r.me); let rows = b.slice(0, 6).map((r, i) => [r, i]); if (mi >= 6) rows = rows.slice(0, 5).concat([[b[mi], mi]]); return rows; };
      const trs = (b, f) => top6(b).map(([r, i]) => `<tr class="${r.me ? "me" : ""}"><td class="rk">${i + 1}</td><td><a href="/u/${encodeURIComponent(r.handle)}">${r.me ? "You" : nm(r)}</a></td><td class="n">${f(r)}</td></tr>`).join("");
      const allH = r => r.minutes > 0 && r.minutes < 60 ? "&lt;1 h" : nf.format(Math.round((r.minutes || 0) / 60)) + " h";
      lbP = panel(`Friends leaderboard <span class="tabs"><a class="on" data-t="week" role="tab" tabindex="0" aria-selected="true">This week</a><a data-t="all" role="tab" tabindex="0" aria-selected="false">All time</a></span>`, `<table class="t2"><tbody data-p="week" class="on">${trs(wk.board, r => h1(r.tenths))}</tbody><tbody data-p="all">${trs(ba, allH)}</tbody></table>
        <div class="per on" data-p="week">Hours this week, ${esc(wk.label)}</div><div class="per" data-p="all">Total hours, as shown on each profile</div>`, "", "o4 lb42"); }
    inner += `<div class="hub"><div>${rgP}${actP}</div><div>${frP}${lbP}</div></div>`;
    $("#h42-in").innerHTML = inner;
    fitAct42();
    if (!fitAct42.on) { fitAct42.on = 1; let t; addEventListener("resize", () => { clearTimeout(t); t = setTimeout(fitAct42, 120); });
      if (document.fonts) document.fonts.ready.then(fitAct42); }
    const wkEl = $("#wk42"); if (wkEl) wkTips(wkEl);
    const lb = $("#h42-in .lb42");
    if (lb) $$(".tabs a", lb).forEach(a => { const go = e => { e.preventDefault(); const t = a.dataset.t;
      $$(".tabs a", lb).forEach(x => { x.classList.toggle("on", x === a); x.setAttribute("aria-selected", x === a ? "true" : "false"); });
      $$("[data-p]", lb).forEach(x => x.classList.toggle("on", x.dataset.p === t)); fitAct42(); };
      a.addEventListener("click", go); a.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") go(e); }); });
  }
  // ---- trending, how it works (signed out), news, poll + community
  const mp = h.most_played.slice(0, 4), m0 = (mp[0] && mp[0].minutes) || 1;
  const trend = mp.length ? panel("Trending games this week", `<div class="tr">${mp.map((g, i) => `<div class="tile2"><div class="art" style="height:132px">${gameArt(g.game_id, g.name)}<span class="rk">#${i + 1}</span><span class="nmx">${esc(g.name)}</span></div><div class="st"><b>${hrsShort(g.minutes)} h</b> this week<span class="hb"><i style="width:${Math.max(2, Math.round(g.minutes / m0 * 100))}%"></i></span></div></div>`).join("")}</div>`, "", "strip4") : "";
  const how = u ? "" : panel("How Fraglink works", `<div class="hows"><div><span class="n1">1</span><div><b>Install Fraglink</b><p>A lightweight Windows tray app. Takes a minute, then it sits down by your clock.</p></div></div><div><span class="n1">2</span><div><b>Play your games</b><p>Just play like you normally would. Your playtime gets tracked automatically.</p></div></div><div><span class="n1">3</span><div><b>Connect with friends</b><p>See who's playing what, and find out who's really put the most hours in.</p></div></div><a class="glass o go" data-dl href="/download" style="margin:0 14px">Get Fraglink</a></div>`);
  const news = h.news.length ? panel("News", h.news.slice(0, 3).map(n => `<div class="ni${n.image ? " has-img" : ""}">${n.image ? `<a class="nimg" href="${esc(n.url)}" tabindex="-1"><img src="${esc(n.image)}" alt="" loading="lazy"></a>` : ""}<div><h4><a href="${esc(n.url)}">${esc(n.title)}</a></h4><div class="d">${fullDate.format(new Date(n.at))}</div>${n.summary ? `<p class="sum">${esc(n.summary)}</p>` : ""}<a class="rm" href="${esc(n.url)}">Read more »</a></div></div>`).join(""), ` <a class="more" href="/news">More news »</a>`) : "";
  const two = `<div class="two" id="h42-two">${panel("Quick poll", `<div class="poll2" id="poll42"><span class="dimt">Loading…</span></div>`)}${panel("Community", `<div class="comm2"><a href="https://discord.gg/QbaYRtvADj" rel="noopener">» Discord server</a><br><a href="/news">» News</a><br><a href="/download">» Download the app</a></div>`)}</div>`;
  $("#h42-rest").innerHTML = trend + how + news + two;
  if (!u) fillDownload();
  loadPoll42();
  // ---- bottom strip (Admin switch "Home page stats strip"): community total + joke estimates
  if (h.stats_strip !== false && (s.minutes_total || 0) >= 60) {
    const rawH = s.minutes_total / 60, est = per => Math.max(1, Math.round(rawH / per));
    const jokes = [[2, "can of energy drink consumed", "cans of energy drink consumed"], [6, "pizza devoured", "pizzas devoured"], [10, "rage-quit", "rage-quits"], [3, "“one more game”", "“one more game”s"], [8, "headset hair day", "headset hair days"]]
      .map(j => ({ n: est(j[0]), one: j[1], many: j[2] })).sort(() => Math.random() - .5).slice(0, 3);
    $("#f2-strip").innerHTML = `<div class="fstat"><div class="wrap">Our members have tracked <b>${nf.format(Math.round(rawH))}</b> hours<div class="fun">${jokes.map(j => `~<b>${nf.format(j.n)}</b> ${j.n === 1 ? j.one : j.many}*`).join(" · ")}<small>*estimated, very scientifically</small></div></div></div>`;
  }
};
pages.download = () => fillDownload();

pages.login = () => {
  if (token() && me()) { location.replace(safeNext(qs("next")) || "/"); return; }
  loadTurnstile();
  const f = $("#login"), err = $("#err"), mf = $("#mfa");
  if (qs("signedout")) showOk(err, "You're signed out.");
  let ticket = null;
  const done = r => { setSession(r.token, r.user); location.href = safeNext(qs("next")) || "/"; };
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true; const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/login", { method: "POST", body: { email: f.login.value.trim(), password: f.password.value, turnstile: turnstileToken(f) } });
      if (r.mfa_required) { ticket = r.mfa_token; f.hidden = true; mf.hidden = false; mf.code.focus(); return; }
      done(r);
    } catch (x) { showErr(err, x); b.disabled = false; turnstileReset(); }
  });
  mf.addEventListener("submit", async e => {
    e.preventDefault(); const me2 = $("#mfa-err"); me2.hidden = true; const b = $("button", mf); b.disabled = true;
    try { done(await api("/auth/mfa", { method: "POST", body: { mfa_token: ticket, code: mf.code.value.trim() } })); }
    catch (x) { showErr(me2, x); b.disabled = false; if (x.code === "mfa_expired") setTimeout(() => location.reload(), 2500); }
  });
};

pages.signup = () => {
  if (token() && me()) { location.replace("/"); return; }
  const blockUntil = Number(localStorage.getItem("fl_dob_block_until") || 0);
  if (blockUntil > Date.now()) {
    const f = $("#signup"), err = $("#err");
    if (f) f.hidden = true;
    showErr(err, new Error("Sorry, you aren't eligible to create a Fraglink account right now."));
    return;
  }
  loadTurnstile();
  const f = $("#signup"), err = $("#err");
  const ysel = f.dob_year, dsel = f.dob_day;
  const yNow = new Date().getFullYear();
  for (let y = yNow; y >= 1900; y--) ysel.insertAdjacentHTML("beforeend", `<option value="${y}">${y}</option>`);
  for (let d = 1; d <= 31; d++) dsel.insertAdjacentHTML("beforeend", `<option value="${String(d).padStart(2, "0")}">${d}</option>`);
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true;
    if (f.password.value !== f.password2.value) { showErr(err, new Error("The passwords don't match.")); return; }
    if (!f.dob_year.value || !f.dob_month.value || !f.dob_day.value) {
      showErr(err, new Error("Please enter your date of birth.")); return;
    }
    const birth_date = `${f.dob_year.value}-${f.dob_month.value}-${f.dob_day.value}`;
    const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/signup", { method: "POST", body: { email: f.email.value.trim(), handle: f.handle.value.trim(), password: f.password.value, birth_date, turnstile: turnstileToken(f) } });
      setSession(r.token, r.user);
      location.href = "/u/" + encodeURIComponent(r.user.handle) + "?welcome=1" + (r.user.email ? "&sent=1" : "");
    } catch (x) {
      if (x.code === "too_young") {
        localStorage.setItem("fl_dob_block_until", String(Date.now() + 24 * 3600 * 1000));
        f.hidden = true;
      }
      showErr(err, x); b.disabled = false; turnstileReset();
    }
  });
};


pages.forgot = () => {
  loadTurnstile();
  const f = $("#forgot"), err = $("#err");
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true; const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/forgot", { method: "POST", body: { email: f.email.value.trim(), turnstile: turnstileToken(f) } });
      showOk(err, r.mail_live ? "If that email is on a Fraglink account, a reset link is on its way. It works for 1 hour. Can't find it? Check your junk or spam folder." : "Reset emails aren't working yet, sorry. Contact support (below) and we'll get you back in.");
    } catch (x) { showErr(err, x); turnstileReset(); }
    b.disabled = false;
  });
};

pages.contact = () => {
  loadTurnstile();
  const f = $("#contact"), err = $("#err");
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true; const b = $("button", f); b.disabled = true;
    try {
      await api("/contact", { method: "POST", body: { name: f.name.value.trim(), email: f.email.value.trim(), topic: f.topic.value, message: f.message.value.trim(), turnstile: turnstileToken(f) } });
      f.reset(); showOk(err, "Sent! We'll get back to you by email.");
    } catch (x) { showErr(err, x); }
    turnstileReset(); b.disabled = false;
  });
};

pages.status = () => {
  const set = (k, ok, text) => { const li = $(`#st-list [data-k="${k}"]`); li.className = ok ? "up" : "down"; $(".st-v", li).textContent = text; };
  const SEV = { minor: "Minor", major: "Major", maintenance: "Maintenance" };
  const when = t => new Date(t).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short", timeZone: ZONE });
  const run = async () => {
    let site = false, api_ = false, live = false;
    try { const r = await fetch("/", { cache: "no-store", method: "HEAD" }); site = r.ok; } catch (e) { /* down */ }
    set("site", site, site ? "Up" : "Down");
    const t0 = performance.now();
    try {
      const r = await fetch("/api/v1/status", { cache: "no-store" });
      if (!r.ok) throw new Error(String(r.status));
      const d = await r.json(), ms = Math.round(performance.now() - t0);
      api_ = d.api && d.database && d.database.ok; live = d.live && d.live.ok;
      set("api", api_, api_ ? `Up · ${ms} ms` : "Down");
      set("live", live, d.maintenance ? "Maintenance" : live ? "Up" : "Down");
      const inc = d.incidents || [];
      $("#st-inc").innerHTML = inc.length ? inc.map(i => `<article class="inc ${esc(i.severity)}"><h3>${esc(i.title)} <span class="tag">${SEV[i.severity] || ""}</span>${i.resolved_at ? ` <span class="tag ok">Resolved</span>` : ` <span class="tag bad">Ongoing</span>`}</h3><div class="hint">${when(i.started_at)}${i.resolved_at ? ` – ${when(i.resolved_at)}` : ""}</div>${i.body ? `<p>${esc(i.body)}</p>` : ""}</article>`).join("") : `<div class="empty">No incidents reported.</div>`;
    } catch (e) { set("api", false, "Down"); set("live", false, "Unknown"); $("#st-inc").innerHTML = `<div class="empty">Couldn't load incidents.</div>`; }
    const all = site && api_ && live;
    $("#st-summary").className = "st-sum " + (all ? "up" : "down");
    $("#st-summary").textContent = all ? "All systems working" : "Some things aren't working right now";
    $("#st-when").textContent = `Checked ${new Date().toLocaleTimeString()} from your browser. Rechecks every 60 seconds.`;
  };
  run(); setInterval(run, 60000);
};

pages.reset = () => {
  const f = $("#reset"), err = $("#err"), t = qs("token");
  const done = $("#reset-done"), msg = $("#done-msg"), note = $("#done-note"), links = $("#done-links");
  // The form goes away for good (success or a dead link) and the panel says what happened.
  const finish = (ok, text, hint, html) => {
    f.hidden = true; done.hidden = false;
    if (ok) showOk(msg, text); else showErr(msg, new Error(text));
    note.textContent = hint || ""; note.hidden = !hint;
    links.innerHTML = html;
  };
  const deadLink = text => finish(false, text, "Reset links work once, for 1 hour. You can ask for a new one.",
    `<a class="btn dl" href="/forgot">Get a new reset link</a><a href="/login" style="margin-left:auto">Back to sign in</a>`);
  if (!t) { deadLink("This page needs the link from your reset email."); return; }
  const b = $("button", f);
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true;
    if (f.password.value !== f.password2.value) return showErr(err, new Error("The passwords don't match."));
    b.disabled = true;
    try {
      await api("/auth/reset", { method: "POST", body: { token: t, password: f.password.value } });
      clearSession(); f.reset();
      finish(true, "Your password has been changed.", "You've been signed out everywhere else, including the app. Sign in with your new password.",
        `<a class="btn dl" href="/login">Sign in</a>`);
    } catch (x) {
      if (x.code === "bad_token") deadLink("That reset link has expired or was already used.");
      else { showErr(err, x.status >= 500 ? new Error("Something went wrong on our side and your password wasn't changed. Please try again in a minute.") : x); b.disabled = false; }
    }
  });
};

pages.verify = async () => {
  const m = $("#msg"), t = qs("token");
  if (!t) { showErr(m, new Error("This page needs the link from your email.")); return; }
  try {
    const r = await api("/auth/verify", { method: "POST", body: { token: t } });
    if (r.verified) {
      showOk(m, "Thanks, your email is confirmed.");
      const u = me(); if (u) setSession(token(), Object.assign(u, { email_verified: true }));
      $$(".verify-bar:not(.delete-bar)").forEach(el => el.remove());
      sessionStorage.removeItem("fl_vb_closed");
    }
    else showErr(m, new Error("This link was for an email address that's no longer on the account."));
  } catch (x) { showErr(m, x); }
};

/* ----- profile ----- */
const GPAGE = 10;
/* Pager: "‹ Prev  11–20 of 248  Next ›" (buttons carry data-off). */
function pagerHtml(off, lim, total, term) {
  if (!total) return "";
  const from = off + 1, to = Math.min(off + lim, total), nf_ = n => n.toLocaleString("en");
  return `<div class="pager"><button type="button" class="btn sm" data-off="${Math.max(0, off - lim)}" ${off ? "" : "disabled"}>‹ Prev</button>
    <span>${nf_(from)}–${nf_(to)} of ${nf_(total)}${term ? ` matching “${esc(term)}”` : ""}</span>
    <button type="button" class="btn sm" data-off="${off + lim}" ${to < total ? "" : "disabled"}>Next ›</button></div>`;
}
/* Search box with clear (x): filters as you type. */
function searchBox(ph, id) { return `<span class="sbox"><input class="t" type="search" id="${id}" placeholder="${esc(ph)}" autocomplete="off" spellcheck="false"><button type="button" class="sx-clear" aria-label="Clear search" hidden>&times;</button></span>`; }
function wireSearch(input, onChange) {
  const x = input.parentNode.querySelector(".sx-clear");
  const run = () => { if (x) x.hidden = !input.value; onChange(input.value.trim().toLowerCase()); };
  input.addEventListener("input", run);
  input.addEventListener("keydown", e => { if (e.key === "Escape" && input.value) { e.preventDefault(); input.value = ""; run(); } });
  if (x) x.addEventListener("click", () => { input.value = ""; run(); input.focus(); });
}
function COUNTRIES() {
  if (COUNTRIES.c) return COUNTRIES.c;
  const dn = new Intl.DisplayNames(["en"], { type: "region" }), out = [];
  for (let a = 65; a <= 90; a++) for (let b = 65; b <= 90; b++) { const c = String.fromCharCode(a, b); try { const n = dn.of(c); if (n && n !== c && !/Unknown/.test(n)) out.push([c, n]); } catch (e) { } }
  return (COUNTRIES.c = out.filter(([c]) => !["AA", "QM", "QN", "QO", "QP", "QQ", "QR", "QS", "QT", "QU", "QV", "QW", "QX", "QY", "QZ", "XA", "XB", "XK", "ZZ", "EU", "EZ", "UN"].includes(c) || c === "XK").sort((x, y) => x[1].localeCompare(y[1])));
}
function countryName(c) { try { return new Intl.DisplayNames(["en"], { type: "region" }).of(c); } catch (e) { return c; } }
// Flags are served from our own site (flag-icons, MIT; see /flags/LICENSE.txt). Codes we have an image for:
const FLAGS = new Set("AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CP CR CU CV CW CX CY CZ DE DG DJ DK DM DO DZ EC EE EG EH ER ES ET EU FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU IC ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PC PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM UN US UY UZ VA VC VE VG VI VN VU WF WS XK YE YT ZA ZM ZW".split(" "));
function flag(c) { return c && FLAGS.has(c) ? `<img class="flag" src="/flags/${c.toLowerCase()}.png" width="20" height="15" alt="" loading="lazy">` : ""; }
pages.profile = async () => {
  const root = $("#profile");
  const bits = location.pathname.replace(/^\/u\/?/, "").replace(/\/$/, "").split("/").filter(Boolean);
  const handle = decodeURIComponent(bits[0] || "");
  const wowAll = bits.length === 2 && bits[1] === "wow";
  if (!handle) { location.replace(me() ? "/u/" + encodeURIComponent(me().handle) : "/login"); return; }
  if (bits.length > 1 && !wowAll) { root.innerHTML = `<div class="narrow panel"><h2>Not found</h2><div class="body"><p>That page isn't on Fraglink.</p></div></div>`; return; }
  document.title = handle + " · Fraglink";
  let p;
  try { p = await api("/users/" + encodeURIComponent(handle) + "/profile?limit=" + GPAGE + "&tz=" + encodeURIComponent(TZ), { keepSession: true }); }
  catch (e) {
    if (e.status === 401) {
      root.innerHTML = `<div class="narrow panel"><h2>${esc(handle)}</h2><div class="body"><p>This profile is only visible to signed-in Fraglink players.</p><p><a class="btn" href="/login?next=${encodeURIComponent(location.pathname)}">Sign in</a> <a class="btn dl" href="/signup">Create an account</a></p></div></div>`;
      return;
    }
    root.innerHTML = e.status === 404
      ? `<div class="narrow panel"><h2>Not found</h2><div class="body"><p>There's no Fraglink player called <b>${esc(handle)}</b>.</p></div></div>`
      : `<div class="narrow alert err">${esc(e.message)}</div>`;
    return;
  }
  // old username (renamed by staff): show the new address
  if (p.handle && p.handle.toLowerCase() !== String(handle).toLowerCase()) history.replaceState(null, "", "/u/" + encodeURIComponent(p.handle) + (wowAll ? "/wow" : "") + location.search + location.hash);
  if (p.private) {
    const nm = esc(p.display_name || p.handle), next = encodeURIComponent(location.pathname + location.search);
    // Friend actions: hidden for yourself, friends and test accounts (server says can_add).
    let act = "";
    if (p.relation === "outgoing") act = `<span class="muted" style="margin-right:8px">Request sent</span><button class="btn sm" type="button" data-cancel-req>Cancel</button>`;
    else if (p.relation === "incoming") act = `<button class="btn dl" type="button" data-accept>Accept friend request</button>`;
    else if (p.can_add) act = !token() ? `<a class="btn blue" href="/login?next=${next}">Sign in to add ${nm}</a>` : `<button class="btn blue" type="button" data-add>+ Add friend</button>`;
    else if (!token()) act = `<a class="btn" href="/login?next=${next}">Sign in</a>`;
    root.innerHTML = `<div class="narrow panel"><h2>${nm}</h2><div class="body" style="display:flex;gap:16px;align-items:center">${avatar(p.avatar, 64)}<div><p>This profile is private. Only ${nm}'s friends can see it.</p>
      <div class="pact">${act}</div><div class="alert err" data-perr hidden style="margin-top:8px"></div></div></div></div>`;
    const box = $(".pact", root), perr = $("[data-perr]", root);
    const run = async (b, fn, done) => { b.disabled = true; perr.hidden = true; try { await fn(); box.innerHTML = done; wire(); } catch (x) { b.disabled = false; showErr(perr, x); } };
    const wire = () => {
      const add = $("[data-add]", box), can = $("[data-cancel-req]", box), acc = $("[data-accept]", box);
      if (add) add.addEventListener("click", () => run(add, () => api("/friends/request", { method: "POST", body: { handle: p.handle } }),
        `<span class="muted" style="margin-right:8px">Request sent</span><button class="btn sm" type="button" data-cancel-req>Cancel</button>`));
      if (can) can.addEventListener("click", () => run(can, () => api("/friends/" + encodeURIComponent(p.handle), { method: "DELETE" }),
        `<button class="btn blue" type="button" data-add>+ Add friend</button>`));
      if (acc) acc.addEventListener("click", () => run(acc, async () => { await api("/friends/respond", { method: "POST", body: { handle: p.handle, accept: true } }); location.reload(); }, `<span class="muted">✓ Friends</span>`));
    };
    wire();
    return;
  }
  // Gear, Mythic+, raids and collections open only for a verified Retail character, and
  // only while Admin has "Show WoW character details" on. Hidden and Forever cards stay plain.
  const poppable = c => FEATURES.wow_characters && c.verified && c.game_id === "wow" && !c.hidden;
  const wowCard = (c, showRealm = true) => {
    const col = WOW.color(c.class), cls = WOW.cls(c.class);
    const tick = c.verified ? `<span class="wc-tick" title="On the linked Battle.net account">✓</span>` : "";
    const guild = c.show_guild && c.guild ? `<div class="wc-g">&lt;${esc(c.guild)}&gt;</div>` : "";
    const realm = showRealm && c.realm ? `<div class="wc-realm">${esc(c.realm)}</div>` : "";
    const ini = esc(String(c.name || "?").slice(0, 2));
    const lvl = Number(c.level) > 0 ? `Level ${c.level}` : "";
    const line = [lvl && `<span class="wc-lv">${esc(lvl)}</span>`, cls && `<span class="wc-cl">${esc(cls)}</span>`].filter(Boolean).join(" ");
    const open = poppable(c);
    return `<div class="wc${open ? " wc-click" : ""}" style="--cls:${col}"${open ? ` data-wcid="${esc(c.id)}" tabindex="0" role="button" aria-label="Open ${esc(c.name)} character details"` : ""}><div class="wc-p"><span class="wc-ini">${ini}</span><img class="r" src="${WOW.raceIcon(c.race, c.gender, c.class)}" alt="" loading="lazy" ${imgOk}></div><div class="wc-t"><b>${esc(c.name)}${factionCrest(c.faction)}${tick}</b>${line ? `<div class="wc-meta">${line}</div>` : ""}${guild}${realm}</div></div>`;
  };
  const wireWowPop = () => {
    if (root._wowpop) return;
    root._wowpop = true;
    const go = n => {
      const c = (p.wow_characters || []).find(x => String(x.id) === n.dataset.wcid);
      if (c && poppable(c)) WOWPOP.open(p.handle, c);
    };
    root.addEventListener("click", e => { const n = e.target.closest("[data-wcid]"); if (n && root.contains(n)) go(n); });
    root.addEventListener("keydown", e => {
      if (e.key !== "Enter" && e.key !== " ") return;
      const n = e.target.closest("[data-wcid]");
      if (!n || e.target !== n) return;
      e.preventDefault();
      go(n);
    });
  };
  const wowShow = p.wow_show === "wow" || p.wow_show === "wow_forever" || p.wow_show === "none" ? p.wow_show : "both";
  const wowRows = flav => (p.wow_characters || []).filter(c => !c.hidden && (c.game_id === "wow_forever" ? "wow_forever" : "wow") === flav);
  const bothSides = wowRows("wow").length > 0 && wowRows("wow_forever").length > 0;
  const wowToggle = typeof p.wow_toggle === "boolean" ? p.wow_toggle : wowShow === "both" && bothSides;
  const wowOpen = p.wow_open === "wow" || p.wow_open === "wow_forever" || p.wow_open === "none" ? p.wow_open
    : wowShow === "none" ? "none"
    : wowShow === "wow" || wowShow === "wow_forever" ? wowShow
    : bothSides ? (p.wow_default === "wow_forever" ? "wow_forever" : "wow")
    : wowRows("wow_forever").length && !wowRows("wow").length ? "wow_forever" : "wow";
  const manage = p.relation === "self" ? `<a href="/settings#wow">Manage</a>` : "";
  if (wowAll) {
    document.title = "WoW characters · " + (p.display_name || p.handle);
    window.flCrumb([[p.display_name || p.handle, "/u/" + encodeURIComponent(p.handle)], ["WoW characters"]]);
    if (wowShow === "none" || wowOpen === "none") {
      root.innerHTML = `<div class="wow-page"><a href="/u/${encodeURIComponent(p.handle)}">← ${esc(p.display_name || p.handle)}</a><h1>WoW characters</h1><p class="empty">No WoW characters on this profile.</p>${manage ? `<p class="wow-foot">${manage}</p>` : ""}</div>`;
      return;
    }
    const paint = flav => { const box = $("[data-wowgal]", root); if (box) box.innerHTML = groupWowRealms(wowRows(flav)).map(g => `<section class="wow-realm"><h2>${esc(g.realm)}${g.showRegion ? ` <span class="sub">${esc(g.region)}</span>` : ""}<span class="wow-realm-count" aria-label="${g.rows.length} characters">${g.rows.length}</span></h2><div class="wow-gal">${g.rows.map(c => wowCard(c, false)).join("")}</div></section>`).join("") || `<p class="empty">No ${flav === "wow_forever" ? "Forever" : "Retail"} characters.</p>`; $$("[data-flav]", root).forEach(b => b.setAttribute("aria-pressed", b.dataset.flav === flav ? "true" : "false")); };
    const tog = wowToggle ? `<div class="seg" role="tablist" aria-label="Which roster to show"><button type="button" data-flav="wow" aria-pressed="${wowOpen === "wow"}">Retail</button><button type="button" data-flav="wow_forever" aria-pressed="${wowOpen === "wow_forever"}">Forever</button></div>` : "";
    root.innerHTML = `<div class="wow-page"><a class="wow-back" href="/u/${encodeURIComponent(p.handle)}">← ${esc(p.display_name || p.handle)}</a><div class="bar"><h1>WoW characters</h1>${tog}</div><div class="wow-realms" data-wowgal></div>${manage ? `<p class="wow-foot">${manage}</p>` : ""}</div>`;
    paint(wowOpen);
    wireWowPop();
    if (wowToggle) root.addEventListener("click", e => { const b = e.target.closest("[data-flav]"); if (!b) return; paint(b.dataset.flav); });
    return;
  }
  const pres = p.presence;
  const ids = [].concat((p.games || []).map(g => g.game_id), (p.recent || []).map(r => r.game_id), pres && pres.game_id ? [pres.game_id] : []);
  await loadIcons(ids);
  document.title = (p.display_name || p.handle) + " · Fraglink";
  window.flCrumb([[p.display_name || p.handle]]);
  const self = p.relation === "self";
  const live = pres && pres.status !== "offline" && pres.game_name;
  const listedGames = (p.games || []).filter(g => g.listed !== false);
  const main = listedGames[0];
  const bannerG = live && !isUnlisted(pres.game_id) ? { game_id: pres.game_id, name: pres.game_name } : main;
  const total = p.overall ? p.overall.total_min : 0;

  let actions = "";
  if (p.relation === "none") actions = `<button class="btn blue" data-add>+ Add friend</button>`;
  else if (p.relation === "outgoing") actions = `<span class="btn" aria-disabled="true">Friend request sent</span>`;
  else if (p.relation === "incoming") actions = `<button class="btn dl" data-accept>Accept friend request</button>`;
  else if (self) actions = `<a class="btn" href="/settings">Edit profile</a>`;
  else if (p.relation === "friend") actions = `<span class="btn" aria-disabled="true">✓ Friends</span>`;
  else if (p.relation === "public" && !token()) actions = `<a class="btn blue" href="/signup">Join Fraglink to add ${esc(p.handle)}</a>`;

  const st = pres ? pres.status : null;
  let sub = pres ? `<span class="status"><span class="dot ${esc(st)}"></span>${statusText(pres)}</span>` : "";
  if (p.member_since) sub += (sub ? " · " : "") + `member since ${fullDate.format(new Date(p.member_since))}`;
  if (p.location) sub += " · " + esc(p.location);
  const badges = (p.linked || []).filter(l => BRAND[l.provider]).length ? `<div class="lbadges">${p.linked.filter(l => BRAND[l.provider]).map(l => {
    const [nm, col, path] = BRAND[l.provider], tip = esc(nm + (l.display ? ": " + l.display : " (verified)"));
    const inner = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${path}"/></svg><i class="vt" aria-hidden="true">✓</i>`;
    return l.url ? `<a class="lb" href="${esc(l.url)}" target="_blank" rel="noopener" style="--bc:${col}" data-tip="${tip}" aria-label="${tip}">${inner}</a>` : `<span class="lb" tabindex="0" style="--bc:${col}" data-tip="${tip}" aria-label="${tip}">${inner}</span>`;
  }).join("")}</div>` : "";
  const np = live ? `<div class="np">${gameArt(pres.game_id, pres.game_name, "npa")}<div><div style="font-size:10px;letter-spacing:1px;text-transform:uppercase;opacity:.8">Now playing</div>
      <b>${isUnlisted(pres.game_id) ? "A game" : esc(pres.game_name)}</b>${pres.game_detail ? ` <span style="opacity:.8">· ${esc(pres.game_detail)}</span>` : ""}</div>${pres.session_started_at ? `<span style="opacity:.8">for ${since(pres.session_started_at)}</span>` : ""}</div>` : "";
  const sec = (title, body, right, cls, id) => `<section class="panel sx${cls ? " " + cls : ""}"${id ? ` id="${id}"` : ""}><h2>${title}${right ? `<span class="more">${right}</span>` : ""}</h2>${body}</section>`;
  const report = !self && token() ? `<div class="avlinks">${p.avatar && p.avatar.kind === "upload" ? `<a href="#" class="report" data-report>Report picture</a>` : ""}<a href="#" class="report flagit" data-report-profile>&#9873; Report this profile</a></div>` : "";
  const kpi = `<div class="kpi">${p.history_visible ? `<div><b>${total > 0 && total < 60 ? "&lt;1" : nf.format(Math.round(total / 60))}</b><span>Total hours</span></div><div${p.overall.week_label ? ` title="Tracked hours, ${esc(p.overall.week_label)}"` : ""}><b>${p.overall.tenths_7d != null ? (p.overall.tenths_7d / 10).toFixed(1) : hrsShort(p.overall.minutes_7d)}</b><span>Past 7 days</span></div>` : ""}${self || Array.isArray(p.friends) ? `<a href="${self ? "/friends" : "#friends"}" class="kf"><b>${p.friend_count}</b><span>Friends</span></a>` : `<div><b>${p.friend_count}</b><span>Friends</span></div>`}</div>`;
  const nick = p.display_name || p.handle, prevs = (p.previous_names || []).filter(n => n.toLowerCase() !== nick.toLowerCase() && n.toLowerCase() !== p.handle.toLowerCase());
  const head = `<section class="ph${bannerG ? "" : " noart"}"${bannerG ? ` style="background:${artBg(bannerG.game_id, bannerG.name)}"` : ""}><div class="inr">
      <div>${avatar(p.avatar, 184)}${report}</div>
      <div class="who"><h1><span class="pname"${prevs.length ? ` tabindex="0" data-tip="${esc("Previously known as: " + prevs.join(", "))}"` : ""}>${esc(p.display_name || p.handle)}</span>${p.display_name && p.display_name !== p.handle ? ` <small style="font-size:15px;color:#c8d4e0">@${esc(p.handle)}</small>` : ""}</h1>
      <div class="sub">${sub}</div>${badges}${np}<div class="acts">${actions}</div></div>${kpi}</div></section>`;

  const welcome = qs("welcome") && self ? `<div class="alert ok" style="margin-bottom:12px">Welcome to Fraglink. <a href="/download">Download the app</a> and sign in with the same account to start tracking. You can pick a picture in <a href="/settings">Settings</a>.${qs("sent") ? " We sent a link to confirm your email. Can't find it? Check your junk or spam folder." : ""}</div>` : "";
  const pendingDelete = self && p.delete_after ? `<div class="alert err" style="margin-bottom:12px">This account will be deleted on ${fullDate.format(new Date(p.delete_after))}. Sign in again to keep it.</div>` : "";

  // Miniprofile (public profiles only): the owner gets the embed text and copy rows; everyone
  // else (signed out or another user) just sees the live image, linking to the profile.
  const shareUrl = siteBase() + "/u/" + encodeURIComponent(p.handle), sig = sigUrl(p.handle);
  const codes = [["Link", shareUrl], ["BBCode", `[url=${shareUrl}][img]${sig}[/img][/url]`], ["HTML", `<a href="${shareUrl}"><img src="${sig}" alt="${p.display_name || p.handle} on Fraglink" width="460" height="100"></a>`]];
  const miniprofile = !p.public_profile
    ? (self ? sec("Miniprofile", `<div class="body empty">Your profile is private (friends only), so there's no public link or signature image. <a href="/settings#privacy">Privacy settings</a></div>`) : "")
    : self ? sec("Miniprofile", `<div class="body mp">${sigPreview(p.handle)}
        <div class="mpt">Embed this anywhere (forums, blogs, Discord). It updates live.</div>
        <div class="mpcodes">${codes.map(([l, v]) => `<div class="code"><b>${l}</b><input class="t" readonly value="${esc(v)}"><button class="btn sm" type="button" data-copy="${esc(v)}">Copy</button></div>`).join("")}</div></div>`)
    : sec("Miniprofile", `<div class="body mp mpv"><a href="/u/${encodeURIComponent(p.handle)}" title="${esc(p.display_name || p.handle)} on Fraglink">${sigPreview(p.handle)}</a></div>`);

  // Details + inset box
  const kv = (k, v) => v ? `<dt>${k}</dt><dd>${v}</dd>` : "";
  const loc = [p.location ? esc(p.location) : (p.country ? esc(countryName(p.country)) : ""), flag(p.country)].filter(Boolean).join(" ");
  const inset = `<div class="inset">${pres ? `<b>${live ? "Playing" : "Status"}</b><div>${live ? `${icon(pres.game_id, pres.game_name)} ${isUnlisted(pres.game_id) ? "A game" : `<span class="gl">${esc(pres.game_name)}</span>`}` : statusText(pres)}</div>` : ""}
      ${p.member_since ? `<b>Member since</b><div>${fullDate.format(new Date(p.member_since))}</div>` : ""}${self ? `<a href="/friends" class="fc">${p.friend_count} friend${p.friend_count === 1 ? "" : "s"}</a>` : Array.isArray(p.friends) ? `<a href="#friends" class="fc">${p.friend_count} friend${p.friend_count === 1 ? "" : "s"}</a>` : ""}</div>`;
  const details = sec("Details", `<div class="body details2"><dl class="kv">
      ${kv("Name", `${esc(nick)}${nick !== p.handle ? ` <span class="mut">(${esc(p.handle)})</span>` : ""}`)}
      ${kv("Location", loc)}${kv("Age", p.age != null ? String(p.age) : "")}${kv("Gender", p.gender ? esc(p.gender) : "")}${kv("Real name", p.real_name ? esc(p.real_name) : "")}
      ${kv("Style", p.gaming_style ? esc(p.gaming_style) : "")}${main ? kv("Main game", esc(main.name)) : ""}
    </dl>${inset}</div>${p.about ? `<p class="about">${esc(p.about)}</p>` : ""}`);
  let games, gamesRow;
  if (!p.history_visible && !p.games_visible) {
    const why = p.relation === "friend" || p.relation === "public" ? `${esc(p.handle)} keeps their play history private.` : `Add ${esc(p.handle)} as a friend to see their games and hours.`;
    games = sec("Games", `<div class="body empty">${why}</div>`);
  } else if (!p.games || !p.games.length) {
    games = sec("Games", `<div class="body empty">No hours recorded yet.${self ? ` <a href="/download">Get the app</a> and play something.` : ""}</div>`);
  } else {
    const w7 = x => x.tenths_7d != null ? (x.tenths_7d > 0 ? h1(x.tenths_7d) : "-") : hours(x.minutes_7d);
    const row = g => `<tr><td><span class="gico">${icon(g.listed === false && !self ? "manual:unlisted" : g.game_id, g.name)}</span>${g.listed === false && !self ? `<span class="empty">${esc(g.name)}</span>` : esc(g.name)}${g.tier && g.tier !== "tracked" && g.tier !== "imported" ? ` <span class="tier">${esc(g.tier)}</span>` : ""}</td>
      <td class="n">${w7(g)}</td><td class="n">${hours(g.total_min)}</td><td class="n hide-sm">${live && pres.game_id === g.game_id ? `<span style="color:var(--online)">Now</span>` : lastPlayed(g.last_played_at)}</td></tr>`;
    const many = (p.games_total || p.games.length) > GPAGE;
    games = sec("Games", `
      <table class="gt" style="width:100%;border-collapse:collapse"><thead><tr>${[["name", "Game", ""], ["week", "Past 7 days", "n"], ["total", "Total", "n"], ["recent", "Last played", "n hide-sm"]].map(([k, l, c]) => `<th class="${c} srt${k === "total" ? " on" : ""}" data-gs="${k}" tabindex="0" role="button" aria-sort="${k === "total" ? "descending" : "none"}" title="Sort by ${l.toLowerCase()}">${l}<i>${k === "total" ? " ▾" : ""}</i></th>`).join("")}</tr></thead>
      <tbody data-gbody>${p.games.map(row).join("")}</tbody>
      <tfoot><tr class="tot"><td>Overall hours</td><td class="n">${w7(p.overall)}</td><td class="n">${hours(p.overall.total_min)}</td><td class="hide-sm"></td></tr></tfoot></table>
      <div data-gpager>${many ? pagerHtml(0, GPAGE, p.games_total) : ""}</div>`);
    gamesRow = row;
  }

  // Public cards: name, guild, class, avatar, verified tick. No hours and no past names.
  let wowc = "";
  const WC = (p.wow_characters || []).filter(c => !c.hidden);
  if (wowShow !== "none" && WC.length) {
    const ordered = flav => wowRows(flav).slice().sort((a, b) => (a.featured || 9) - (b.featured || 9));
    const grid = flav => { const rows = ordered(flav); return `<div class="wcs" data-wowcards>${rows.slice(0, 6).map(c => wowCard(c)).join("") || `<p class="empty" style="padding:8px 0">No ${flav === "wow_forever" ? "Forever" : "Retail"} characters.</p>`}</div><p class="wow-foot">${self ? `<a href="/settings#wow">Manage</a><span>·</span>` : ""}<a href="/u/${encodeURIComponent(p.handle)}/wow">View all</a></p>`; };
    const tog = wowToggle ? `<div class="seg" role="tablist" aria-label="Which roster to show"><button type="button" data-wflav="wow" aria-pressed="${wowOpen === "wow"}">Retail</button><button type="button" data-wflav="wow_forever" aria-pressed="${wowOpen === "wow_forever"}">Forever</button></div>` : "";
    wowc = sec("WoW characters", grid(wowOpen), tog);
  }

  // Achievements
  let ach = "";
  const A = p.achievements;
  if (A && A.rows) {
    const AI = { clocked_in: "&#9201;", no_days_off: "&#128197;", game_hopper: "&#127918;", regular_circuit: "&#128260;", one_more_round: "&#9203;", night_owl: "&#127769;" };
    const unit = (r, n) => r.unit === "hours" ? `${n} hour${n === 1 ? "" : "s"}` : `${n} ${r.unit.replace(/s$/, "")}${n === 1 ? "" : "s"}`;
    ach = sec("Achievements", `<div class="body achv"><div class="achv-top"><span><b>${A.unlocked}</b> of ${A.total_tiers} tiers unlocked</span><span><b>${A.points}</b> points · ${A.mastered} / ${A.tracks} mastered</span></div>
      <table class="achs"><thead><tr><th>Achievement</th><th>Progress</th><th class="n">Tier</th></tr></thead><tbody>${A.rows.map(r => `<tr><td><span class="ai">${AI[r.key] || "&#9733;"}</span><div class="an"><b>${esc(r.name)}</b><small>${unit(r, r.value)}${r.next_at != null ? ` · ${unit(r, r.remaining)} to ${esc(r.next_tier)}` : " · maxed"}</small></div></td>
        <td><div class="pbar"><i style="width:${r.progress}%"></i></div><small>${r.progress}%</small></td><td class="n"><span class="tierb ${r.tier ? r.tier.toLowerCase() : "none"}">${r.tier ? esc(r.tier) : "—"}</span></td></tr>`).join("")}</tbody></table>
      ${A.next ? `<div class="achv-next"><span>Next unlock: <b>${esc(A.next.name)} · ${esc(A.next.next_tier)}</b></span><span>${unit(A.next, A.next.remaining)} to go</span><div class="pbar big"><i style="width:${A.next.progress}%"></i></div></div>` : ""}</div>`);
  }

  // Mini activity: last 3 days, events + per-game daily totals
  const cut = Date.now() - 3 * 86400e3;
  const items = [];
  (p.events || []).forEach(e => {
    const at = new Date(e.at);
    if (e.kind === "friend") items.push({ at, html: `<span class="ev">&#128101;</span><span><b>${esc(p.display_name || p.handle)}</b> is now friends with <a href="/u/${encodeURIComponent(e.handle)}">${esc(e.display_name || e.handle)}</a></span>` });
    if (e.kind === "first_play") { const g = e.name ? { game_id: e.game_id, name: e.name } : null; if (g) items.push({ at, html: `${icon(g.game_id, g.name)}<span><b>${esc(p.display_name || p.handle)}</b> played <span class="gl">${esc(g.name)}</span> for the first time</span>` }); }
  });
  const daysM = new Map();
  if (p.history_visible) (p.recent || []).forEach(r => {
    const d = new Date(r.ended_at), k = dayKey(d);
    if (!daysM.has(k)) daysM.set(k, { at: d, games: new Map() });
    const g = daysM.get(k).games, gk = r.game_id + "|" + r.game_name, cur = g.get(gk) || { id: r.game_id, name: r.game_name, secs: 0, at: d };
    cur.secs += r.duration_s || 0; if (d > cur.at) cur.at = d; g.set(gk, cur);
  });
  daysM.forEach(d => d.games.forEach(g => items.push({ at: g.at, old: g.at < cut, html: `${icon(g.id, g.name)}<span>Played ${isUnlisted(g.id) ? "a game" : `<span class="gl">${esc(g.name)}</span>`} for ${dur(g.secs)}</span>` })));
  items.sort((x, y) => y.at - x.at);
  let act = live ? `<div class="ma-day">Now</div><div class="ma-row">${icon(pres.game_id, pres.game_name)}<span>${isUnlisted(pres.game_id) ? "Playing a game now" : `Playing <span class="gl">${esc(pres.game_name)}</span> now`}${pres.session_started_at ? ` <span class="ma-tm">for ${since(pres.session_started_at)}</span>` : ""}</span></div>` : "";
  let lastDay = "", older = 0;
  items.forEach(it => {
    const lbl = dayLabel(it.at);
    const blk = (lbl !== lastDay ? `<div class="ma-day">${lbl}</div>` : "") + `<div class="ma-row">${it.html}</div>`;
    lastDay = lbl;
    if (it.old) { act += `<div class="olda" hidden>${blk}</div>`; older++; } else act += blk;
  });
  const mini = sec("Mini activity", `<div class="body mact">${act || `<span class="empty">${p.history_visible ? "Nothing in the last few days." : "Activity is visible to friends when shared."}</span>`}${older ? `<div class="seemore"><a href="#" data-seemore>See more</a></div>` : ""}</div>`);

  const fr = Array.isArray(p.friends) ? sec("Friends", `<div class="body">${p.friends.length ? `<div class="flist">${p.friends.map(f => `<div class="fi"><a href="/u/${encodeURIComponent(f.handle)}">${avatar(f.avatar, 32)}</a><div><a href="/u/${encodeURIComponent(f.handle)}">${esc(f.display_name || f.handle)}</a><div class="st ${f.game_name ? "pl" : f.status === "online" ? "on" : ""}">${f.game_name ? esc(f.game_name) : f.status === "online" ? "Online" : f.status === "away" ? "Away" : "Offline"}</div></div></div>`).join("")}</div>` : `<span class="empty">No friends yet.</span>`}</div>`, self ? `<a href="/friends">See all friends</a>` : `${p.friend_count}`, "", "friends") : "";
  const RIG = [["manufacturer", "Manufacturer"], ["cpu", "Processor"], ["gpu", "Video card"], ["memory", "Memory"], ["storage", "Storage"], ["motherboard", "Motherboard"], ["monitor", "Monitor"], ["headset", "Headset"], ["keyboard", "Keyboard"], ["mouse", "Mouse"], ["mousepad", "Mouse pad"], ["case", "Case"], ["os", "Operating system"]];
  const rig = p.rig ? sec("Gaming rig", `<div class="body"><dl class="rig">${RIG.filter(([k]) => p.rig[k]).map(([k, l]) => `<dt>${l}</dt><dd>${esc(p.rig[k])}</dd>`).join("")}</dl></div>`, self ? `<a href="/settings#rig">Edit</a>` : "")
    : self ? sec("Gaming rig", `<div class="body empty">Show off your setup. <a href="/settings#rig">Add your gaming rig</a>.</div>`) : "";
  const discord = sec("Discord server", `<div class="body discord-box"><div class="dl"><img src="/assets/discord-symbol-white.svg" alt=""></div><div><a href="${DISCORD}" rel="noopener">Fraglink on Discord</a><small>Chat, feedback and release news</small></div></div>`);

  wireWowPop();
  root.addEventListener("click", e => {
    const b = e.target.closest("[data-wflav]"); if (!b || !wowToggle) return;
    $$("[data-wflav]", root).forEach(x => x.setAttribute("aria-pressed", x === b ? "true" : "false"));
    const box = $("[data-wowcards]", root); if (!box) return;
    const rows = wowRows(b.dataset.wflav).slice().sort((a, c) => (a.featured || 9) - (c.featured || 9));
    box.innerHTML = rows.slice(0, 6).map(c => wowCard(c)).join("") || `<p class="empty" style="padding:8px 0">No ${b.dataset.wflav === "wow_forever" ? "Forever" : "Retail"} characters.</p>`;
  });
  root.innerHTML = `<div class="pv3">` + head + pendingDelete + welcome + `<div class="g2v"><div class="col">${details}${games}${wowc}${ach}${miniprofile}</div><aside class="col">${mini}${rig}${fr}${discord}</aside></div></div>`;
  $$('a[href="#friends"]', root).forEach(a => a.addEventListener("click", e => { const f = $("#friends", root); if (!f) return; e.preventDefault(); f.scrollIntoView({ behavior: "smooth", block: "center" }); f.classList.add("flash"); setTimeout(() => f.classList.remove("flash"), 1200); }));
  const sm = $("[data-seemore]", root);
  if (sm) sm.addEventListener("click", e => { e.preventDefault(); $$(".olda", root).forEach(x => x.hidden = false); sm.parentNode.remove(); });
  const rp = $("[data-report-profile]", root);
  if (rp) rp.addEventListener("click", async e => {
    e.preventDefault(); const reason = prompt("Report this profile to the Fraglink moderators. What's wrong? (name, about text, impersonation...)"); if (reason === null) return;
    try { await api("/users/" + encodeURIComponent(p.handle) + "/report", { method: "POST", body: { reason } }); rp.textContent = "Reported. Thanks."; } catch (x) { alert(x.message); }
  });
  const add = $("[data-add]", root), acc = $("[data-accept]", root), more = $("[data-more]", root), rep = $("[data-report]", root);
  if (add) add.addEventListener("click", async () => { if (!token()) return requireLogin(); add.disabled = true; try { await api("/friends/request", { method: "POST", body: { handle: p.handle } }); add.textContent = "Friend request sent"; } catch (e) { add.disabled = false; alert(e.message); } });
  if (acc) acc.addEventListener("click", async () => { acc.disabled = true; try { await api("/friends/respond", { method: "POST", body: { handle: p.handle, accept: true } }); location.reload(); } catch (e) { acc.disabled = false; alert(e.message); } });
  const gbody = $("[data-gbody]", root), gpg = $("[data-gpager]", root);
  if (gbody && gpg) {
    // Sorted and paged on the server. Clicking the active column flips its direction; another
    // column starts in its natural one (Game A→Z, the numbers biggest first, Last played newest).
    const st = { offset: 0, sort: "total", dir: "desc" };
    const load = async keepPos => {
      const qs = new URLSearchParams({ show: "games", limit: GPAGE, offset: st.offset, sort: st.sort, dir: st.dir, tz: TZ });
      gbody.style.opacity = .5;
      try {
        const r = await api("/users/" + encodeURIComponent(p.handle) + "/profile?" + qs, { keepSession: true });
        await loadIcons(r.games.map(g => g.game_id));
        gbody.innerHTML = r.games.map(gamesRow).join("");
        gpg.innerHTML = r.total > GPAGE ? pagerHtml(st.offset, GPAGE, r.total) : "";
        if (keepPos) { const t = gbody.closest("section"); if (t.getBoundingClientRect().top < 0) t.scrollIntoView({ block: "start" }); }
      } catch (x) { gpg.textContent = x.message; }
      gbody.style.opacity = 1;
    };
    gpg.addEventListener("click", e => { const b = e.target.closest("[data-off]"); if (!b) return; e.preventDefault(); st.offset = +b.dataset.off; load(true); });
    const sortBy = th => {
      const k = th.dataset.gs;
      if (k === st.sort) st.dir = st.dir === "asc" ? "desc" : "asc";
      else { st.sort = k; st.dir = k === "name" ? "asc" : "desc"; }
      st.offset = 0;
      $$("th.srt", root).forEach(x => { const on = x === th; x.classList.toggle("on", on); x.querySelector("i").textContent = on ? (st.dir === "asc" ? " ▴" : " ▾") : ""; x.setAttribute("aria-sort", on ? (st.dir === "asc" ? "ascending" : "descending") : "none"); });
      load();
    };
    $$("th.srt", root).forEach(th => { th.addEventListener("click", () => sortBy(th)); th.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); sortBy(th); } }); });
  }
  if (rep) rep.addEventListener("click", async e => {
    e.preventDefault(); const reason = prompt("Report this picture to the Fraglink moderators. What's wrong with it? (optional)"); if (reason === null) return;
    try { await api("/users/" + encodeURIComponent(p.handle) + "/avatar/report", { method: "POST", body: { reason } }); rep.textContent = "Reported. Thanks."; } catch (x) { alert(x.message); }
  });
  $$("[data-copy]", root).forEach(b => b.addEventListener("click", () => { navigator.clipboard && navigator.clipboard.writeText(b.dataset.copy); b.textContent = "Copied"; setTimeout(() => b.textContent = "Copy", 1500); }));
};
function rank(p) { return !p ? 0 : p.status === "online" ? (p.game_name ? 3 : 2) : p.status === "away" ? 1 : 0; }
function friendRow(f, opts) {
  const p = f.presence || {};
  const st = p.status === "online" ? (p.game_name ? "● playing" : "● online") : p.status === "away" ? "away" : "offline";
  const game = p.status !== "offline" && p.game_name ? `<div class="gm">${icon(p.game_id, p.game_name)}${isUnlisted(p.game_id) ? "Playing a game" : esc(p.game_name)}</div>` : "";
  return `<div class="fr" data-h="${esc(f.handle)}">${f.avatar ? avatar(f.avatar, 32) : `<div class="fa ${tint(f.handle)}">${initial(f.display_name || f.handle)}</div>`}<div class="who">
    <div class="nm"><span><a href="/u/${encodeURIComponent(f.handle)}">${esc(f.display_name || f.handle)}</a>${f.display_name && f.display_name !== f.handle ? ` <small>${esc(f.handle)}</small>` : ""}</span><span class="st ${esc(p.status || "")}">${st}</span></div>${game}</div>
    ${opts && opts.actions ? `<div class="acts">${opts.actions}</div>` : ""}</div>`;
}

/* ----- friends ----- */
pages.friends = async () => {
  if (!requireLogin()) return;
  const root = $("#friends"), msg = $("#msg");
  const FP = 50, fs = { offset: 0, q: "", show: "all" };
  async function load() {
    let fl, act;
    const qs = new URLSearchParams({ limit: FP, offset: fs.offset, q: fs.q, show: fs.show });
    try { [fl, act] = await Promise.all([api("/friends/page?" + qs), api("/friends/activity?tz=" + encodeURIComponent(TZ)).catch(() => ({ items: [] }))]); }
    catch (e) { if (!token()) return requireLogin(); showErr(msg, e); return; }
    await loadIcons([].concat(fl.friends.map(f => f.presence && f.presence.game_id), act.items.map(i => i.game_id)));
    const btns = `<button class="btn sm" data-act="remove">Remove</button>`;
    const reqIn = fl.incoming.map(r => `<div class="fr" data-h="${esc(r.handle)}"><div class="fa ${tint(r.handle)}">${initial(r.display_name || r.handle)}</div><div class="who"><div class="nm"><a href="/u/${encodeURIComponent(r.handle)}">${esc(r.display_name || r.handle)}</a></div><div class="gm">wants to be friends</div></div>
      <div class="acts"><button class="btn sm dl" data-act="accept">Accept</button><button class="btn sm" data-act="decline">Decline</button><button class="btn sm" data-act="block">Block</button></div></div>`).join("");
    const reqOut = fl.outgoing.map(r => `<div class="fr" data-h="${esc(r.handle)}"><div class="fa ${tint(r.handle)}">${initial(r.handle)}</div><div class="who"><div class="nm"><a href="/u/${encodeURIComponent(r.handle)}">${esc(r.display_name || r.handle)}</a></div><div class="gm">request sent ${dateFmt.format(new Date(r.created_at))}</div></div>
      <div class="acts"><button class="btn sm" data-act="cancel">Cancel</button></div></div>`).join("");
    let feed = "", last = "";
    act.items.slice(0, 20).forEach(i => {
      const d = actDay(i); if (d !== last) { feed += `<div class="day">${d}</div>`; last = d; }
      feed += `<div class="ev">${icon(i.game_id, i.game_name)}<p><a href="/u/${encodeURIComponent(i.handle)}">${esc(i.display_name || i.handle)}</a> ${actText(i)}</p></div>`;
    });
    root.innerHTML = `<div class="grid"><div class="col">
      ${fl.incoming.length ? `<section class="panel"><h2>Friend requests (${fl.incoming_count})</h2><div>${reqIn}</div></section>` : ""}
      <section class="panel"><h2>Friends (${fl.all})${fl.all ? ` <span class="more" style="text-transform:none">${fl.online} online</span>` : ""}</h2>
        ${fl.all > 0 ? `<div class="lbar" style="padding:0 10px">${searchBox("Find a friend", "fr-q")}<select class="t sm" data-fshow aria-label="Show"><option value="all">Everyone</option><option value="online">Online</option><option value="offline">Offline</option></select></div>` : ""}
        <div data-flist>${fl.friends.length ? fl.friends.map(f => friendRow(f, { actions: btns })).join("") : `<div class="body empty">${fs.q ? `No friends match “${esc(fs.q)}”. <a href="#" data-fclear>Clear search</a> · <a href="/u/${encodeURIComponent(fs.q)}">Look up “${esc(fs.q)}”</a>` : fl.all ? (fs.show === "online" ? "Nobody's online right now." : "Everyone's online.") : "No friends yet. Add someone by their Fraglink username."}</div>`}</div>
        <div data-fpager>${fl.total > FP ? pagerHtml(fs.offset, FP, fl.total, fs.q) : ""}</div></section>
      ${fl.outgoing.length ? `<section class="panel"><h2>Sent requests (${fl.outgoing_count})</h2><div>${reqOut}</div></section>` : ""}
    </div><aside class="col">
      <section class="panel"><h2>Add a friend</h2><div class="body"><form ${FA} class="f" id="addf"><label>Username<input type="text" name="handle" maxlength="20" required placeholder="their Fraglink username" autocomplete="off"></label><div class="row"><button class="btn">Send request</button></div></form></div></section>
      <section class="panel"><h2>Friends' activity</h2><div>${feed || `<div class="body empty">Nothing in the last 7 days.</div>`}</div></section>
      ${fl.blocked.length ? `<section class="panel"><h2>Blocked</h2><div>${fl.blocked.map(h => `<div class="fr" data-h="${esc(h)}"><div class="who">${esc(h)}</div><div class="acts"><button class="btn sm" data-act="unblock">Unblock</button></div></div>`).join("")}</div></section>` : ""}
    </aside></div>`;
    const fq = $("#fr-q"), fsel = $("[data-fshow]");
    if (fq) {
      fq.value = fs.q; fsel.value = fs.show; if (fs.q) { fq.parentNode.querySelector(".sx-clear").hidden = false; }
      let t; wireSearch(fq, () => { clearTimeout(t); t = setTimeout(() => { fs.q = fq.value.trim(); fs.offset = 0; load().then(() => { const n = $("#fr-q"); n.focus(); n.setSelectionRange(n.value.length, n.value.length); }); }, 250); });
      fsel.onchange = () => { fs.show = fsel.value; fs.offset = 0; load(); };
    }
    const fpg = $("[data-fpager]");
    if (fpg) fpg.addEventListener("click", e => { const b = e.target.closest("[data-off]"); if (!b) return; fs.offset = +b.dataset.off; load().then(() => $("[data-flist]").closest("section").scrollIntoView({ block: "start" })); });
    const fcl = $("[data-fclear]"); if (fcl) fcl.addEventListener("click", e => { e.preventDefault(); fs.q = ""; fs.offset = 0; load(); });
    $("#addf").addEventListener("submit", async e => {
      e.preventDefault(); const h = e.target.handle.value.trim().replace(/^@/, ""); if (!h) return;
      try { const r = await api("/friends/request", { method: "POST", body: { handle: h } });
        showOk(msg, r && r.status === "accepted" ? `You and ${h} are now friends.` : `Friend request sent to ${h}.`); load(); refreshFriendsLive(); }
      catch (x) { showErr(msg, x); }
    });
  }
  root.addEventListener("click", async e => {
    const b = e.target.closest("[data-act]"); if (!b) return;
    const h = b.closest("[data-h]").dataset.h, a = b.dataset.act;
    if ((a === "remove" || a === "block") && !confirm(a === "remove" ? `Remove ${h} from your friends?` : `Block ${h}? They won't be able to send you requests.`)) return;
    b.disabled = true;
    try {
      if (a === "accept" || a === "decline") await api("/friends/respond", { method: "POST", body: { handle: h, accept: a === "accept" } });
      else if (a === "remove" || a === "cancel") await api("/friends/" + encodeURIComponent(h), { method: "DELETE" });
      else if (a === "block") await api("/blocks", { method: "POST", body: { handle: h } });
      else if (a === "unblock") await api("/blocks/" + encodeURIComponent(h), { method: "DELETE" });
      msg.hidden = true; load(); refreshFriendsLive();
    } catch (x) { b.disabled = false; showErr(msg, x); }
  });
  onFriendsLive(() => { if (document.body.dataset.page === "friends") load(); });
  load();
};

/* ----- settings (tabs) ----- */
const RIG_FIELDS = [["manufacturer", "Manufacturer", "e.g. Custom build, Alienware"], ["cpu", "Processor", "e.g. Ryzen 7 7800X3D"], ["gpu", "Video card", "e.g. RTX 4070"], ["memory", "Memory", "e.g. 32 GB DDR5"], ["storage", "Storage", "e.g. 2 TB NVMe"], ["motherboard", "Motherboard", ""], ["monitor", "Monitor", "e.g. 27\" 1440p 165 Hz"], ["headset", "Headset", ""], ["keyboard", "Keyboard", ""], ["mouse", "Mouse", ""], ["mousepad", "Mouse pad", ""], ["case", "Case", ""], ["os", "Operating system", "e.g. Windows 11"]];
const STYLES = ["Casual", "Weekend warrior", "Competitive", "Completionist", "MMO addict", "Sniper only", "Support main", "Speedrunner", "Explorer", "Clueless"];
/* Settings > Account > Time zone: every IANA zone the browser knows, grouped by region. */
const ZONES = (() => { let z = []; try { z = Intl.supportedValuesOf("timeZone"); } catch (e) { /* old browser */ }
  if (!z.length) z = ["Africa/Johannesburg", "America/Chicago", "America/Los_Angeles", "America/New_York", "America/Sao_Paulo", "Asia/Kolkata", "Asia/Singapore", "Asia/Tokyo", "Australia/Brisbane", "Australia/Perth", "Australia/Sydney", "Europe/Berlin", "Europe/London", "Pacific/Auckland"];
  return z.includes("UTC") ? z : z.concat("UTC"); })();
const zoneOff = z => { try { const p = new Intl.DateTimeFormat("en", { timeZone: z, timeZoneName: "shortOffset" }).formatToParts(new Date()).find(x => x.type === "timeZoneName"); return p ? p.value.replace("GMT", "UTC") : ""; } catch (e) { return ""; } };
const zoneLabel = z => { const i = z.indexOf("/"); return (i < 0 ? z : z.slice(i + 1)).replace(/_/g, " ").replace(/\//g, " / ") + ` (${zoneOff(z) || "UTC"})`; };
function zoneOptions(cur, term) {
  const t = (term || "").trim().toLowerCase(), groups = new Map();
  const list = cur && !ZONES.includes(cur) ? ZONES.concat(cur) : ZONES;
  list.forEach(z => { const l = zoneLabel(z); if (t && z !== cur && !(z.toLowerCase().includes(t) || l.toLowerCase().includes(t))) return;
    const g = z.includes("/") ? z.split("/")[0] : "Other"; if (!groups.has(g)) groups.set(g, []); groups.get(g).push([z, l]); });
  const auto = `<option value=""${cur ? "" : " selected"}>Automatic (browser): ${esc(TZ ? `${TZ} (${zoneOff(TZ) || "UTC"})` : "not detected, Australia/Sydney")}</option>`;
  return auto + Array.from(groups.keys()).sort().map(g => `<optgroup label="${esc(g)}">${groups.get(g).sort((a, b) => a[1].localeCompare(b[1])).map(([z, l]) => `<option value="${esc(z)}"${z === cur ? " selected" : ""}>${esc(l)}</option>`).join("")}</optgroup>`).join("");
}
const BASE_TABS = [["account", "Account & profile"], ["rig", "Gaming rig"], ["games", "Played games"], ["privacy", "Privacy"], ["notifications", "Notifications"], ["connections", "Connections"], ["security", "Security"]];
pages.settings = async () => {
  if (!requireLogin()) return;
  const root = $("#settings");
  let m;
  try { m = await api("/me"); } catch (e) { if (!token()) return requireLogin(); root.innerHTML = `<div class="alert err">${esc(e.message)}</div>`; return; }
  let u = m.user;
  const keep = () => setSession(token(), Object.assign(me() || {}, { id: u.id, handle: u.handle, display_name: u.display_name, email: u.email, avatar: u.avatar, email_verified: u.email_verified, mail_live: u.mail_live, staff_role: u.staff_role || null, timezone: u.timezone || null }));
  keep(); renderHeader(); verifyBar();
  const tabs = () => {
    const t = BASE_TABS.slice();
    if (u.has_wow) { const i = t.findIndex(x => x[0] === "games"); t.splice(i + 1, 0, ["wow", "World of Warcraft"]); }
    return t;
  };
  const tab = () => { const t = location.hash.replace("#", "").split("/")[0]; return tabs().some(x => x[0] === t) ? t : "account"; };
  const row = (label, html, hint) => `<label>${label}</label><div>${html}${hint ? `<div class="sub">${hint}</div>` : ""}</div>`;
  const msg = `<div class="alert" hidden data-msg></div>`;
  const ok = (el, t) => showOk($("[data-msg]", el), t), bad = (el, e) => showErr($("[data-msg]", el), e);

  function accountTab() {
    const verified = u.email ? (u.email_verified ? `<span style="color:var(--online)">✓ confirmed</span>` : `<span style="color:var(--away)">not confirmed</span> · ${u.mail_live ? `<a href="#" data-resend>Send the link again</a>` : `<span class="dimt">confirmation emails aren't switched on yet</span>`}`) : `<span class="empty">none</span>`;
    const canUp = u.avatar_upload_allowed;
    return `<h3>Profile picture</h3>${msg}<div class="frm">
      ${row("Current", `<div class="avsizes"><div>${avatar(u.avatar, 184)}<small>184 profile</small></div><div>${avatar(u.avatar, 64)}<small>64 panels</small></div><div>${avatar(u.avatar, 32)}<small>32 lists</small></div></div>`)}
      ${row("Upload", `<div class="row"><input type="file" id="av-file" accept="image/png,image/jpeg,image/webp,image/gif" hidden><button class="btn blue" type="button" data-upload ${canUp ? "" : "disabled"}>Upload a picture…</button><button class="btn" type="button" data-avreset>Reset to default</button></div>`,
        canUp ? "PNG, JPEG, WebP or GIF (first frame), up to 2 MB. Cropped to a square. Keep it clean, pictures can be reported and taken down. 5 changes a day." : esc(u.avatar_upload_block || "Uploads aren't available right now."))}
      ${row("Or pick a preset", `<div class="presets">${PRESET_GLYPHS.map((g, i) => `<button type="button" data-preset="${i}" class="${u.avatar.kind === "preset" && u.avatar.preset === i ? "on" : ""}" title="Preset ${i + 1}">${avatar({ kind: "preset", preset: i }, 48)}</button>`).join("")}</div>`, "Your default colour comes from your account.")}
    </div>
    <h3>Account</h3><form ${FA} data-f="name">${msg}<div class="frm">
      ${row("Username", `<b>${esc(u.handle)}</b> <small style="color:#8a96a3">playfraglink.com/u/${esc(u.handle)}</small>`, "Your sign-in name and profile address. It's permanent; ask staff if it really needs changing.")}
      ${row("Nickname", `<input class="t" name="display_name" maxlength="32" value="${esc(u.display_name || "")}">`, "The name everyone sees, in the app and on the site. Up to 32 characters, 3 changes a day. It can't be someone else's name or look like staff. Leave empty to use your username.")}
      ${row("Email", `${esc(u.email || "")} ${verified}`)}
      ${row("", `<button class="btn dl">Save</button>`)}</div></form>
    <h3>Time zone</h3><form ${FA} data-f="tz">${msg}<div class="frm">
      ${row("Time zone", `<div class="tzpick"><input class="t" type="search" data-tzq placeholder="Search, e.g. London or New York" aria-label="Search time zones" autocomplete="off"><select class="t" name="timezone" aria-label="Time zone">${zoneOptions(u.timezone || "", "")}</select></div>`, "Your week (Hours this week, Past 7 days, leaderboards) and the days and times on the site use this zone, on every device and in the app. Automatic uses this browser's zone.")}
      ${row("", `<button class="btn dl">Save</button>`)}</div></form>
    <h3>About you</h3><form ${FA} data-f="about">${msg}<div class="frm">
      ${row("About me", `<textarea class="t" name="about" maxlength="300">${esc(u.about || "")}</textarea>`, "Up to 300 characters.")}
      ${row("Location", `<input class="t" name="location" maxlength="40" value="${esc(u.location || "")}" placeholder="e.g. Sydney, Australia">`, "Optional. Shown on your profile as typed.")}
      ${row("Country", `<select class="t" name="country"><option value="">(not shown)</option>${COUNTRIES().map(([c, n]) => `<option value="${c}" ${(u.details || {}).country === c ? "selected" : ""}>${esc(n)}</option>`).join("")}</select>`, "Shows a small flag next to your location.")}
      ${row("Real name", `<input class="t" name="real_name" maxlength="40" value="${esc((u.details || {}).real_name || "")}">`, "Optional.")}
      ${row("Gender", `<input class="t" name="gender" maxlength="20" value="${esc((u.details || {}).gender || "")}">`, "Optional, as you'd like it shown.")}
      ${row("Gaming style", `<select class="t" name="gaming_style"><option value="">(none)</option>${STYLES.map(s => `<option ${u.gaming_style === s ? "selected" : ""}>${s}</option>`).join("")}</select>`)}
      ${row("", `<button class="btn dl">Save</button>`)}</div></form>
    <h3>Change email</h3><form ${FA} data-f="email">${msg}<div class="frm">
      ${row("New email", `<input class="t" type="email" name="new_email" required autocomplete="email">`)}
      ${row("Current password", `<input class="t" type="password" name="current_password" required autocomplete="current-password">`)}
      ${row("", `<button class="btn">Change email</button>`)}</div></form>
    <h3>Your data</h3>${msg}<div class="frm">
      ${row("Download my data", `<button class="btn" type="button" data-export>Download (zip)</button>`, "Everything we store about you: account, sessions, hours, friends, as JSON and CSV. Once an hour.")}
      ${row("Delete my account", `<button class="btn red" type="button" data-delete>Delete account…</button>`, "Signs you out everywhere now and deletes everything after 7 days. Signing in again during those 7 days cancels it.")}
    </div><form ${FA} data-f="delete" hidden>${msg}<div class="frm"><div class="danger" style="grid-column:1/-1">Your profile disappears now and your account and all data are deleted for good after 7 days.</div>
      ${row("Password", `<input class="t" type="password" name="password" required autocomplete="current-password">`)}
      ${u.mfa && u.mfa.enabled ? row("Two-step code", `<input class="t" name="code" required autocomplete="one-time-code">`) : ""}
      ${row("", `<button class="btn red">Delete my account</button>`)}</div></form>`;
  }
  function rigTab() {
    const r = u.rig || {};
    return `<h3>Gaming rig</h3><form ${FA} data-f="rig">${msg}<div class="frm">${RIG_FIELDS.map(([k, l, ph]) => row(l, `<input class="t" name="${k}" maxlength="60" value="${esc(r[k] || "")}" placeholder="${esc(ph)}">`)).join("")}
      ${row("", `<button class="btn dl">Save rig</button>`, "Shown on your profile. Leave everything empty to hide the box.")}</div></form>`;
  }
  async function gamesTab(el) {
    const PG = 50, st = { offset: 0 };
    el.innerHTML = `<h3>Played games</h3>${msg}<div class="empty-v2">Loading…</div>`;
    let first; try { first = await api(`/me/games?limit=${PG}&tz=${encodeURIComponent(TZ)}`); } catch (e) { bad(el, e); return; }
    if (!first.all) { el.innerHTML = `<h3>Played games</h3>${msg}<div class="empty-v2">No games yet. Games show up here after you play with the Fraglink app running.</div>`; return; }
    el.innerHTML = `<h3>Played games</h3>${msg}
      <p class="pg-help">Switch a game off to hide it from your profile, signature image and the home page. It still counts in your own totals.</p>
      <div class="lbar">${searchBox("Search your games", "pg-q")}
        <select class="t sm" data-sort aria-label="Sort"><option value="total">Most played</option><option value="recent">Recently played</option><option value="week">Past 7 days</option><option value="name">Name A–Z</option></select>
        <select class="t sm" data-show aria-label="Show"><option value="all">All games</option><option value="shown">On profile</option><option value="hidden">Hidden</option></select>
        <span class="lcount" data-count></span></div>
      <div class="pg2" data-list></div><div class="empty-v2" data-none hidden></div><div data-pager></div>`;
    const list = $("[data-list]", el), q = $("#pg-q", el), sortSel = $("[data-sort]", el), showSel = $("[data-show]", el), pager = $("[data-pager]", el), none = $("[data-none]", el);
    let rows = [];
    const rowH = x => `<div class="pgr${x.hidden ? " hid" : ""}" data-g="${esc(x.game_id)}"><span class="gico">${icon(x.game_id, x.name)}</span>
        <span class="nm">${esc(x.name)}<small>${hours(x.total_min)} total · ${x.minutes_7d ? hours(x.minutes_7d) + " this week · " : ""}${lastPlayed(x.last_played_at)}</small></span>
        <label class="sw" title="${x.hidden ? "Hidden from your profile" : "Shown on your profile"}"><input type="checkbox" data-show-g ${x.hidden ? "" : "checked"}><i></i><span>${x.hidden ? "Hidden" : "On profile"}</span></label>
        <button class="lnk danger" type="button" data-reset title="Delete all hours for this game">Reset hours</button></div>`;
    const render = async (r, keepPos) => {
      rows = r.games; await loadIcons(rows.map(x => x.game_id));
      list.innerHTML = rows.map(rowH).join("");
      const term = q.value.trim();
      $("[data-count]", el).textContent = `${r.all} game${r.all === 1 ? "" : "s"}${r.hidden ? ` · ${r.hidden} hidden` : ""}`;
      none.hidden = rows.length > 0;
      none.innerHTML = term ? `No games match “${esc(term)}”. <a href="#" data-clear>Clear search</a>` : showSel.value === "hidden" ? "No hidden games." : "No games here.";
      pager.innerHTML = r.total > PG ? pagerHtml(st.offset, PG, r.total, term) : "";
      if (keepPos && el.getBoundingClientRect().top < 0) el.scrollIntoView({ block: "start" });
    };
    const load = async keepPos => {
      const qs = new URLSearchParams({ limit: PG, offset: st.offset, sort: sortSel.value, show: showSel.value, q: q.value.trim() });
      list.style.opacity = .5;
      try { await render(await api("/me/games?" + qs + "&tz=" + encodeURIComponent(TZ)), keepPos); } catch (x) { bad(el, x); }
      list.style.opacity = 1;
    };
    let t; wireSearch(q, () => { clearTimeout(t); t = setTimeout(() => { st.offset = 0; load(); }, 200); });
    sortSel.onchange = showSel.onchange = () => { st.offset = 0; load(); };
    pager.addEventListener("click", e => { const b = e.target.closest("[data-off]"); if (b) { st.offset = +b.dataset.off; load(true); } });
    none.addEventListener("click", e => { if (e.target.closest("[data-clear]")) { e.preventDefault(); q.value = ""; q.dispatchEvent(new Event("input")); q.focus(); } });
    render(first);
    list.addEventListener("change", async e => {
      const c = e.target.closest("[data-show-g]"); if (!c) return; const r = c.closest(".pgr"), x = rows.find(y => y.game_id === r.dataset.g), hidden = !c.checked;
      try { await api("/me/games/hide", { method: "POST", body: { game_id: x.game_id, hidden } }); x.hidden = hidden; r.classList.toggle("hid", hidden); r.querySelector(".sw span").textContent = hidden ? "Hidden" : "On profile"; }
      catch (err) { c.checked = !c.checked; bad(el, err); }
    });
    list.addEventListener("click", async e => {
      const b = e.target.closest("[data-reset]"); if (!b) return; const r = b.closest(".pgr"), x = rows.find(y => y.game_id === r.dataset.g);
      const pw = prompt(`Delete every hour recorded for ${x.name}? This can't be undone.\n\nEnter your password to confirm:`); if (!pw) return;
      try { await api("/me/games/reset", { method: "POST", body: { game_id: x.game_id, password: pw } }); ok(el, `Hours for ${x.name} reset.`); load(); } catch (err) { bad(el, err); }
    });
  }
  function privacyTab() {
    const share = siteBase() + "/u/" + encodeURIComponent(u.handle);
    const pv = u.privacy || {};
    const priv = !u.public_profile;
    const sw = (k, label) => `<label style="display:block;margin:3px 0"><input type="checkbox" data-pv="${k}" ${pv[k] !== false ? "checked" : ""}>${label}</label>`;
    return `<h3>Privacy</h3><form ${FA} data-f="privacy">${msg}<div class="frm">
      ${(() => {
        const dob = (u.details && u.details.birth_date) || null;
        if (dob) {
          return row("Date of birth", `<span>${esc(dob)}</span>${u.is_minor ? ' <span class="muted">(under 18 · private by default)</span>' : ""}`,
            "Your date of birth can't be changed here. Email support@playfraglink.com if it needs a correction.");
        }
        if (!sessionStorage.getItem("fl_dob_prompted")) sessionStorage.setItem("fl_dob_prompted", "1");
        const days = Array.from({length:31},(_,i)=>`<option value="${String(i+1).padStart(2,"0")}">${i+1}</option>`).join("");
        const months = ["January","February","March","April","May","June","July","August","September","October","November","December"].map((n,i)=>`<option value="${String(i+1).padStart(2,"0")}">${n}</option>`).join("");
        const years = Array.from({length:new Date().getFullYear()-1899},(_,i)=>new Date().getFullYear()-i).map(y=>`<option value="${y}">${y}</option>`).join("");
        return row("Date of birth", `<span class="dob"><select name="dob_day" aria-label="Day"><option value="">Day</option>${days}</select><select name="dob_month" aria-label="Month"><option value="">Month</option>${months}</select><select name="dob_year" aria-label="Year"><option value="">Year</option>${years}</select><button class="btn sm" type="button" data-save-dob>Save</button></span>`,
          "Please add your date of birth. You must be at least 13. Under-18 accounts start private; you can switch to public any time. Once saved, only support can correct it.");
      })()}${row("Private profile", `<label><input type="checkbox" name="private_profile" ${priv ? "checked" : ""}>Only my friends can see my profile</label>`, u.is_minor ? "Under-18 accounts start private. You can make your profile public any time; everyone will be able to see it." : "Your profile is public by default. Private: everyone else just sees your name and picture, you're left out of the home page lists, and your signature image is switched off.")}
      ${row("Show on my profile", `${sw("now_playing", "Now Playing (the game I'm in right now)")}${sw("games", "Games &amp; hours")}<label style="display:block;margin:3px 0"><input type="checkbox" name="share_activity" ${u.share_activity ? "checked" : ""}>Activity (recent play history)</label><label style="display:block;margin:3px 0"><input type="checkbox" name="show_friends" ${u.show_friends ? "checked" : ""}>Friends list</label>${sw("rig", "Gaming rig")}${sw("name_history", "Previous nicknames (\"Previously known as\")")}`, "Hiding Now Playing also keeps you off the home page Now Playing list and your signature image. Your friends still see your status in the app; pick Invisible there to hide it.")}
      ${row("Personal details", `${sw("about", "About me")}${sw("location", "Location &amp; country")}${sw("gaming_style", "Gaming style")}${sw("real_name", "Real name")}${sw("gender", "Gender")}${sw("age", "Age")}`)}
      ${row("Achievements", `${sw("achievements", "Show my achievements")}`, "Needs Games &amp; hours shown.")}
      ${FEATURES.wow_characters ? row("WoW characters", `${sw("wow_characters", "Show my WoW characters and /played")}`, "Cards for each character from the Fraglink WoW addon. You can also hide single characters on your profile.") : ""}
      ${row("Connected accounts", `${sw("connection_names", "Show connected account names")}`, "Off: visitors still see your Steam, Battle.net and Discord icons with the verified tick, but not your account names or links. You always see your own.")}
      ${row("In the app", `<span class="sub">Whether friends see the game you're in right now (and friends of friends) is set per PC in the desktop app: Options &gt; Privacy. The switches on this page show there too.</span>`)}
      ${row("Leaderboards", `${sw("leaderboards", "Show me in Top players on the home page")}`, "Needs Games &amp; hours shown.")}
      ${row("Your link", `<div class="share"><input class="t" readonly value="${esc(share)}"><button class="btn sm" type="button" data-copy="${esc(share)}">Copy</button></div>`)}
      ${!priv ? row("Signature image", `${sigPreview(u.handle)}<div class="share" style="margin-top:6px"><input class="t" readonly value="${esc(sigUrl(u.handle))}"><button class="btn sm" type="button" data-copy="${esc(sigUrl(u.handle))}">Copy</button></div>`, "Paste the image link into forum signatures. It refreshes within about 30 seconds. New accounts show a default card until you have hours.") : ""}
      </div></form>`;
  }
  function notificationsTab() {
    const n = u.notify_prefs || {};
    const cb = (k, t) => `<label style="display:block;margin:2px 0"><input type="checkbox" name="${k}" ${n[k] ? "checked" : ""}>${t}</label>`;
    return `<h3>Notifications</h3><form ${FA} data-f="notify">${msg}<div class="frm">
      ${row("Email", cb("signin_email", "New sign-in to my account") + cb("friend_request_email", "Someone sends me a friend request") + cb("news_email", "Fraglink news (a few times a year)"), u.mail_live ? "" : "Email delivery isn't switched on yet. Your choices are saved for when it is.")}
      ${row("Desktop app", cb("friend_request_client", "Friend requests") + cb("friend_online_client", "A friend comes online or starts a game"), "The same switches as Options > General in the desktop app; a change here shows there next time you open Options.")}
      </div></form>`;
  }
  async function connectionsTab(el) {
    el.innerHTML = `<h3>Connections</h3><div data-conn>${msg}<div class="empty-v2">Loading…</div></div>`;
    const box = $("[data-conn]", el);
    const q = new URLSearchParams(location.search), NAMES = { steam: "Steam", discord: "Discord", bnet: "Battle.net" };
    let c;
    try { c = await api("/connect"); } catch (x) { return bad(box, x); }
    const card = (p, logo, blurb) => {
      const v = c[p] || {}, l = v.linked;
      if (!l && !v.off && !v.available) return ""; // not built yet: hide rather than show a "Coming soon" stub
      const right = l
        ? `${p === "steam" && v.import ? `<button class="btn sm" type="button" data-import>Import hours</button> ` : ""}${p === "bnet" && u.has_wow ? `<a class="btn sm" href="/settings#wow">Manage WoW</a> ` : ""}<button class="btn sm" type="button" data-unlink="${p}">Unlink</button>`
        : v.off ? `<span class="btn sm" aria-disabled="true">Turned off for now</span>` : v.available ? `<button class="btn sm blue" type="button" data-link="${p}">${p === "steam" ? "Sign in through Steam" : "Link " + NAMES[p]}</button>` : `<span class="btn sm" aria-disabled="true">Coming soon</span>`;
      const who = l ? `${p === "steam" ? `<a href="https://steamcommunity.com/profiles/${esc(l.uid)}" rel="noopener">${esc(l.display || l.uid)}</a>` : `<b>${esc(l.display || l.uid)}</b>`} ${l.verified ? `<span class="vbadge">✓ Verified</span>` : `<span class="sub">(not verified)</span>`}${l.last_sync_at ? `<div class="sub">Hours imported ${fullDate.format(new Date(l.last_sync_at))}${l.sync_status === "private" ? " · Steam game details were private" : ""}</div>` : ""}${p === "bnet" ? (v.wow_imported_at ? `<div class="sub">Hours imported ${fullDate.format(new Date(v.wow_imported_at))}</div>` : `<div class="sub"><a href="/connect/battlenet">No /played upload yet</a></div>`) : ""}`
        : `<span class="sub">${blurb}</span>`;
      return `<div class="conn"><span class="cl cl-${p}">${logo}</span><div style="flex:1"><b style="display:block">${NAMES[p]}</b>${who}</div><div>${right}</div></div>`;
    };
    box.innerHTML = msg + `<div style="padding:0 14px">
      ${card("steam", `<img src="/assets/brand-steam.svg" alt="" width="22" height="22">`, c.steam && c.steam.import ? "Verify your Steam account and import your Steam hours." : "Verify your Steam account. It shows as a badge on your profile.")}
      ${card("discord", `<img src="/assets/brand-discord.svg" alt="" width="22" height="22">`, "Show your Discord name on your profile.")}
      ${card("bnet", `<img src="/assets/brand-battlenet.svg" alt="" width="22" height="22">`, "Show your BattleTag on your profile.")}
      ${c.bnet && c.bnet.needs_reconnect ? `<div class="conn-sub"><span class="sub warn">Reconnect Battle.net to verify characters.</span> ${u.has_wow ? `<a href="/settings#wow">Manage WoW</a>` : ""}</div>` : ""}
      <p class="sub" style="padding:8px 0">Verified accounts show as badges on your profile. You're sent to the provider's own sign-in page; Fraglink never sees your password there.</p></div>`;
    if (q.get("connected")) { ok(box, `${NAMES[q.get("connected")] || "Account"} linked.`); history.replaceState(null, "", "/settings#connections"); }
    else if (q.get("connect_error")) { bad(box, new Error(q.get("connect_error") === "expired" ? "That link expired. Try again." : q.get("connect_error") === "cancelled" ? "Linking was cancelled." : q.get("connect_error"))); history.replaceState(null, "", "/settings#connections"); }
    box.onclick = async e => {
      const b = e.target.closest("button"); if (!b) return;
      try {
        if (b.dataset.link) { b.disabled = true; const r = await api(`/connect/${b.dataset.link}/start`, { method: "POST" }); location.href = r.url; }
        else if (b.dataset.unlink) { if (!confirm(`Unlink ${NAMES[b.dataset.unlink]}?`)) return; await api(`/connect/${b.dataset.unlink}`, { method: "DELETE" }); connectionsTab(el); }
        else if (b.matches("[data-import]")) { b.disabled = true; const r = await api("/steam/sync", { method: "POST" }); await connectionsTab(el); ok($("[data-conn]", el), `Imported ${r.games} games.`); }
      } catch (x) { b.disabled = false; bad(box, x); }
    };
  }
  async function securityTab(el) {
    const mf = u.mfa || {};
    el.innerHTML = `<h3>Password</h3><form ${FA} data-f="pw">${msg}<div class="frm">
      ${row("Current password", `<input class="t" type="password" name="current_password" required autocomplete="current-password">`)}
      ${row("New password", `<input class="t" type="password" name="new_password" required minlength="10" maxlength="128" autocomplete="new-password">`)}
      ${row("New password again", `<input class="t" type="password" name="new_password2" required minlength="10" maxlength="128" autocomplete="new-password">`)}
      ${row("", `<button class="btn">Change password</button>`, "Every other device is signed out.")}</div></form>
      <h3>Two-step sign-in</h3><div data-mfa>${msg}<div class="frm">${mf.enabled
        ? row("Status", `<b style="color:var(--online)">On</b> · ${mf.backup_codes_left} backup code${mf.backup_codes_left === 1 ? "" : "s"} left`) +
          row("Password", `<input class="t" type="password" name="password" autocomplete="current-password">`) + row("Code", `<input class="t" name="code" autocomplete="one-time-code" placeholder="6-digit code or backup code">`) +
          row("", `<button class="btn" type="button" data-mfa-codes>New backup codes</button> <button class="btn red" type="button" data-mfa-off>Turn off</button>`)
        : row("Status", `Off`, "Ask for a code from an authenticator app (Google Authenticator, Microsoft Authenticator, 1Password, Authy...) when you sign in on the website or the app.") +
          row("Password", `<input class="t" type="password" name="password" autocomplete="current-password">`) + row("", `<button class="btn blue" type="button" data-mfa-setup>Set up two-step sign-in</button>`)}</div><div data-mfa-step></div></div>
      <h3>Where you're signed in</h3><div data-sess>${msg}<div class="empty-v2">Loading…</div></div>`;
    loadSessions(el);
  }
  async function loadSessions(el) {
    const box = $("[data-sess]", el);
    let s; try { s = await api("/me/sessions"); } catch (e) { bad(box, e); return; }
    box.innerHTML = `${msg}${s.sessions.map(x => `<div class="sess" data-id="${esc(x.id)}"><div class="d"><b>${esc(x.device)}</b>${x.current ? `<span class="cur">This browser</span>` : ""}<small>${x.kind === "device" ? "Desktop app" : "Website"} · ${x.location ? esc(x.location) + " · " : ""}${x.ip ? esc(x.ip) + " · " : ""}signed in ${fullDate.format(new Date(x.created_at))} · last active ${since(x.last_used_at)} ago</small></div>${x.current ? "" : `<button class="btn sm" type="button" data-revoke>Sign out</button>`}</div>`).join("")}
      <div style="padding:10px 14px"><button class="btn red" type="button" data-revoke-all>Log out of all other devices</button></div>
      <div class="note" style="padding:0 14px 10px">Countries are a rough guess, looked up on our own server. <a href="https://db-ip.com" target="_blank" rel="noopener">IP Geolocation by DB-IP</a>. Signed-out sign-ins are removed after 30 days, and desktop apps unused for 180 days are signed out.</div>`;
  }

  function render() {
    const t = tab();
    root.innerHTML = `<section class="head"><div><h1>Account settings</h1><div class="sub">Your profile, gaming rig, privacy, notifications and linked accounts.</div></div></section><nav class="tabs">${tabs().map(([k, l]) => `<a href="#${k}" class="${k === t ? "on" : ""}">${l}</a>`).join("")}</nav><div class="tabpane" id="pane"></div>`;
    const el = $("#pane");
    if (t === "account") el.innerHTML = accountTab();
    else if (t === "rig") el.innerHTML = rigTab();
    else if (t === "games") gamesTab(el);
    else if (t === "wow") { el.dataset.embed = "1"; el.classList.add("bn"); wowManager(el); }
    else if (t === "privacy") el.innerHTML = privacyTab();
    else if (t === "notifications") el.innerHTML = notificationsTab();
    else if (t === "connections") connectionsTab(el);
    else if (t === "security") securityTab(el);
  }
  async function refresh() { u = (await api("/me")).user; keep(); renderHeader(); }
  window.addEventListener("hashchange", render);
  root.addEventListener("input", e => {
    if (!e.target.matches || !e.target.matches("[data-tzq]")) return;
    const sel = e.target.parentNode.querySelector("select[name=timezone]"); if (!sel) return;
    sel.innerHTML = zoneOptions(sel.value, e.target.value);
  });
  root.addEventListener("submit", async e => {
    const f = e.target.closest("form[data-f]"); if (!f) return; e.preventDefault();
    const k = f.dataset.f, v = n => f.elements[n] ? f.elements[n].value : undefined;
    try {
      if (k === "name") { const r = await api("/me/display_name", { method: "POST", body: { display_name: v("display_name") } }); u.display_name = r.display_name; keep(); renderHeader(); ok(f, r.changes_left == null ? "Saved." : `Saved. ${r.changes_left} more change${r.changes_left === 1 ? "" : "s"} today.`); }
      else if (k === "about" || k === "rig") {
        const rig = Object.assign({}, u.rig || {}); if (k === "rig") RIG_FIELDS.forEach(([n]) => { rig[n] = v(n); });
        const body = { about: k === "about" ? v("about") : u.about, location: k === "about" ? v("location") : u.location, gaming_style: (k === "about" ? v("gaming_style") : u.gaming_style) || null, rig };
        if (k === "about") {
          Object.assign(body, { country: v("country"), real_name: v("real_name"), gender: v("gender") });
        }
        const r = await api("/me/profile", { method: "POST", body }); Object.assign(u, r); ok(f, "Saved.");
      }
      else if (k === "tz") {
        const r = await api("/me/timezone", { method: "POST", body: { timezone: v("timezone") || null } });
        u.timezone = r.timezone; keep();
        ok(f, r.timezone ? `Saved. Your week and times now use ${zoneLabel(r.timezone)}.` : "Saved. Your week and times follow this browser's time zone.");
      }
      else if (k === "email") { await api("/me/email", { method: "POST", body: { current_password: v("current_password"), new_email: v("new_email").trim() } }); await refresh(); render(); ok($("#pane"), "Email changed. We sent a link to confirm the new address. Can't find it? Check your junk or spam folder."); }
      else if (k === "delete") {
        if (!confirm("Delete your Fraglink account? You'll be signed out everywhere and everything is deleted after 7 days.")) return;
        const r = await api("/me", { method: "DELETE", body: { password: v("password"), code: v("code") } }); clearSession(); location.href = "/?deleted=" + encodeURIComponent(r.delete_after || "1");
      }
      else if (k === "pw") {
        if (v("new_password") !== v("new_password2")) throw new Error("The new passwords don't match.");
        const r = await api("/me/password", { method: "POST", body: { current_password: v("current_password"), new_password: v("new_password") } }); f.reset(); ok(f, `Password changed. ${plural(r.signed_out_other_sessions || 0, "other session")} signed out.`);
      }
    } catch (x) {
      if (x.code === "account_paused") { clearSession(); alert(x.message); location.href = "/"; return; }
      bad(f, x);
    }
  });
  root.addEventListener("change", async e => {
    const f = e.target.closest("form[data-f]");
    if (e.target.id === "av-file") {
      const file = e.target.files[0]; e.target.value = ""; if (!file) return; const pane = $("#pane");
      if (file.size > 2 * 1024 * 1024) return bad(pane, new Error("Images can be up to 2 MB."));
      cropEditor(file, async crop => {
        const q = `?x=${crop.x}&y=${crop.y}&size=${crop.size}`;
        const r = await fetch(API + "/me/avatar" + q, { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": "application/octet-stream" }, body: file });
        const j = await r.json().catch(() => null); if (!r.ok) throw new Error((j && j.message) || "Upload failed (HTTP " + r.status + ").");
        await refresh(); render(); ok($("#pane"), "Picture updated.");
      }).catch(x => bad(pane, x));
      return;
    }
    if (!f) return;
    try {
      if (f.dataset.f === "privacy") {
        const t = e.target;
        // Unchecking "Private profile" = going public. Minors get a short, neutral confirm.
        if (t.name === "private_profile" && !t.checked && u.is_minor) {
          if (!await confirmEqual("Your profile will be visible to everyone. You can switch back any time.")) {
            t.checked = true; return;
          }
        }
        const body = t.dataset.pv ? { privacy: { [t.dataset.pv]: t.checked } } : t.name === "private_profile" ? { public_profile: !t.checked } : { [t.name]: t.checked };
        const r = await api("/me/privacy", { method: "POST", body }); Object.assign(u, r); keep(); render(); ok($("#pane"), "Saved."); }
      else if (f.dataset.f === "notify") { u.notify_prefs = await api("/me/notifications", { method: "POST", body: { [e.target.name]: e.target.checked } }); ok(f, "Saved."); }
    } catch (x) { e.target.checked = !e.target.checked; bad(f, x); }
  });
  root.addEventListener("click", async e => {
    const b = e.target.closest("button,a[data-resend]"); if (!b) return; const pane = $("#pane");
    try {
      if (b.matches("[data-upload]")) $("#av-file").click();
      else if (b.matches("[data-avreset]")) { await api("/me/avatar/reset", { method: "POST" }); await refresh(); render(); ok($("#pane"), "Back to the default picture."); }
      else if (b.matches("[data-preset]")) { await api("/me/avatar/preset", { method: "POST", body: { preset: Number(b.dataset.preset) } }); await refresh(); render(); ok($("#pane"), "Picture updated."); }
      else if (b.matches("[data-resend]")) { e.preventDefault(); if (!u.mail_live) return; await api("/me/verify/resend", { method: "POST" }); ok(pane, "Sent. Check your inbox. Can't find it? Check your junk or spam folder."); }
      else if (b.matches("[data-save-dob]")) {
        const day = $('[name=dob_day]', pane), month = $('[name=dob_month]', pane), year = $('[name=dob_year]', pane);
        if (!day.value || !month.value || !year.value) { alert("Please enter your date of birth."); return; }
        const birth_date = `${year.value}-${month.value}-${day.value}`;
        try {
          const r = await api("/me/profile", { method: "POST", body: { birth_date } });
          Object.assign(u, r); if (r.details) u.details = r.details; keep(); render();
          ok($("#pane"), "Date of birth saved.");
        } catch (x) { alert(x.message); if (x.code === "account_paused") location.href = "/login"; }
      } else if (b.matches("[data-export]")) {
        const r = await fetch(API + "/export.zip?tz=" + encodeURIComponent(TZ), { headers: { Authorization: "Bearer " + token() } });
        if (!r.ok) { const j = await r.json().catch(() => null); throw new Error((j && j.message) || "Export failed (HTTP " + r.status + ")."); }
        const a = document.createElement("a"); a.href = URL.createObjectURL(await r.blob()); a.download = "fraglink-export.zip"; a.click();
      }
      else if (b.matches("[data-delete]")) { $('form[data-f="delete"]').hidden = false; b.disabled = true; }
      else if (b.matches("[data-copy]")) { navigator.clipboard && navigator.clipboard.writeText(b.dataset.copy); b.textContent = "Copied"; }
      else if (b.matches("[data-mfa-setup]")) {
        const box = $("[data-mfa]"), pw = $('[data-mfa] [name="password"]').value;
        const r = await api("/me/mfa/setup", { method: "POST", body: { password: pw } });
        $("[data-mfa-step]").innerHTML = `<div class="frm">${row("1. Scan", `<div class="qr">${r.qr_svg}</div>`, "Scan with your authenticator app.")}${row("Or type this key", `<code style="font-size:14px;letter-spacing:1px">${esc(r.secret)}</code>`, "Time-based, 6 digits.")}
          ${row("2. Enter the code", `<input class="t" name="enable_code" autocomplete="one-time-code" maxlength="6" placeholder="123456"> <button class="btn dl" type="button" data-mfa-enable>Turn on</button>`)}</div>`;
        box.querySelector("[data-msg]").hidden = true;
      }
      else if (b.matches("[data-mfa-enable]")) {
        const r = await api("/me/mfa/enable", { method: "POST", body: { code: $('[name="enable_code"]').value.trim() } });
        await refresh();
        $("[data-mfa]").innerHTML = `${msg}<div class="frm">${row("Status", `<b style="color:var(--online)">On</b>`)}${row("Backup codes", `<div class="mpcodes">${r.backup_codes.map(c => `<span>${esc(c)}</span>`).join("")}</div>`, "Save these somewhere safe. Each one signs you in once if you lose your phone. They won't be shown again.")}${row("", `<button class="btn" type="button" data-reload>Done</button>`)}</div>`;
      }
      else if (b.matches("[data-mfa-off]") || b.matches("[data-mfa-codes]")) {
        const body = { password: $('[data-mfa] [name="password"]').value, code: $('[data-mfa] [name="code"]').value.trim() };
        if (b.matches("[data-mfa-off]")) { await api("/me/mfa/disable", { method: "POST", body }); await refresh(); render(); ok($("#pane"), "Two-step sign-in is off."); }
        else { const r = await api("/me/mfa/backup_codes", { method: "POST", body }); $("[data-mfa-step]").innerHTML = `<div class="frm">${row("New backup codes", `<div class="mpcodes">${r.backup_codes.map(c => `<span>${esc(c)}</span>`).join("")}</div>`, "The old ones no longer work.")}</div>`; }
      }
      else if (b.matches("[data-revoke]")) { const id = b.closest("[data-id]").dataset.id; await api("/me/sessions/" + encodeURIComponent(id), { method: "DELETE" }); b.closest("[data-id]").remove(); }
      else if (b.matches("[data-revoke-all]")) { if (!confirm("Sign out every other browser and desktop app?")) return; const r = await api("/me/sessions/revoke_all", { method: "POST" }); loadSessions(pane); setTimeout(() => ok($("[data-sess]"), `${plural(r.revoked, "session")} signed out.`), 300); }
    } catch (x) { const scope = b.closest("[data-mfa],[data-sess],form") || pane; bad(scope, x); }
  });
  render();
};

/* ---------- unconfirmed email banner (doesn't block anything) ---------- */
function verifyBar() {
  const u = me();
  if (u && u.email_verified) { const old = $(".verify-bar"); if (old) old.remove(); }
  if (!u || !u.email || u.email_verified || $(".verify-bar") || sessionStorage.getItem("fl_vb_closed")) return;
  const d = document.createElement("div"); d.className = "verify-bar";
  d.innerHTML = `<div class="wrap"><span>Please confirm your email address (${esc(u.email)}).${u.mail_live ? "" : " Confirmation emails are switched on soon; you can keep using Fraglink meanwhile."}</span>${u.mail_live ? `<button type="button" data-vb-send>Send the link again</button>` : ""}<button type="button" data-vb-x style="margin-left:auto">Hide</button></div>`;
  d.addEventListener("click", async e => {
    if (e.target.matches("[data-vb-x]")) { sessionStorage.setItem("fl_vb_closed", "1"); d.remove(); }
    if (e.target.matches("[data-vb-send]")) { try { await api("/me/verify/resend", { method: "POST" }); e.target.textContent = "Sent. Can't find it? Check junk or spam."; } catch (x) { e.target.textContent = x.message; } }
  });
  ($(".preview-flag") || $(".top")).after(d);
}

/* ---------- site banner (staff announcements) ---------- */
async function banner() {
  try {
    const r = await fetch(API + "/announcement", { cache: "no-store" });
    if (!r.ok) return;
    const a = (await r.json()).announcement;
    if (!a || sessionStorage.getItem("fl_ann_closed") === String(a.id)) return;
    const d = document.createElement("div");
    d.className = "announce " + (a.style === "warning" ? "warning" : "info");
    const link = a.link ? ` <a href="${esc(a.link)}"${a.link.startsWith("http") ? ' rel="noopener"' : ""}>More</a>` : "";
    d.innerHTML = `<div class="wrap"><span>${esc(a.message)}${link}</span><button type="button" aria-label="Dismiss">×</button></div>`;
    $("button", d).addEventListener("click", () => { sessionStorage.setItem("fl_ann_closed", String(a.id)); d.remove(); });
    const flag = $(".preview-flag") || $(".top");
    flag.after(d);
    // gone right when it ends (same as the app), not only on the next page load
    if (a.ends_at) setTimeout(() => d.remove(), Math.max(0, Math.min(Date.parse(a.ends_at) - Date.now(), 2147483000)));
  } catch (e) { /* no banner */ }
}

/* ---------- admin (staff only; the page itself is a 404 for everyone else) ---------- */
/* ----- news: /news (paged list) and /news/<slug> (one post) ----- */
/* Body text: blank line = new paragraph, single newline = line break, https:// links are clickable. */
function newsBody(t) {
  const link = s => s.replace(/(https:\/\/[^\s<]+[^\s<.,;:!?)\]'"])/g, u => `<a href="${u}" rel="noopener">${u}</a>`);
  return String(t || "").split(/\n{2,}/).map(p => p.trim()).filter(Boolean).map(p => `<p>${link(esc(p)).replace(/\n/g, "<br>")}</p>`).join("");
}
pages.news = async () => {
  const root = $("#news"), slug = decodeURIComponent(location.pathname.replace(/^\/news\/?/, "").replace(/\/$/, ""));
  if (slug) {
    try {
      const d = await api("/news/" + encodeURIComponent(slug)), p = d.post;
      document.title = p.title + " · Fraglink";
      window.flCrumb([[p.title]]);
      if (p.slug !== slug) history.replaceState(null, "", p.url);
      root.innerHTML = `<p class="crumb"><a href="/news">‹ All news</a></p><article class="panel post">${p.image ? `<img class="post-img" src="${esc(p.image)}" alt="">` : ""}<div class="body">
        <h1>${esc(p.title)}</h1><div class="sub">${fullDate.format(new Date(p.at))}${p.pinned ? ' · <span class="pin">Pinned</span>' : ""}</div>
        <div class="txt">${newsBody(p.body) || `<p class="muted">${esc(p.summary)}</p>`}</div>
        ${p.link ? `<p><a class="btn blue" href="${esc(p.link)}"${p.link.startsWith("http") ? ' rel="noopener"' : ""}>${esc(p.link_label || "Read more")}</a></p>` : ""}</div></article>
        <div class="post-nav">${d.newer ? `<a href="${esc(d.newer.url)}">‹ Newer: ${esc(d.newer.title)}</a>` : "<span></span>"}${d.older ? `<a href="${esc(d.older.url)}">Older: ${esc(d.older.title)} ›</a>` : ""}</div>`;
    } catch (x) {
      root.innerHTML = `<p class="crumb"><a href="/news">‹ All news</a></p><div class="panel"><div class="body empty">${x.status === 404 ? "That post doesn't exist or was removed." : esc(x.message)}</div></div>`;
    }
    return;
  }
  const PER = 10;
  const load = async pg => {
    const d = await api(`/news?page=${pg}&per=${PER}`);
    root.innerHTML = `<section class="head"><div><h1>News</h1><div class="sub">Updates from the Fraglink team.</div></div></section>
      <div class="panel"><div class="body nlist">${d.items.length ? d.items.map(n => `<a class="nrow${n.image ? " has-img" : ""}" href="${esc(n.url)}">${n.image ? `<img src="${esc(n.image)}" alt="" loading="lazy">` : ""}<div><small>${fullDate.format(new Date(n.at))}${n.pinned ? ' · <span class="pin">Pinned</span>' : ""}</small><b>${esc(n.title)}</b>${n.summary ? `<p>${esc(n.summary)}</p>` : ""}</div></a>`).join("") : `<p class="empty">No news yet.</p>`}</div></div>
      ${d.total > PER ? pagerHtml((d.page - 1) * PER, PER, d.total) : ""}`;
    $$(".pager [data-off]", root).forEach(b => b.addEventListener("click", () => { const p = Number(b.dataset.off) / PER + 1; history.replaceState(null, "", p > 1 ? "?page=" + p : "/news"); load(p); window.scrollTo(0, 0); }));
  };
  try { await load(Math.max(1, parseInt(new URLSearchParams(location.search).get("page"), 10) || 1)); }
  catch (x) { root.innerHTML = `<div class="panel"><div class="body empty">${esc(x.message)}</div></div>`; }
};

pages.admin = async () => {
  if (!token()) return;
  let who;
  try { who = await api("/admin/me", { keepSession: true }); } catch (e) { return; }
  document.title = "Admin · Fraglink";
  window.FL = { api, esc, $, $$, hours, dur, since, fullDate, token, me, showErr, showOk, icon, loadIcons, initial, API };
  const s = document.createElement("script");
  s.src = "/assets/admin.js?v=" + (document.querySelector('script[src*="app.js"]').src.split("v=")[1] || "");
  s.onload = () => window.FLAdmin(who);
  document.head.appendChild(s);
};

/* ---------- feature switches (Admin > Site settings; the server enforces them too) ---------- */
const FEATURES = { signups: true, announcement: true, connect_steam: true, connect_bnet: true, connect_discord: true, friend_requests: true, public_profiles: true, wow_characters: false, maintenance: false };
const LINKS = {};
const featuresReady = fetch(API + "/features", { cache: "no-store" }).then(r => r.ok ? r.json() : null).then(d => { if (d && d.features) Object.assign(FEATURES, d.features); if (d && d.links) Object.assign(LINKS, d.links); }).catch(() => {});
function applyFeatures() {
  document.body.classList.toggle("ff-no-friendreq", !FEATURES.friend_requests);
  document.body.classList.toggle("ff-no-search", !FEATURES.public_profiles);
  const page = document.body.dataset.page, staff = !!(me() && me().staff_role);
  if (FEATURES.maintenance && !staff && page !== "login" && page !== "admin") {
    $("main").innerHTML = `<div class="maint panel"><div class="body"><h1>Down for maintenance</h1><p>We're doing some work on Fraglink. Back soon.</p><p class="muted">News and updates: <a href="https://discord.gg/QbaYRtvADj" rel="noopener">our Discord</a>.</p></div></div>`;
    return false;
  }
  if (!FEATURES.signups && page === "signup") {
    const f = $("#signup"); if (f) f.outerHTML = `<p>Sign-ups are closed for now. Check back soon.</p><p><a href="/login">Already have an account? Sign in</a></p>`;
    return false;
  }
  return true;
}

/* ---------- shared "You are here" breadcrumb (every page except home, which has its own bar) ---------- */
const CRUMBS = { download: [["Download", "/download"]], forgot: [["Sign in", "/login"], ["Forgot password"]], friends: [["Friends", "/friends"]],
  login: [["Sign in"]], news: [["News", "/news"]], privacy: [["Privacy Policy"]], reset: [["Sign in", "/login"], ["New password"]],
  settings: [["Account Settings", "/settings"]], signup: [["Create an account"]], terms: [["Terms of Use"]], profile: [["Profile"]],
  verify: [["Confirm your email"]], wowupload: [["Account settings", "/settings"], ["World of Warcraft", "/settings#wow"], ["Upload /played"]], bnetverify: [["Account settings", "/settings"], ["World of Warcraft", "/settings#wow"], ["Verify characters"]], bnet: [["Account settings", "/settings"], ["World of Warcraft", "/settings#wow"]], faq: [["FAQ", "/faq"]], changelog: [["Changelog", "/changelog"]], contact: [["Contact", "/contact"]], status: [["Status", "/status"]], media: [["Media", "/media"]], admin: [["Admin", "/admin"]], notfound: [["Page not found"]] };
function setCrumb(parts) {
  const nav = document.getElementById("ycrumb"); if (!nav || !parts) return;
  const all = [["Home", "/"], ...parts];
  nav.firstElementChild.innerHTML = `<span class="yl">You are here:</span> <ol>${all.map((p, i) => { const last = i === all.length - 1;
    return `<li${last ? ' aria-current="page"' : ""}>${p[1] && !last ? `<a href="${esc(p[1])}" title="${esc(p[0])}">${esc(p[0])}</a>` : `<span title="${esc(p[0])}">${esc(p[0])}</span>`}</li>`; }).join("")}</ol>`;
  nav.hidden = false;
}
window.flCrumb = extra => setCrumb([...(CRUMBS[document.body.dataset.page] || []), ...extra]);

/* ---------- boot ---------- */
document.addEventListener("DOMContentLoaded", async () => {
  await featuresReady;
  renderHeader();
  if (!applyFeatures()) return;
  const y = $("#year"); if (y) y.textContent = new Date().getFullYear();
  banner();
  setCrumb(CRUMBS[document.body.dataset.page]);
  const f = pages[document.body.dataset.page];
  // Submit buttons in the page HTML start disabled (data-wait) until the page's handlers exist.
  const ready = () => $$("button[data-wait]").forEach(b => { b.disabled = false; b.removeAttribute("data-wait"); });
  if (f) Promise.resolve(f()).catch(e => { console.error(e); }).finally(ready); else ready();
  // keep the header in sync with the real account (handle/display name changes)
  if (token() && document.body.dataset.page !== "settings" && document.body.dataset.page !== "verify") api("/me").then(m => { setSession(token(), { id: m.user.id, handle: m.user.handle, display_name: m.user.display_name, email: m.user.email, staff_role: m.user.staff_role || null, avatar: m.user.avatar, email_verified: m.user.email_verified, mail_live: m.user.mail_live, timezone: m.user.timezone || null }); renderHeader(); verifyBar(); /* saved zone changed (another device, or first visit after sign-in): redraw with it */ if ((m.user.timezone || null) !== SAVED0 && okZone(m.user.timezone || "UTC")) location.reload(); }).catch(() => { if (!token()) renderHeader(); });
  else verifyBar();
  if (token()) {
    startFriendsLive();
    onFriendsLive(() => {
      if (document.body.dataset.page !== "home") return;
      const kpi = document.querySelector(".kpi");
      if (kpi) {
        const cells = kpi.querySelectorAll("div");
        // third kpi cell is friends online/total when signed in
        if (cells.length >= 3 && LIVE_FRIENDS.friends != null) {
          const b = cells[2].querySelector("b");
          if (b) b.textContent = LIVE_FRIENDS.online + "/" + LIVE_FRIENDS.friends;
        }
      }
      // refresh the Friends online panel heading count
      $$(".panel").forEach(p => {
        const h2 = p.querySelector("h2");
        if (h2 && /^Friends online/.test(h2.textContent || "")) {
          h2.childNodes[0].textContent = `Friends online (${LIVE_FRIENDS.online})`;
        }
      });
    });
  }
  if (qs("deleted")) { const d = new Date(qs("deleted")); const el = document.createElement("div"); el.className = "verify-bar delete-bar"; el.innerHTML = `<div class="wrap">Your account is scheduled for deletion${isNaN(d) ? "" : " on " + fullDate.format(d)}. Sign in again before then to keep it.</div>`; ($(".preview-flag") || $(".top")).after(el); }
});


// WoW character popup (public profile)
const WOWPOP = (() => {
  const CC = { 1: "#C69B6D", 2: "#F48CBA", 3: "#AAD372", 4: "#FFF468", 5: "#FFFFFF", 6: "#C41E3A", 7: "#0070DD", 8: "#3FC7EB", 9: "#8788EE", 10: "#00FF98", 11: "#FF7C0A", 12: "#A330C9", 13: "#33937F" };
  const L = ["HEAD", "NECK", "SHOULDER", "BACK", "CHEST", "SHIRT", "TABARD", "WRIST"], R = ["HANDS", "WAIST", "LEGS", "FEET", "FINGER_1", "FINGER_2", "TRINKET_1", "TRINKET_2"], W = ["MAIN_HAND", "OFF_HAND"];
  const SN = { HEAD: "Head", NECK: "Neck", SHOULDER: "Shoulder", BACK: "Back", CHEST: "Chest", SHIRT: "Shirt", TABARD: "Tabard", WRIST: "Wrist", HANDS: "Hands", WAIST: "Waist", LEGS: "Legs", FEET: "Feet", FINGER_1: "Ring", FINGER_2: "Ring", TRINKET_1: "Trinket", TRINKET_2: "Trinket", MAIN_HAND: "Main hand", OFF_HAND: "Off hand" };
  const DO = ["LFR", "NORMAL", "HEROIC", "MYTHIC"];
  const ago = iso => { const m = Math.round((Date.now() - new Date(iso)) / 60000); return m < 1 ? "just now" : m < 60 ? plural(m, "minute") + " ago" : m < 1440 ? plural(Math.round(m / 60), "hour") + " ago" : plural(Math.round(m / 1440), "day") + " ago"; };
  let box = null, pushed = false;
  const close = fromPop => {
    if (!box) return; box.remove(); box = null; document.body.classList.remove("cp-lock"); document.removeEventListener("keydown", key);
    if (pushed && !fromPop) { pushed = false; history.back(); } pushed = false;
  };
  const key = e => { if (e.key === "Escape") close(); };
  addEventListener("popstate", () => { if (box) close(true); });
  const whp = it => [it.ilvl && "ilvl=" + it.ilvl, it.bonus && it.bonus.length && "bonus=" + it.bonus.join(":"), it.ench && "ench=" + it.ench, it.gems && it.gems.length && "gems=" + it.gems.join(":"), it.pcs && it.pcs.length && "pcs=" + it.pcs.join(":")].filter(Boolean).join("&");
  const slot = (g, k) => { const it = g[k]; const lk = it && it.id && WH.ok, tag = lk ? "a" : "div"; return it ? `<${tag} class="cp-slot q-${esc((it.quality || "common").toLowerCase())}"${lk ? ` href="https://www.wowhead.com/item=${Number(it.id)}" data-wowhead="${esc(whp(it))}" target="_blank" rel="noopener"` : it.id ? ` data-whi="${Number(it.id)}" data-whp="${esc(whp(it))}" role="button" tabindex="0" aria-expanded="false"` : ` title="${esc(it.name)}"`}>${it.icon ? `<img src="/api/v1/wow/icons/${encodeURIComponent(it.icon)}.jpg" alt="" loading="lazy" ${imgOk}>` : `<span class="cp-noi"></span>`}<div><b>${esc(it.name)}</b><small>${esc(SN[k])}${it.ilvl ? " · " + it.ilvl : ""}</small></div></${tag}>` : `<div class="cp-slot cp-empty"><span class="cp-noi"></span><div><b>Empty</b><small>${esc(SN[k])}</small></div></div>`; };
  const raid = r => r.instances.map(i => `<div class="cp-raid"><b>${esc(i.name)}</b><div class="cp-modes">${[...i.modes].sort((a, b) => DO.indexOf(a.difficulty) - DO.indexOf(b.difficulty)).map(m => `<span class="${m.done >= m.total ? "full" : ""}">${esc(m.label)} <b>${m.done}/${m.total}</b></span>`).join("")}</div></div>`).join("");
  const body = (d, c) => {
    if (d.status === "missing") return `<div class="cp-hd"><div><h2>${esc(c.name)}${factionCrest(c.faction)}</h2><div class="sub">${esc(c.realm)} ${esc(c.region.toUpperCase())}</div></div></div><p class="empty cp-pad">Not on the Armory.</p>`;
    const g = {}; (d.gear || []).forEach(x => g[x.slot] = x);
    const col = CC[d.class_id] || "#eef2f6", mp = d.mplus || {}, pv = d.pvp || {}, rd = d.raids || {}, co = d.collections || {}, pr = d.professions || [];
    const stat = (l, v) => `<div class="cp-st"><small>${l}</small><b>${v}</b></div>`;
    const rr = rd.current && rd.current.instances.length ? { t: rd.current.expansion, r: rd.current } : rd.recent && rd.recent.instances.length ? { t: "Most recent progress · " + rd.recent.expansion, r: rd.recent } : null;
    return `<div class="cp-hd${d.render ? " has-rd" : ""}" style="--cc:${col}">${d.render ? `<div class="cp-rw" aria-hidden="true"><img class="cp-rd" alt="" hidden></div>` : ""}<div class="cp-who">
      ${d.title ? `<div class="cp-ti">${esc(d.title.replace("{name}", d.name))}</div>` : ""}<h2 style="color:${col}">${esc(d.name)}</h2>
      <div class="sub">Level ${d.level} ${esc([d.race, d.spec, d.class].filter(Boolean).join(" "))}</div>
      <div class="sub">${esc(d.realm)} ${esc(d.region.toUpperCase())}${factionCrest(d.faction || c.faction)}</div>
      ${d.guild ? `<div class="cp-gu">&lt;${esc(d.guild.name)}&gt;${d.guild.rank != null ? ` · ${d.guild.rank === 0 ? "Guild Master" : "Rank " + d.guild.rank}` : ""}</div>` : ""}
      <div class="cp-sts">${stat("Item level", d.ilvl ? nf.format(d.ilvl) : "—")}${stat("Achievements", d.achievement_points ? nf.format(d.achievement_points) : "—")}${stat("M+ rating", mp.rating ? nf.format(Math.round(mp.rating)) : "—")}</div></div></div>
      <div class="cp-bd"><section><h3>Gear</h3>${d.gear && d.gear.length ? `<div class="cp-gear"><div>${L.map(k => slot(g, k)).join("")}</div><div>${R.map(k => slot(g, k)).join("")}</div></div><div class="cp-gear cp-w">${W.map(k => slot(g, k)).join("")}</div>` : `<p class="empty">No gear equipped.</p>`}</section>
      <section><h3>Raid progress</h3>${rr ? `<div class="sub cp-rt">${esc(rr.t)}</div>${raid(rr.r)}` : `<p class="empty">No raid progress yet.</p>`}</section>
      <div class="cp-two"><section><h3>Mythic+</h3>${mp.runs && mp.runs.length ? `<table class="cp-tb">${mp.runs.map(r => `<tr><td>${esc(r.dungeon)}</td><td class="n">+${r.level}</td><td class="n ${r.timed ? "ok" : "dim"}">${r.timed ? "Timed" : "Overtime"}</td></tr>`).join("")}</table>` : `<p class="empty">No Mythic+ runs this season.</p>`}</section>
      <section><h3>PvP</h3>${pv.honor_level || pv.honorable_kills || (pv.rated && pv.rated.length) ? `<table class="cp-tb"><tr><td>Honor level</td><td class="n">${nf.format(pv.honor_level || 0)}</td></tr><tr><td>Honorable kills</td><td class="n">${nf.format(pv.honorable_kills || 0)}</td></tr>${(pv.rated || []).map(b => `<tr><td>${esc(b.bracket)}</td><td class="n">${nf.format(b.rating)}</td></tr>`).join("")}</table>` : `<p class="empty">No PvP activity.</p>`}</section></div>
      <div class="cp-two"><section><h3>Collections</h3><table class="cp-tb"><tr><td>Mounts</td><td class="n">${co.mounts != null ? nf.format(co.mounts) : "—"}</td></tr><tr><td>Pets</td><td class="n">${co.pets != null ? nf.format(co.pets) : "—"}</td></tr></table></section>
      <section><h3>Professions</h3>${pr.length ? `<table class="cp-tb">${pr.map(x => `<tr><td>${esc(x.name)}${x.tier ? `<small class="sub"> ${esc(x.tier)}</small>` : ""}</td><td class="n">${x.max ? `${x.skill}/${x.max}` : x.skill != null ? x.skill : ""}</td></tr>`).join("")}</table>` : `<p class="empty">No professions.</p>`}</section></div></div>
      <div class="cp-ft"><span class="sub">${d.updated_at ? "Updated " + ago(d.updated_at) : ""}</span><a href="${esc(d.armory)}" target="_blank" rel="noopener">View on Armory ↗</a></div>`;
  };
  // Touch devices: tap a gear row to show Wowhead's tooltip for it inline (sanitised), tap again/elsewhere to hide.
  const TT = new Map(), OKT = new Set(["TABLE", "TBODY", "TR", "TD", "TH", "B", "BR", "SPAN", "DIV", "SMALL", "I", "UL", "LI"]);
  const clean = html => { const doc = new DOMParser().parseFromString(`<div>${html}</div>`, "text/html"), out = document.createElement("div");
    const walk = (src, dst) => src.childNodes.forEach(n => { if (n.nodeType === 3) dst.appendChild(document.createTextNode(n.nodeValue));
      else if (n.nodeType === 1) { const t = OKT.has(n.tagName) ? n.tagName : n.tagName === "A" ? "SPAN" : null; if (!t) return; const e = document.createElement(t);
        const c = n.getAttribute("class"); if (c && /^[\w -]+$/.test(c)) e.className = c; if (/^(TD|TH|TABLE)$/.test(t) && n.getAttribute("width") === "100%") e.setAttribute("width", "100%");
        walk(n, e); dst.appendChild(e); } });
    walk(doc.body.firstChild, out); return out; };
  const tipOff = () => { $$(".cp-tip", box).forEach(t => t.remove()); $$(".cp-slot[aria-expanded=true]", box).forEach(r => r.setAttribute("aria-expanded", "false")); };
  const tip = async row => {
    const was = row.getAttribute("aria-expanded") === "true"; tipOff(); if (was) return;
    row.setAttribute("aria-expanded", "true");
    const t = document.createElement("div"); t.className = "cp-tip"; t.textContent = "Loading…"; row.after(t);
    const k = row.dataset.whi + "?" + row.dataset.whp;
    try { if (!TT.has(k)) TT.set(k, fetch(`https://nether.wowhead.com/tooltip/item/${row.dataset.whi}?dataEnv=1&locale=0${row.dataset.whp ? "&" + row.dataset.whp : ""}`).then(r => { if (!r.ok) throw 0; return r.json(); }));
      const j = await TT.get(k); if (!t.isConnected) return; t.textContent = ""; t.appendChild(clean(j.tooltip || "")); }
    catch (e) { TT.delete(k); if (t.isConnected) t.textContent = "Couldn't load this item's tooltip."; }
  };
  const open = async (handle, c) => {
    if (!c) return; close(true);
    box = document.createElement("div"); box.className = "cp-bg";
    box.innerHTML = `<div class="cp" role="dialog" aria-modal="true" aria-label="${esc(c.name)}"><button class="cp-x" type="button" aria-label="Close">×</button><div class="cp-in"><p class="empty cp-pad">Loading ${esc(c.name)}…</p></div></div>`;
    document.body.appendChild(box); document.body.classList.add("cp-lock");
    box.onclick = e => { if (e.target === box || e.target.closest(".cp-x")) return close(); const r = e.target.closest(".cp-slot[data-whi]"); if (r) return tip(r); if (!e.target.closest(".cp-tip")) tipOff(); };
    box.onkeydown = e => { const r = (e.key === "Enter" || e.key === " ") && e.target.closest && e.target.closest(".cp-slot[data-whi]"); if (r) { e.preventDefault(); tip(r); } };
    document.addEventListener("keydown", key); history.pushState({ wowpop: 1 }, ""); pushed = true;
    $(".cp-x", box).focus();
    const me = box;
    try { const d = await api(`/users/${encodeURIComponent(handle)}/wow/${encodeURIComponent(c.id)}`); if (box !== me) return; $(".cp-in", box).innerHTML = body(d, { ...c, handle }); WH.scan();
      const im = $(".cp-rd", box);
      if (im) { const t = localStorage.getItem(TK), r = await fetch(`${API}/users/${encodeURIComponent(handle)}/wow/${encodeURIComponent(c.id)}/render.png`, { headers: t ? { authorization: "Bearer " + t } : {} });
        if (r.ok && box === me) { im.src = URL.createObjectURL(await r.blob()); im.onload = () => URL.revokeObjectURL(im.src); im.hidden = false; } } }
    catch (x) { if (box === me) $(".cp-in", box).innerHTML = `<p class="empty cp-pad">Couldn't load this character.</p>`; }
  };
  return { open, close };
})();

})();

// Home screenshot carousel (V4.2): arrows, thumbnails, arrow keys, swipe. No auto-advance.
(function () {
  const r = document.getElementById("car42"); if (!r) return;
  const s = r.querySelectorAll(".sl"), t = r.querySelectorAll(".th42 button"), caps = Array.from(t, b => b.title);
  let i = 0, x0 = null;
  const go = n => { i = (n + s.length) % s.length; s.forEach((e, k) => e.classList.toggle("on", k === i)); t.forEach((e, k) => { e.classList.toggle("on", k === i); e.setAttribute("aria-current", k === i ? "true" : "false"); });
    r.querySelector(".cap").textContent = caps[i]; r.querySelector(".cnt").textContent = (i + 1) + " / " + s.length; };
  r.querySelector(".car-b.p").onclick = () => go(i - 1);
  r.querySelector(".car-b.n").onclick = () => go(i + 1);
  t.forEach(b => { b.onclick = () => go(+b.dataset.go); });
  r.addEventListener("keydown", e => { if (e.key === "ArrowLeft") go(i - 1); else if (e.key === "ArrowRight") go(i + 1); });
  const st = r.querySelector(".stage");
  st.addEventListener("touchstart", e => { x0 = e.touches[0].clientX; }, { passive: true });
  st.addEventListener("touchend", e => { if (x0 !== null) { const dx = e.changedTouches[0].clientX - x0; if (Math.abs(dx) > 40) go(i + (dx < 0 ? 1 : -1)); } x0 = null; });
  go(0);
})();
