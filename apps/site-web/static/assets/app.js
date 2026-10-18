/* Fraglink website: plain JS, no build step. Talks to the server's JSON API with a
   bearer token (kept in localStorage after sign-in). Pages pick their code via <body data-page>. */
(function () {
"use strict";
const API = "/api/v1", TK = "fl_token", UK = "fl_user";
const TZ = (() => { try { return Intl.DateTimeFormat().resolvedOptions().timeZone || ""; } catch (e) { return ""; } })();
const DISCORD = "https://discord.gg/QbaYRtvADj";
const $ = (s, r) => (r || document).querySelector(s);
const $$ = (s, r) => Array.from((r || document).querySelectorAll(s));
const esc = s => String(s == null ? "" : s).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const token = () => localStorage.getItem(TK);
const me = () => { try { return JSON.parse(localStorage.getItem(UK)); } catch (e) { return null; } };
function setSession(t, u) { localStorage.setItem(TK, t); localStorage.setItem(UK, JSON.stringify(u)); }
function clearSession() { localStorage.removeItem(TK); localStorage.removeItem(UK); }

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
const dayFmt = new Intl.DateTimeFormat("en-AU", { weekday: "long" });
const dateFmt = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short" });
const fullDate = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", year: "numeric" });
/* Friends' activity rows from the server: "playing" first, then one per friend/game/day. */
function actDay(i) { return i.kind === "playing" ? "Now" : dayLabel(i.day ? new Date(i.day + "T12:00:00") : i.at); }
function actText(i) {
  const g = isUnlisted(i.game_id) ? "a game" : `<b>${esc(i.game_name)}</b>`;
  return i.kind === "playing" ? `is playing ${g} now` : `played ${g} for ${dur(i.duration_s)}`;
}
function dayLabel(d) {
  const t = new Date(); t.setHours(0, 0, 0, 0);
  const x = new Date(d); x.setHours(0, 0, 0, 0);
  const diff = Math.round((t - x) / 86400000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  if (diff < 7) return dayFmt.format(x);
  return dateFmt.format(x);
}
function lastPlayed(iso) {
  if (!iso) return "-";
  const d = new Date(iso), diff = (Date.now() - d) / 86400000;
  if (diff < 1 && new Date().getDate() === d.getDate()) return "Today";
  if (diff < 7) return dayFmt.format(d).slice(0, 3);
  return dateFmt.format(d);
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
  if (src) return `<img class="${cls || ""}" src="${esc(src)}" alt="" loading="lazy" width="20" height="20" onerror="this.style.visibility='hidden'">`;
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
  return `<div class="artbox ${cls || ""}" style="background:${artGrad(name)}"><span class="ai">${iconCache[id] ? icon(id, name) : `<img src="/assets/mark.svg" alt="" style="opacity:.35">`}</span>${u ? `<img class="ov" src="${u}" alt="" loading="lazy" onerror="this.remove()">` : ""}</div>`;
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
    items.push(["download", "/download", "Download"]);
    const mine = page === "profile" && u && location.pathname.toLowerCase() === "/u/" + u.handle.toLowerCase();
    nav.innerHTML = items.map(([k, href, label]) =>
      `<a href="${href}" class="${(k === page && (k !== "profile" || mine)) ? "on" : ""}">${label}</a>`).join("");
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
  try {
    const l = await latest();
    btns.forEach(b => { if (l.url) b.href = l.url; b.removeAttribute("aria-disabled"); });
    vers.forEach(v => { v.textContent = "Version " + l.version + (l.date ? " · " + fullDate.format(new Date(l.date)) : "") + " · Windows 10/11, 64-bit"; });
  } catch (e) {
    vers.forEach(v => { v.textContent = "The download is unavailable right now. Try again in a few minutes."; });
  }
}

/* ---------- pages ---------- */
const pages = {};

pages.home = async () => {
  fillDownload();
  const u = me();
  if (u) $$("[data-signedout]").forEach(e => e.hidden = true);
  let h;
  try { h = await api("/home"); } catch (e) { return; }
  const live = $("#home-live");
  const ids = [].concat(h.most_played.map(g => g.game_id), h.now_playing.map(n => n.game_id));
  let fl = null, act = null;
  if (u && token()) {
    [fl, act] = await Promise.all([api("/friends/page?show=online&limit=10").catch(() => null), api("/friends/activity?tz=" + encodeURIComponent(TZ)).catch(() => null)]);
    if (fl) fl.friends.forEach(f => f.presence && ids.push(f.presence.game_id));
    if (act) act.items.forEach(i => ids.push(i.game_id));
  }
  await loadIcons(ids);
  const top = h.most_played[0];
  // hero: art of this week's most played game (signed out); welcome strip (signed in)
  if (u && h.me) {
    $("#hero").outerHTML = `<section class="welcome-strip">${avatar((me() || {}).avatar, 64)}<div><h1>Welcome back, ${esc(u.display_name || u.handle)}</h1>
      <div class="sub">${h.me.top_game ? `Most played this week: <b>${esc(h.me.top_game)}</b>` : `Nothing played this week yet. <a href="/download">Get the app</a> and it counts your hours.`}</div>
      <div style="margin-top:6px"><a href="/u/${encodeURIComponent(u.handle)}">View my profile</a> · <a href="/friends">Friends</a> · <a href="/settings">Settings</a></div></div>
      <div class="kpi"><div><b>${hrsShort(h.me.minutes_7d)}</b><span>Hours this week</span></div><div><b>${hrsShort(h.me.total_min)}</b><span>Total hours</span></div><div><b>${h.me.friends_online}/${h.me.friends}</b><span>Friends online</span></div></div></section>`;
  } else if (top) {
    $("#hero").style.background = artBg(top.game_id, top.name);
    $("#hero .inr").insertAdjacentHTML("beforeend", `<p style="margin-top:14px;font-size:12px;color:#c8d4e0">Most played on Fraglink this week: <b style="color:#fff">${esc(top.name)}</b> · ${plural(top.players, "player")}</p>`);
  }
  const ticker = h.now_playing.length ? (() => {
    const items = h.now_playing.map(n => `<span class="it">${avatar(n.avatar, 18)}<a href="/u/${encodeURIComponent(n.handle)}">${esc(n.display_name || n.handle)}</a> is playing ${icon(n.game_id, n.game_name)}<b>${esc(n.game_name)}</b></span>`).join("");
    return `<div class="ticker"><span class="l">LIVE</span><div style="overflow:hidden;flex:1"><div class="run">${items}${items}</div></div></div>`;
  })() : "";
  const s = h.stats;
  const stats = `<div class="stats"><div><b>${nf.format(s.registered)}</b><span>Players</span></div><div><b>${nf.format(s.online)}</b><span>Online now</span></div>
    <div><b>${nf.format(s.in_game)}</b><span>In a game now</span></div><div><b>${hrsShort(s.minutes_7d)}</b><span>Hours played this week</span></div></div>`;
  const panel = (title, body, more) => `<section class="panel"><h2>${title}${more || ""}</h2><div class="body">${body}</div></section>`;
  const tiles = h.most_played.length ? panel("Most played this week", `<div class="tiles">${h.most_played.slice(0, 7).map((g, i) => `<div class="tile${i === 0 ? " big" : ""}">
      ${gameArt(g.game_id, g.name)}
      <span class="rk">#${i + 1}</span><div class="cap">${esc(g.name)}<br><b>${hrsShort(g.minutes)} h</b> <span class="pl">· ${plural(g.players, "player")}</span></div></div>`).join("")}</div>`) : "";
  const feed = h.now_playing.length ? panel("Now playing", `<div class="feed2">${h.now_playing.map(n => `<div>${avatar(n.avatar, 24)}<span><a href="/u/${encodeURIComponent(n.handle)}">${esc(n.display_name || n.handle)}</a><br>${icon(n.game_id, n.game_name)} ${esc(n.game_name)}</span>${n.since ? `<small>${since(n.since)}</small>` : ""}</div>`).join("")}</div>`) : "";
  const lb = h.top_players.length ? panel("Top players this week", `<div class="lb">${h.top_players.map((p, i) => `<div><span class="n">${i + 1}</span>${avatar(p.avatar, 24)}<span class="who"><a href="/u/${encodeURIComponent(p.handle)}">${esc(p.display_name || p.handle)}</a>${p.top_game ? `<small>${esc(p.top_game)}</small>` : ""}</span><span class="h">${hrsShort(p.minutes)} h</span></div>`).join("")}</div>`) : "";
  const mem = h.newest.length ? panel("Newest members", `<div class="mem">${h.newest.map(m => `<div><a href="/u/${encodeURIComponent(m.handle)}">${avatar(m.avatar, 40)}${esc(m.display_name || m.handle)}</a></div>`).join("")}</div>`) : "";
  const news = panel("News", h.news.length ? `<div class="nws">${h.news.map(n => `<a class="nw" href="${esc(n.url)}"><span class="ic">F</span><div><small>${fullDate.format(new Date(n.at))}${n.pinned ? ' · <span class="pin">Pinned</span>' : ""}</small><b>${esc(n.title)}</b>${n.summary ? `<p>${esc(n.summary.length > 110 ? n.summary.slice(0, 108).replace(/\s+\S*$/, "") + "…" : n.summary)}</p>` : ""}</div></a>`).join("")}</div>` : `<p class="empty" style="padding:6px 0">No news yet.</p>`, ` <a class="more" href="/news">All news ›</a>`);
  let friendsBox = "", actBox = "";
  if (fl) {
    const on = fl.friends.filter(f => f.presence && f.presence.status !== "offline").sort((a, b) => rank(b.presence) - rank(a.presence));
    friendsBox = panel(`Friends online (${fl.online})`, on.length ? `<div class="feed2">${on.slice(0, 10).map(f => `<div>${avatar(f.avatar, 24)}<span><a href="/u/${encodeURIComponent(f.handle)}">${esc(f.display_name || f.handle)}</a><br>${f.presence.game_name ? (isUnlisted(f.presence.game_id) ? "Playing a game" : `${icon(f.presence.game_id, f.presence.game_name)} ${esc(f.presence.game_name)}`) : `<span style="color:var(--online)">Online</span>`}</span></div>`).join("")}</div>` : `<span class="empty">${fl.all ? "None of your friends are online." : `No friends yet. <a href="/friends">Add someone</a>.`}</span>`, ` <a class="more" href="/friends">All friends</a>`);
  }
  if (act && act.items.length) {
    let f = "", last = "";
    act.items.slice(0, 12).forEach(i => { const d = actDay(i); if (d !== last) { f += `<div class="day">${d}</div>`; last = d; }
      f += `<div class="act">${icon(i.game_id, i.game_name)}<span><a href="/u/${encodeURIComponent(i.handle)}">${esc(i.display_name || i.handle)}</a> ${actText(i)}</span></div>`; });
    actBox = panel("Friends' activity", f);
  }
  const why = u ? "" : panel("Why Fraglink", `<p style="margin:0 0 8px"><b>Friends, live.</b> See who's online and what they're playing, with a quiet pop-up when a friend starts a game.</p><p style="margin:0 0 8px"><b>Hours that add up.</b> Play time is counted game by game while the app runs.</p><p style="margin:0"><b>Light and private.</b> Never reads game memory or keystrokes. Export or delete your data any time.</p>`);
  const left = u ? friendsBox + lb : lb + mem;
  const mid = tiles + actBox;
  const right = feed + (u ? mem : "") + news + why;
  const cols = [left, mid, right].filter(Boolean);
  live.innerHTML = ticker + stats + (cols.length === 3 ? `<div class="g3"><div class="col">${left}</div><div class="col">${mid}</div><div class="col">${right}</div></div>`
    : `<div class="g2v"><div class="col">${mid || left}</div><div class="col">${mid ? left + right : right}</div></div>`);
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
  loadTurnstile();
  const f = $("#signup"), err = $("#err");
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true;
    if (f.password.value !== f.password2.value) { showErr(err, new Error("The passwords don't match.")); return; }
    if (!f.terms.checked) { showErr(err, new Error("Please accept the Terms and Privacy Policy.")); return; }
    if (!f.age_ok.checked) { showErr(err, new Error("Please tick the box to confirm you're 13 or older (16 or older in the EU/UK).")); return; }
    const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/signup", { method: "POST", body: { email: f.email.value.trim(), handle: f.handle.value.trim(), password: f.password.value, age_ok: f.age_ok.checked, turnstile: turnstileToken(f) } });
      setSession(r.token, r.user);
      location.href = "/u/" + encodeURIComponent(r.user.handle) + "?welcome=1" + (r.user.email ? "&sent=1" : "");
    } catch (x) { showErr(err, x); b.disabled = false; turnstileReset(); }
  });
};

pages.forgot = () => {
  loadTurnstile();
  const f = $("#forgot"), err = $("#err");
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true; const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/forgot", { method: "POST", body: { email: f.email.value.trim(), turnstile: turnstileToken(f) } });
      showOk(err, r.mail_live ? "If that email is on a Fraglink account, a reset link is on its way. It works for 1 hour. Can't find it? Check your junk or spam folder." : "Thanks. Reset emails aren't being delivered yet, so please contact support (below) to get back in.");
    } catch (x) { showErr(err, x); turnstileReset(); }
    b.disabled = false;
  });
};

pages.reset = () => {
  const f = $("#reset"), err = $("#err"), t = qs("token");
  if (!t) { showErr(err, new Error("This page needs the link from your email.")); return; }
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true;
    if (f.password.value !== f.password2.value) return showErr(err, new Error("The passwords don't match."));
    try { await api("/auth/reset", { method: "POST", body: { token: t, password: f.password.value } }); clearSession(); f.hidden = true; showOk(err, "Password changed. You can sign in now."); err.insertAdjacentHTML("afterend", `<p><a class="btn dl" href="/login">Sign in</a></p>`); }
    catch (x) { showErr(err, x); }
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
  const handle = decodeURIComponent(location.pathname.replace(/^\/u\/?/, "").replace(/\/$/, ""));
  if (!handle) { location.replace(me() ? "/u/" + encodeURIComponent(me().handle) : "/login"); return; }
  document.title = handle + " · Fraglink";
  let p;
  try { p = await api("/users/" + encodeURIComponent(handle) + "/profile?limit=" + GPAGE, { keepSession: true }); }
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
  if (p.handle && p.handle.toLowerCase() !== String(handle).toLowerCase()) history.replaceState(null, "", "/u/" + encodeURIComponent(p.handle) + location.search + location.hash);
  if (p.private) {
    root.innerHTML = `<div class="narrow panel"><h2>${esc(p.display_name || p.handle)}</h2><div class="body" style="display:flex;gap:16px;align-items:center">${avatar(p.avatar, 64)}<div><p>This profile is private. Only ${esc(p.display_name || p.handle)}'s friends can see it.</p>
      ${!token() ? `<a class="btn" href="/login?next=${encodeURIComponent(location.pathname)}">Sign in</a>` : p.relation === "none" ? `<button class="btn blue" data-add>+ Add friend</button>` : p.relation === "outgoing" ? `<span class="muted">Friend request sent</span>` : p.relation === "incoming" ? `<a class="btn" href="/friends">Answer friend request</a>` : ""}</div></div></div>`;
    const add = root.querySelector("[data-add]");
    if (add) add.addEventListener("click", async () => { add.disabled = true; try { await api("/friends/request", { method: "POST", body: { handle: p.handle } }); add.textContent = "Friend request sent"; } catch (x) { add.disabled = false; alert(x.message); } });
    return;
  }
  const pres = p.presence;
  const ids = [].concat((p.games || []).map(g => g.game_id), (p.recent || []).map(r => r.game_id), pres && pres.game_id ? [pres.game_id] : []);
  await loadIcons(ids);
  document.title = (p.display_name || p.handle) + " · Fraglink";
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
  const BRAND = { steam: ["Steam", "#66c0f4", "M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z"], bnet: ["Battle.net", "#148eff", "M18.94 8.296C15.9 6.892 11.534 6 7.426 6.332c.206-1.36.714-2.308 1.548-2.508 1.148-.275 2.4.48 3.594 1.854.782.102 1.71.28 2.355.429C12.747 2.013 9.828-.282 7.607.565c-1.688.644-2.553 2.97-2.448 6.094-2.2.468-3.915 1.3-5.013 2.495-.056.065-.181.227-.137.305.034.058.146-.008.194-.04 1.274-.89 2.904-1.373 5.027-1.676.303 3.333 1.713 7.56 4.055 10.952-1.28.502-2.356.536-2.946-.087-.812-.856-.784-2.318-.19-4.04a26.764 26.764 0 0 1-.807-2.254c-2.459 3.934-2.986 7.61-1.143 9.11 1.402 1.14 3.847.725 6.502-.926 1.505 1.672 3.083 2.74 4.667 3.094.084.015.287.043.332-.034.034-.06-.08-.124-.131-.149-1.408-.657-2.64-1.828-3.964-3.515 2.735-1.929 5.691-5.263 7.457-8.988 1.076.86 1.64 1.773 1.398 2.595-.336 1.131-1.615 1.84-3.403 2.185a27.697 27.697 0 0 1-1.548 1.826c4.634.16 8.08-1.22 8.458-3.565.286-1.786-1.295-3.696-4.053-5.17.696-2.139.832-4.04.346-5.588-.029-.08-.106-.27-.196-.27-.068 0-.067.13-.063.187.135 1.547-.263 3.2-1.062 5.19zm-8.533 9.869c-1.96-3.145-3.09-6.849-3.082-10.594 3.702-.124 7.474.748 10.714 2.627-1.743 3.269-4.385 6.1-7.633 7.966h.001z"], discord: ["Discord", "#5865f2", "M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z"] };
  const badges = (p.linked || []).filter(l => BRAND[l.provider]).length ? `<div class="lbadges">${p.linked.filter(l => BRAND[l.provider]).map(l => {
    const [nm, col, path] = BRAND[l.provider], tip = esc(nm + (l.display ? ": " + l.display : " (verified)"));
    const inner = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${path}"/></svg><i class="vt" aria-hidden="true">✓</i>`;
    return l.url ? `<a class="lb" href="${esc(l.url)}" target="_blank" rel="noopener" style="--bc:${col}" data-tip="${tip}" aria-label="${tip}">${inner}</a>` : `<span class="lb" tabindex="0" style="--bc:${col}" data-tip="${tip}" aria-label="${tip}">${inner}</span>`;
  }).join("")}</div>` : "";
  const np = live ? `<div class="np">${gameArt(pres.game_id, pres.game_name, "npa")}<div><div style="font-size:10px;letter-spacing:1px;text-transform:uppercase;opacity:.8">Now playing</div>
      <b>${isUnlisted(pres.game_id) ? "A game" : esc(pres.game_name)}</b>${pres.game_detail ? ` <span style="opacity:.8">· ${esc(pres.game_detail)}</span>` : ""}</div>${pres.session_started_at ? `<span style="opacity:.8">for ${since(pres.session_started_at)}</span>` : ""}</div>` : "";
  const sec = (title, body, right, cls, id) => `<section class="panel sx${cls ? " " + cls : ""}"${id ? ` id="${id}"` : ""}><h2><span class="tab">${title}</span>${right ? `<span class="rt">${right}</span>` : ""}</h2>${body}</section>`;
  const report = !self && token() ? `<div class="avlinks">${p.avatar && p.avatar.kind === "upload" ? `<a href="#" class="report" data-report>Report picture</a>` : ""}<a href="#" class="report flagit" data-report-profile>&#9873; Report this profile</a></div>` : "";
  const kpi = `<div class="kpi">${p.history_visible ? `<div><b>${total > 0 && total < 60 ? "&lt;1" : nf.format(Math.round(total / 60))}</b><span>Total hours</span></div><div><b>${hrsShort(p.overall.minutes_7d)}</b><span>Past 7 days</span></div>` : ""}${self || Array.isArray(p.friends) ? `<a href="${self ? "/friends" : "#friends"}" class="kf"><b>${p.friend_count}</b><span>Friends</span></a>` : `<div><b>${p.friend_count}</b><span>Friends</span></div>`}</div>`;
  const nick = p.display_name || p.handle, prevs = (p.previous_names || []).filter(n => n.toLowerCase() !== nick.toLowerCase() && n.toLowerCase() !== p.handle.toLowerCase());
  const head = `<section class="ph${bannerG ? "" : " noart"}"${bannerG ? ` style="background:${artBg(bannerG.game_id, bannerG.name)}"` : ""}><div class="inr">
      <div>${avatar(p.avatar, 184)}${report}</div>
      <div class="who"><h1><span class="pname"${prevs.length ? ` tabindex="0" data-tip="${esc("Previously known as: " + prevs.join(", "))}"` : ""}>${esc(p.display_name || p.handle)}</span>${p.display_name && p.display_name !== p.handle ? ` <small style="font-size:15px;color:#c8d4e0">@${esc(p.handle)}</small>` : ""}</h1>
      <div class="sub">${sub}</div>${badges}${np}<div class="acts">${actions}</div></div>${kpi}</div></section>`;

  const welcome = qs("welcome") && self ? `<div class="alert ok" style="margin-bottom:12px">Welcome to Fraglink. <a href="/download">Download the app</a> and sign in with the same account to start tracking. You can pick a picture in <a href="/settings">Settings</a>.${qs("sent") ? " We sent a link to confirm your email. Can't find it? Check your junk or spam folder." : ""}</div>` : "";
  const pendingDelete = self && p.delete_after ? `<div class="alert err" style="margin-bottom:12px">This account will be deleted on ${fullDate.format(new Date(p.delete_after))}. Sign in again to keep it.</div>` : "";

  // Miniprofile: the live signature image + embed codes (public profiles only)
  const shareUrl = siteBase() + "/u/" + encodeURIComponent(p.handle), sig = sigUrl(p.handle);
  const codes = [["Link", shareUrl], ["BBCode", `[url=${shareUrl}][img]${sig}[/img][/url]`], ["HTML", `<a href="${shareUrl}"><img src="${sig}" alt="${p.display_name || p.handle} on Fraglink" width="460" height="100"></a>`]];
  const miniprofile = p.public_profile
    ? sec("Miniprofile", `<div class="body mp"><div class="mpt">Embed this anywhere (forums, blogs, Discord). It updates live.</div>
        <img class="sigimg" src="/api/v1/sig/${encodeURIComponent(p.handle)}.png${sigBust()}" alt="Signature for ${esc(p.handle)}" width="460" height="100">
        <div class="mpcodes">${codes.map(([l, v]) => `<div class="code"><b>${l}</b><input class="t" readonly value="${esc(v)}"><button class="btn sm" type="button" data-copy="${esc(v)}">Copy</button></div>`).join("")}</div></div>`)
    : self ? sec("Miniprofile", `<div class="body empty">Your profile is private (friends only), so there's no public link or signature image. <a href="/settings#privacy">Privacy settings</a></div>`) : "";

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
    const row = g => `<tr><td><span class="gico">${icon(g.listed === false && !self ? "manual:unlisted" : g.game_id, g.name)}</span>${g.listed === false && !self ? `<span class="empty">${esc(g.name)}</span>` : esc(g.name)}${g.tier && g.tier !== "tracked" ? ` <span class="tier">${esc(g.tier)}</span>` : ""}</td>
      <td class="n">${hours(g.minutes_7d)}</td><td class="n">${hours(g.total_min)}</td><td class="n hide-sm">${live && pres.game_id === g.game_id ? `<span style="color:var(--online)">Now</span>` : lastPlayed(g.last_played_at)}</td></tr>`;
    const many = (p.games_total || p.games.length) > GPAGE;
    games = sec("Games", `
      <table class="gt" style="width:100%;border-collapse:collapse"><thead><tr>${[["name", "Game", ""], ["week", "Past 7 days", "n"], ["total", "Total", "n"], ["recent", "Last played", "n hide-sm"]].map(([k, l, c]) => many ? `<th class="${c} srt${k === "total" ? " on" : ""}" data-gs="${k}" tabindex="0" role="button">${l}<i>${k === "total" ? " ▾" : ""}</i></th>` : `<th class="${c}">${l}</th>`).join("")}</tr></thead>
      <tbody data-gbody>${p.games.map(row).join("")}</tbody>
      <tfoot><tr class="tot"><td>Overall hours</td><td class="n">${hours(p.overall.minutes_7d)}</td><td class="n">${hours(p.overall.total_min)}</td><td class="hide-sm"></td></tr></tfoot></table>
      ${many ? `<div data-gpager>${pagerHtml(0, GPAGE, p.games_total)}</div>` : ""}
      <div class="note">Hours counted by the Fraglink app while you play, plus imported game counters.</div>`);
    gamesRow = row;
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
    const d = new Date(r.ended_at), k = d.toDateString();
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

  root.innerHTML = `<div class="pv3">` + head + pendingDelete + welcome + `<div class="g2v"><div class="col">${details}${games}${ach}${miniprofile}</div><aside class="col">${mini}${rig}${fr}${discord}</aside></div></div>`;
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
    const st = { offset: 0, sort: "total" };
    const load = async keepPos => {
      const qs = new URLSearchParams({ show: "games", limit: GPAGE, offset: st.offset, sort: st.sort });
      gbody.style.opacity = .5;
      try {
        const r = await api("/users/" + encodeURIComponent(p.handle) + "/profile?" + qs, { keepSession: true });
        await loadIcons(r.games.map(g => g.game_id));
        gbody.innerHTML = r.games.map(gamesRow).join("");
        gpg.innerHTML = pagerHtml(st.offset, GPAGE, r.total);
        if (keepPos) { const t = gbody.closest("section"); if (t.getBoundingClientRect().top < 0) t.scrollIntoView({ block: "start" }); }
      } catch (x) { gpg.textContent = x.message; }
      gbody.style.opacity = 1;
    };
    gpg.addEventListener("click", e => { const b = e.target.closest("[data-off]"); if (!b) return; e.preventDefault(); st.offset = +b.dataset.off; load(true); });
    const sortBy = th => { st.sort = th.dataset.gs; st.offset = 0; $$("th.srt", root).forEach(x => { x.classList.toggle("on", x === th); x.querySelector("i").textContent = x === th ? (st.sort === "name" ? " ▴" : " ▾") : ""; }); load(); };
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
      <section class="panel"><h2>Add a friend</h2><div class="body"><form class="f" id="addf"><label>Username<input type="text" name="handle" maxlength="20" required placeholder="their Fraglink username" autocomplete="off"></label><div class="row"><button class="btn">Send request</button></div></form></div></section>
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
        showOk(msg, r && r.status === "accepted" ? `You and ${h} are now friends.` : `Friend request sent to ${h}.`); load(); }
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
      msg.hidden = true; load();
    } catch (x) { b.disabled = false; showErr(msg, x); }
  });
  load();
};

/* ----- settings (tabs) ----- */
const RIG_FIELDS = [["manufacturer", "Manufacturer", "e.g. Custom build, Alienware"], ["cpu", "Processor", "e.g. Ryzen 7 7800X3D"], ["gpu", "Video card", "e.g. RTX 4070"], ["memory", "Memory", "e.g. 32 GB DDR5"], ["storage", "Storage", "e.g. 2 TB NVMe"], ["motherboard", "Motherboard", ""], ["monitor", "Monitor", "e.g. 27\" 1440p 165 Hz"], ["headset", "Headset", ""], ["keyboard", "Keyboard", ""], ["mouse", "Mouse", ""], ["mousepad", "Mouse pad", ""], ["case", "Case", ""], ["os", "Operating system", "e.g. Windows 11"]];
const STYLES = ["Casual", "Weekend warrior", "Competitive", "Completionist", "MMO addict", "Sniper only", "Support main", "Speedrunner", "Explorer", "Clueless"];
const TABS = [["account", "Account & profile"], ["rig", "Gaming rig"], ["games", "Played games"], ["privacy", "Privacy"], ["notifications", "Notifications"], ["connections", "Connections"], ["security", "Security"]];
pages.settings = async () => {
  if (!requireLogin()) return;
  const root = $("#settings");
  let m;
  try { m = await api("/me"); } catch (e) { if (!token()) return requireLogin(); root.innerHTML = `<div class="alert err">${esc(e.message)}</div>`; return; }
  let u = m.user;
  const keep = () => setSession(token(), Object.assign(me() || {}, { id: u.id, handle: u.handle, display_name: u.display_name, email: u.email, avatar: u.avatar, email_verified: u.email_verified, mail_live: u.mail_live, staff_role: u.staff_role || null }));
  keep(); renderHeader(); verifyBar();
  const tab = () => { const t = location.hash.replace("#", ""); return TABS.some(x => x[0] === t) ? t : "account"; };
  const row = (label, html, hint) => `<label>${label}</label><div>${html}${hint ? `<div class="sub">${hint}</div>` : ""}</div>`;
  const msg = `<div class="alert" hidden data-msg></div>`;
  const ok = (el, t) => showOk($("[data-msg]", el), t), bad = (el, e) => showErr($("[data-msg]", el), e);

  function accountTab() {
    const verified = u.email ? (u.email_verified ? `<span style="color:var(--online)">✓ confirmed</span>` : `<span style="color:var(--away)">not confirmed</span> · <a href="#" data-resend>${u.mail_live ? "Send the link again" : "Confirmation emails are coming soon"}</a>`) : `<span class="empty">none</span>`;
    const canUp = u.avatar_upload_allowed;
    return `<h3>Profile picture</h3>${msg}<div class="frm">
      ${row("Current", `<div class="avsizes"><div>${avatar(u.avatar, 184)}<small>184 profile</small></div><div>${avatar(u.avatar, 64)}<small>64 panels</small></div><div>${avatar(u.avatar, 32)}<small>32 lists</small></div></div>`)}
      ${row("Upload", `<div class="row"><input type="file" id="av-file" accept="image/png,image/jpeg,image/webp,image/gif" hidden><button class="btn blue" type="button" data-upload ${canUp ? "" : "disabled"}>Upload a picture…</button><button class="btn" type="button" data-avreset>Reset to default</button></div>`,
        canUp ? "PNG, JPEG, WebP or GIF (first frame), up to 2 MB. Cropped to a square. Keep it friendly: pictures can be reported and removed. 5 changes a day." : esc(u.avatar_upload_block || "Uploads aren't available right now."))}
      ${row("Or pick a preset", `<div class="presets">${PRESET_GLYPHS.map((g, i) => `<button type="button" data-preset="${i}" class="${u.avatar.kind === "preset" && u.avatar.preset === i ? "on" : ""}" title="Preset ${i + 1}">${avatar({ kind: "preset", preset: i }, 48)}</button>`).join("")}</div>`, "Your default colour comes from your account.")}
    </div>
    <h3>Account</h3><form data-f="name">${msg}<div class="frm">
      ${row("Username", `<b>${esc(u.handle)}</b> <small style="color:#8a96a3">playfraglink.com/u/${esc(u.handle)}</small>`, "Your sign-in name and profile address. It's permanent; ask staff if it really needs changing.")}
      ${row("Nickname", `<input class="t" name="display_name" maxlength="32" value="${esc(u.display_name || "")}">`, "The name everyone sees, in the app and on the site. Up to 32 characters, 3 changes a day. It can't be someone else's name or look like staff. Leave empty to use your username.")}
      ${row("Email", `${esc(u.email || "")} ${verified}`)}
      ${row("", `<button class="btn dl">Save</button>`)}</div></form>
    <h3>About you</h3><form data-f="about">${msg}<div class="frm">
      ${row("About me", `<textarea class="t" name="about" maxlength="300">${esc(u.about || "")}</textarea>`, "Up to 300 characters.")}
      ${row("Location", `<input class="t" name="location" maxlength="40" value="${esc(u.location || "")}" placeholder="e.g. Sydney, Australia">`, "Optional. Shown on your profile as typed.")}
      ${row("Country", `<select class="t" name="country"><option value="">(not shown)</option>${COUNTRIES().map(([c, n]) => `<option value="${c}" ${(u.details || {}).country === c ? "selected" : ""}>${esc(n)}</option>`).join("")}</select>`, "Shows a small flag next to your location.")}
      ${row("Real name", `<input class="t" name="real_name" maxlength="40" value="${esc((u.details || {}).real_name || "")}">`, "Optional.")}
      ${row("Gender", `<input class="t" name="gender" maxlength="20" value="${esc((u.details || {}).gender || "")}">`, "Optional, as you'd like it shown.")}
      ${row("Birthday", `<input class="t" type="date" name="birth_date" value="${esc((u.details || {}).birth_date || "")}">`, "Optional. Only your age is shown, never the date. Hide it in Privacy.")}
      ${row("Gaming style", `<select class="t" name="gaming_style"><option value="">(none)</option>${STYLES.map(s => `<option ${u.gaming_style === s ? "selected" : ""}>${s}</option>`).join("")}</select>`)}
      ${row("", `<button class="btn dl">Save</button>`)}</div></form>
    <h3>Change email</h3><form data-f="email">${msg}<div class="frm">
      ${row("New email", `<input class="t" type="email" name="new_email" required autocomplete="email">`)}
      ${row("Current password", `<input class="t" type="password" name="current_password" required autocomplete="current-password">`)}
      ${row("", `<button class="btn">Change email</button>`)}</div></form>
    <h3>Your data</h3>${msg}<div class="frm">
      ${row("Download my data", `<button class="btn" type="button" data-export>Download (zip)</button>`, "Everything we store about you: account, sessions, hours, friends, as JSON and CSV. Once an hour.")}
      ${row("Delete my account", `<button class="btn red" type="button" data-delete>Delete account…</button>`, "Signs you out everywhere now and deletes everything after 7 days. Signing in again during those 7 days cancels it.")}
    </div><form data-f="delete" hidden>${msg}<div class="frm"><div class="danger" style="grid-column:1/-1">Your profile disappears now and your account and all data are deleted for good after 7 days.</div>
      ${row("Password", `<input class="t" type="password" name="password" required autocomplete="current-password">`)}
      ${u.mfa && u.mfa.enabled ? row("Two-step code", `<input class="t" name="code" required autocomplete="one-time-code">`) : ""}
      ${row("", `<button class="btn red">Delete my account</button>`)}</div></form>`;
  }
  function rigTab() {
    const r = u.rig || {};
    return `<h3>Gaming rig</h3><form data-f="rig">${msg}<div class="frm">${RIG_FIELDS.map(([k, l, ph]) => row(l, `<input class="t" name="${k}" maxlength="60" value="${esc(r[k] || "")}" placeholder="${esc(ph)}">`)).join("")}
      ${row("", `<button class="btn dl">Save rig</button>`, "Shown on your profile. Leave everything empty to hide the box.")}</div></form>`;
  }
  async function gamesTab(el) {
    const PG = 50, st = { offset: 0 };
    el.innerHTML = `<h3>Played games</h3>${msg}<div class="empty-v2">Loading…</div>`;
    let first; try { first = await api(`/me/games?limit=${PG}`); } catch (e) { bad(el, e); return; }
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
      try { await render(await api("/me/games?" + qs), keepPos); } catch (x) { bad(el, x); }
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
    return `<h3>Privacy</h3><form data-f="privacy">${msg}<div class="frm">
      ${row("Private profile", `<label><input type="checkbox" name="private_profile" ${priv ? "checked" : ""}>Only my friends can see my profile</label>`, "Your profile is public by default. Private: everyone else just sees your name and picture, you're left out of the home page lists, and your signature image is switched off.")}
      ${row("Show on my profile", `${sw("now_playing", "Now Playing (the game I'm in right now)")}${sw("games", "Games &amp; hours")}<label style="display:block;margin:3px 0"><input type="checkbox" name="share_activity" ${u.share_activity ? "checked" : ""}>Activity (recent play history)</label><label style="display:block;margin:3px 0"><input type="checkbox" name="show_friends" ${u.show_friends ? "checked" : ""}>Friends list</label>${sw("rig", "Gaming rig")}${sw("name_history", "Previous nicknames (\"Previously known as\")")}`, "Hiding Now Playing also keeps you off the home page Now Playing list and your signature image. Your friends still see your status in the app; pick Invisible there to hide it.")}
      ${row("Personal details", `${sw("about", "About me")}${sw("location", "Location &amp; country")}${sw("gaming_style", "Gaming style")}${sw("real_name", "Real name")}${sw("gender", "Gender")}${sw("age", "Age")}`)}
      ${row("Achievements", `${sw("achievements", "Show my achievements")}`, "Needs Games &amp; hours shown.")}
      ${row("In the app", `<span class="sub">Whether friends see the game you're in right now (and friends of friends) is set per PC in the desktop app: Options &gt; Privacy. The switches on this page show there too.</span>`)}
      ${row("Leaderboards", `${sw("leaderboards", "Show me in Top players on the home page")}`, "Needs Games &amp; hours shown.")}
      ${row("Your link", `<div class="share"><input class="t" readonly value="${esc(share)}"><button class="btn sm" type="button" data-copy="${esc(share)}">Copy</button></div>`)}
      ${!priv ? row("Signature image", `<img class="sigimg" src="/api/v1/sig/${encodeURIComponent(u.handle)}.png${sigBust()}" alt="" width="460" height="100"><div class="share" style="margin-top:6px"><input class="t" readonly value="${esc(sigUrl(u.handle))}"><button class="btn sm" type="button" data-copy="${esc(sigUrl(u.handle))}">Copy</button></div>`, "Paste the image link into forum signatures. It refreshes within about 30 seconds.") : ""}
      </div></form>`;
  }
  function notificationsTab() {
    const n = u.notify_prefs || {};
    const cb = (k, t) => `<label style="display:block;margin:2px 0"><input type="checkbox" name="${k}" ${n[k] ? "checked" : ""}>${t}</label>`;
    return `<h3>Notifications</h3><form data-f="notify">${msg}<div class="frm">
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
      const right = l
        ? `${p === "steam" && v.import ? `<button class="btn sm" type="button" data-import>Import hours</button> ` : ""}<button class="btn sm" type="button" data-unlink="${p}">Unlink</button>`
        : v.off ? `<span class="btn sm" aria-disabled="true">Turned off for now</span>` : v.available ? `<button class="btn sm blue" type="button" data-link="${p}">${p === "steam" ? "Sign in through Steam" : "Link " + NAMES[p]}</button>` : `<span class="btn sm" aria-disabled="true">Coming soon</span>`;
      const who = l ? `${p === "steam" ? `<a href="https://steamcommunity.com/profiles/${esc(l.uid)}" rel="noopener">${esc(l.display || l.uid)}</a>` : `<b>${esc(l.display || l.uid)}</b>`} ${l.verified ? `<span class="vbadge">✓ Verified</span>` : `<span class="sub">(not verified)</span>`}${l.last_sync_at ? `<div class="sub">Hours imported ${fullDate.format(new Date(l.last_sync_at))}${l.sync_status && l.sync_status !== "ok" ? " · " + esc(l.sync_status) : ""}</div>` : ""}`
        : `<span class="sub">${blurb}</span>`;
      return `<div class="conn"><span class="cl">${logo}</span><div style="flex:1"><b style="display:block">${NAMES[p]}</b>${who}</div><div>${right}</div></div>`;
    };
    box.innerHTML = msg + `<div style="padding:0 14px">
      ${card("steam", "S", c.steam && c.steam.import ? "Verify your Steam account and import your Steam hours." : "Verify your Steam account. It shows as a badge on your profile.")}
      ${card("discord", "D", "Show your Discord name on your profile.")}
      ${card("bnet", "B", "Show your BattleTag on your profile.")}
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
    el.innerHTML = `<h3>Password</h3><form data-f="pw">${msg}<div class="frm">
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
      <div class="note" style="padding:0 14px 10px">Countries are approximate, worked out on our own server. <a href="https://db-ip.com" target="_blank" rel="noopener">IP Geolocation by DB-IP</a>. Signed-out sign-ins are removed after 30 days, and desktop apps unused for 180 days are signed out.</div>`;
  }

  function render() {
    const t = tab();
    root.innerHTML = `<h1 style="margin:14px 0 0;font-size:22px">Account settings</h1><nav class="tabs">${TABS.map(([k, l]) => `<a href="#${k}" class="${k === t ? "on" : ""}">${l}</a>`).join("")}</nav><div class="tabpane" id="pane"></div>`;
    const el = $("#pane");
    if (t === "account") el.innerHTML = accountTab();
    else if (t === "rig") el.innerHTML = rigTab();
    else if (t === "games") gamesTab(el);
    else if (t === "privacy") el.innerHTML = privacyTab();
    else if (t === "notifications") el.innerHTML = notificationsTab();
    else if (t === "connections") connectionsTab(el);
    else if (t === "security") securityTab(el);
  }
  async function refresh() { u = (await api("/me")).user; keep(); renderHeader(); }
  window.addEventListener("hashchange", render);
  root.addEventListener("submit", async e => {
    const f = e.target.closest("form[data-f]"); if (!f) return; e.preventDefault();
    const k = f.dataset.f, v = n => f.elements[n] ? f.elements[n].value : undefined;
    try {
      if (k === "name") { const r = await api("/me/display_name", { method: "POST", body: { display_name: v("display_name") } }); u.display_name = r.display_name; keep(); renderHeader(); ok(f, r.changes_left == null ? "Saved." : `Saved. ${r.changes_left} more change${r.changes_left === 1 ? "" : "s"} today.`); }
      else if (k === "about" || k === "rig") {
        const rig = Object.assign({}, u.rig || {}); if (k === "rig") RIG_FIELDS.forEach(([n]) => { rig[n] = v(n); });
        const body = { about: k === "about" ? v("about") : u.about, location: k === "about" ? v("location") : u.location, gaming_style: (k === "about" ? v("gaming_style") : u.gaming_style) || null, rig };
        if (k === "about") Object.assign(body, { country: v("country"), real_name: v("real_name"), gender: v("gender"), birth_date: v("birth_date") });
        const r = await api("/me/profile", { method: "POST", body }); Object.assign(u, r); ok(f, "Saved.");
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
    } catch (x) { bad(f, x); }
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
        const t = e.target, body = t.dataset.pv ? { privacy: { [t.dataset.pv]: t.checked } } : t.name === "private_profile" ? { public_profile: !t.checked } : { [t.name]: t.checked };
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
      else if (b.matches("[data-export]")) {
        const r = await fetch(API + "/export.zip", { headers: { Authorization: "Bearer " + token() } });
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
        $("[data-mfa]").innerHTML = `${msg}<div class="frm">${row("Status", `<b style="color:var(--online)">On</b>`)}${row("Backup codes", `<div class="mpcodes">${r.backup_codes.map(c => `<span>${esc(c)}</span>`).join("")}</div>`, "Save these somewhere safe. Each one signs you in once if you lose your phone. They won't be shown again.")}${row("", `<button class="btn" type="button" onclick="location.reload()">Done</button>`)}</div>`;
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
  const u = me(); if (!u || !u.email || u.email_verified || $(".verify-bar") || sessionStorage.getItem("fl_vb_closed")) return;
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
      if (p.slug !== slug) history.replaceState(null, "", p.url);
      root.innerHTML = `<p class="crumb"><a href="/news">‹ All news</a></p><article class="panel post"><div class="body">
        <h1>${esc(p.title)}</h1><div class="sub">${fullDate.format(new Date(p.at))}${p.pinned ? ' · <span class="pin">Pinned</span>' : ""}</div>
        <div class="txt">${newsBody(p.body) || `<p class="muted">${esc(p.summary)}</p>`}</div>
        ${p.link ? `<p><a class="btn blue" href="${esc(p.link)}"${p.link.startsWith("http") ? ' rel="noopener"' : ""}>Read more</a></p>` : ""}</div></article>
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
      <div class="panel"><div class="body nlist">${d.items.length ? d.items.map(n => `<a class="nrow" href="${esc(n.url)}"><small>${fullDate.format(new Date(n.at))}${n.pinned ? ' · <span class="pin">Pinned</span>' : ""}</small><b>${esc(n.title)}</b>${n.summary ? `<p>${esc(n.summary)}</p>` : ""}</a>`).join("") : `<p class="empty">No news yet.</p>`}</div></div>
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
const FEATURES = { signups: true, announcement: true, connect_steam: true, connect_bnet: true, connect_discord: true, friend_requests: true, public_profiles: true, maintenance: false };
const featuresReady = fetch(API + "/features", { cache: "no-store" }).then(r => r.ok ? r.json() : null).then(d => { if (d && d.features) Object.assign(FEATURES, d.features); }).catch(() => {});
function applyFeatures() {
  document.body.classList.toggle("ff-no-friendreq", !FEATURES.friend_requests);
  document.body.classList.toggle("ff-no-search", !FEATURES.public_profiles);
  const page = document.body.dataset.page, staff = !!(me() && me().staff_role);
  if (FEATURES.maintenance && !staff && page !== "login" && page !== "admin") {
    $("main").innerHTML = `<div class="maint panel"><div class="body"><h1>Down for maintenance</h1><p>Fraglink is getting some work done. We'll be back soon.</p><p class="muted">News and updates: <a href="https://discord.gg/QbaYRtvADj" rel="noopener">our Discord</a>.</p></div></div>`;
    return false;
  }
  if (!FEATURES.signups && page === "signup") {
    const f = $("#signup"); if (f) f.outerHTML = `<p>Sign-ups are closed right now. Please check back soon.</p><p><a href="/login">Already have an account? Sign in</a></p>`;
    return false;
  }
  return true;
}

/* ---------- boot ---------- */
document.addEventListener("DOMContentLoaded", async () => {
  await featuresReady;
  renderHeader();
  if (!applyFeatures()) return;
  const y = $("#year"); if (y) y.textContent = new Date().getFullYear();
  banner();
  const f = pages[document.body.dataset.page];
  if (f) Promise.resolve(f()).catch(e => { console.error(e); });
  // keep the header in sync with the real account (handle/display name changes)
  if (token() && document.body.dataset.page !== "settings" && document.body.dataset.page !== "verify") api("/me").then(m => { setSession(token(), { id: m.user.id, handle: m.user.handle, display_name: m.user.display_name, email: m.user.email, staff_role: m.user.staff_role || null, avatar: m.user.avatar, email_verified: m.user.email_verified, mail_live: m.user.mail_live }); renderHeader(); verifyBar(); }).catch(() => { if (!token()) renderHeader(); });
  else verifyBar();
  if (qs("deleted")) { const d = new Date(qs("deleted")); const el = document.createElement("div"); el.className = "verify-bar delete-bar"; el.innerHTML = `<div class="wrap">Your account is scheduled for deletion${isNaN(d) ? "" : " on " + fullDate.format(d)}. Sign in again before then to keep it.</div>`; ($(".preview-flag") || $(".top")).after(el); }
});
})();
