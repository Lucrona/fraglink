/* Fraglink website: plain JS, no build step. Talks to the server's JSON API with a
   bearer token (kept in localStorage after sign-in). Pages pick their code via <body data-page>. */
(function () {
"use strict";
const API = "/api/v1", TK = "fl_token", UK = "fl_user";
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
  if (r.status === 401 && t && !opt.keepSession) { clearSession(); }
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
  if (h >= 1) return h + (h === 1 ? " hour" : " hours") + (h < 10 && m >= 10 ? " " + m + " min" : "");
  return Math.max(1, m) + " min";
}
function since(iso) { return dur((Date.now() - new Date(iso).getTime()) / 1000); }
const dayFmt = new Intl.DateTimeFormat("en-AU", { weekday: "long" });
const dateFmt = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short" });
const fullDate = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", year: "numeric" });
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
function avatar(a, size, extra) {
  a = a || { kind: "default", color: 0 };
  const st = `width:${size}px;height:${size}px`, x = extra || "";
  if (a.kind === "upload" && a.url_184) {
    const src = size > 64 ? a.url_184 : size > 32 ? a.url_64 : a.url_32;
    return `<span class="av c${(a.color || 0) % 6} ${x}" style="${st}"><img src="${esc(src)}" alt="" loading="lazy"></span>`;
  }
  if (a.kind === "preset") return `<span class="av p c${a.preset % 6} ${x}" style="${st};font-size:${Math.round(size * 0.46)}px" aria-hidden="true">${PRESET_GLYPHS[a.preset % 12]}</span>`;
  return `<span class="av c${(a.color || 0) % 6} ${x}" style="${st}"><img class="mk" src="/assets/mark.svg" alt=""></span>`;
}
const artUrl = appid => appid ? `/api/v1/art/${encodeURIComponent(appid)}.jpg` : null;
function gameArt(appid, id, name, cls) {
  const u = artUrl(appid);
  if (u) return `<img class="${cls || ""}" src="${u}" alt="" loading="lazy" onerror="this.style.visibility='hidden'">`;
  return icon(id, name, cls);
}
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
      <div class="who-menu"><button type="button" aria-haspopup="true">${avatar(u.avatar, 20, "mini-av")}${esc(u.handle)} ▾</button>
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
    [fl, act] = await Promise.all([api("/friends").catch(() => null), api("/friends/activity").catch(() => null)]);
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
  } else if (top && top.steam_appid) {
    $("#hero").style.background = `#000 url(${artUrl(top.steam_appid)}) right center/cover`;
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
      ${g.steam_appid ? `<img src="${artUrl(g.steam_appid)}" alt="" loading="lazy" onerror="this.replaceWith(Object.assign(document.createElement('div'),{className:'noart'}))">` : `<div class="noart">${icon(g.game_id, g.name)}</div>`}
      <span class="rk">#${i + 1}</span><div class="cap">${esc(g.name)}<br><b>${hrsShort(g.minutes)} h</b> <span class="pl">· ${plural(g.players, "player")}</span></div></div>`).join("")}</div>`) : "";
  const feed = h.now_playing.length ? panel("Now playing", `<div class="feed2">${h.now_playing.map(n => `<div>${avatar(n.avatar, 24)}<span><a href="/u/${encodeURIComponent(n.handle)}">${esc(n.display_name || n.handle)}</a><br>${icon(n.game_id, n.game_name)} ${esc(n.game_name)}</span>${n.since ? `<small>${since(n.since)}</small>` : ""}</div>`).join("")}</div>`) : "";
  const lb = h.top_players.length ? panel("Top players this week", `<div class="lb">${h.top_players.map((p, i) => `<div><span class="n">${i + 1}</span>${avatar(p.avatar, 24)}<span class="who"><a href="/u/${encodeURIComponent(p.handle)}">${esc(p.display_name || p.handle)}</a>${p.top_game ? `<small>${esc(p.top_game)}</small>` : ""}</span><span class="h">${hrsShort(p.minutes)} h</span></div>`).join("")}</div>`) : "";
  const mem = h.newest.length ? panel("Newest members", `<div class="mem">${h.newest.map(m => `<div><a href="/u/${encodeURIComponent(m.handle)}">${avatar(m.avatar, 40)}${esc(m.display_name || m.handle)}</a></div>`).join("")}</div>`) : "";
  const news = h.news.length ? panel("News", `<div class="nws">${h.news.map(n => `<div><span class="ic">F</span><div><small>${fullDate.format(new Date(n.at))}</small><p style="color:var(--text)">${esc(n.message)}${n.link ? ` <a href="${esc(n.link)}"${n.link.startsWith("http") ? ' rel="noopener"' : ""}>More</a>` : ""}</p></div></div>`).join("")}</div>`) : "";
  let friendsBox = "", actBox = "";
  if (fl) {
    const on = fl.friends.filter(f => f.presence && f.presence.status !== "offline").sort((a, b) => rank(b.presence) - rank(a.presence));
    friendsBox = panel(`Friends online (${on.length})`, on.length ? `<div class="feed2">${on.slice(0, 10).map(f => `<div>${avatar(f.avatar, 24)}<span><a href="/u/${encodeURIComponent(f.handle)}">${esc(f.display_name || f.handle)}</a><br>${f.presence.game_name ? (isUnlisted(f.presence.game_id) ? "Playing a game" : `${icon(f.presence.game_id, f.presence.game_name)} ${esc(f.presence.game_name)}`) : `<span style="color:var(--online)">Online</span>`}</span></div>`).join("")}</div>` : `<span class="empty">${fl.friends.length ? "None of your friends are online." : `No friends yet. <a href="/friends">Add someone</a>.`}</span>`, ` <a class="more" href="/friends">All friends</a>`);
  }
  if (act && act.items.length) {
    let f = "", last = "";
    act.items.slice(0, 12).forEach(i => { const d = dayLabel(i.at); if (d !== last) { f += `<div class="day">${d}</div>`; last = d; }
      const g = isUnlisted(i.game_id) ? "a game" : `<b>${esc(i.game_name)}</b>`;
      f += `<div class="act">${icon(i.game_id, i.game_name)}<span><a href="/u/${encodeURIComponent(i.handle)}">${esc(i.display_name || i.handle)}</a> ${i.kind === "playing" ? `is playing ${g}` : `played ${g}`}</span>${i.duration_s ? `<span class="tm">${dur(i.duration_s)}</span>` : ""}</div>`; });
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
    const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/signup", { method: "POST", body: { email: f.email.value.trim(), handle: f.handle.value.trim(), password: f.password.value, turnstile: turnstileToken(f) } });
      setSession(r.token, r.user);
      location.href = "/u/" + encodeURIComponent(r.user.handle) + "?welcome=1";
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
      showOk(err, r.mail_live ? "If that email is on a Fraglink account, a reset link is on its way. It works for 1 hour." : "Thanks. Reset emails aren't being delivered yet, so please contact support (below) to get back in.");
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
    if (r.verified) { showOk(m, "Thanks, your email is confirmed."); const u = me(); if (u) setSession(token(), Object.assign(u, { email_verified: true })); }
    else showErr(m, new Error("This link was for an email address that's no longer on the account."));
  } catch (x) { showErr(m, x); }
};

/* ----- profile ----- */
pages.profile = async () => {
  const root = $("#profile");
  const handle = decodeURIComponent(location.pathname.replace(/^\/u\/?/, "").replace(/\/$/, ""));
  if (!handle) { location.replace(me() ? "/u/" + encodeURIComponent(me().handle) : "/login"); return; }
  document.title = handle + " · Fraglink";
  let p;
  try { p = await api("/users/" + encodeURIComponent(handle) + "/profile", { keepSession: true }); }
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
  const pres = p.presence;
  const ids = [].concat((p.games || []).map(g => g.game_id), (p.recent || []).map(r => r.game_id), pres && pres.game_id ? [pres.game_id] : []);
  await loadIcons(ids);
  document.title = (p.display_name || p.handle) + " · Fraglink";
  const self = p.relation === "self";
  const live = pres && pres.status !== "offline" && pres.game_name;
  const listedGames = (p.games || []).filter(g => g.listed !== false);
  const main = listedGames[0];
  const bannerApp = (live && (p.games || []).find(g => g.game_id === pres.game_id && g.steam_appid)) || listedGames.find(g => g.steam_appid);
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
  const badges = (p.linked || []).length ? `<div class="badges">${p.linked.map(l => `<span>${l.provider === "steam" ? "Steam" : l.provider === "discord" ? "Discord" : esc(l.provider)} linked${l.display ? ": " + esc(l.display) : ""}</span>`).join("")}</div>` : "";
  const np = live ? `<div class="np">${icon(pres.game_id, pres.game_name)}<div><div style="font-size:10px;letter-spacing:1px;text-transform:uppercase;opacity:.8">Now playing</div>
      <b>${isUnlisted(pres.game_id) ? "A game" : esc(pres.game_name)}</b>${pres.game_detail ? ` <span style="opacity:.8">· ${esc(pres.game_detail)}</span>` : ""}</div>${pres.session_started_at ? `<span style="opacity:.8">for ${since(pres.session_started_at)}</span>` : ""}</div>` : "";
  const report = !self && token() && p.avatar && p.avatar.kind === "upload" ? `<a href="#" class="report" data-report>Report picture</a>` : "";
  const kpi = `<div class="kpi">${p.history_visible ? `<div><b>${total > 0 && total < 60 ? "&lt;1" : nf.format(Math.round(total / 60))}</b><span>Total hours</span></div><div><b>${hrsShort(p.overall.minutes_7d)}</b><span>Past 7 days</span></div>` : ""}<div><b>${p.friend_count}</b><span>Friends</span></div></div>`;
  const head = `<section class="ph${bannerApp ? "" : " noart"}"${bannerApp ? ` style="background-image:url(${artUrl(bannerApp.steam_appid)})"` : ""}><div class="inr">
      <div>${avatar(p.avatar, 184)}${report}</div>
      <div class="who"><h1>${esc(p.display_name || p.handle)}${p.display_name && p.display_name !== p.handle ? ` <small style="font-size:15px;color:#c8d4e0">${esc(p.handle)}</small>` : ""}</h1>
      <div class="sub">${sub}</div>${badges}${np}<div class="acts">${actions}</div></div>${kpi}</div></section>`;

  const welcome = qs("welcome") && self ? `<div class="alert ok" style="margin-bottom:12px">Welcome to Fraglink. <a href="/download">Download the app</a> and sign in with the same account to start tracking. You can pick a picture in <a href="/settings">Settings</a>.</div>` : "";
  const pendingDelete = self && p.delete_after ? `<div class="alert err" style="margin-bottom:12px">This account will be deleted on ${fullDate.format(new Date(p.delete_after))}. Sign in again to keep it.</div>` : "";

  const details = `<section class="panel"><h2>Details</h2><div class="body details"><div><dl class="kv">
      <dt>Username</dt><dd>${esc(p.handle)}</dd>
      ${p.display_name && p.display_name !== p.handle ? `<dt>Display name</dt><dd>${esc(p.display_name)}</dd>` : ""}
      ${main ? `<dt>Main game</dt><dd>${esc(main.name)}</dd>` : ""}
      ${p.gaming_style ? `<dt>Gaming style</dt><dd>${esc(p.gaming_style)}</dd>` : ""}
      ${p.location ? `<dt>Location</dt><dd>${esc(p.location)}</dd>` : ""}
      <dt>Status</dt><dd>${pres ? statusText(pres) + (live ? " · " + presenceLine(pres) : "") : "Only friends can see this"}</dd>
    </dl>${p.about ? `<p style="margin:10px 0 0;white-space:pre-line">${esc(p.about)}</p>` : ""}</div></div></section>`;

  let games;
  if (!p.history_visible) {
    const why = p.relation === "friend" || p.relation === "public" ? `${esc(p.handle)} keeps their play history private.` : `Add ${esc(p.handle)} as a friend to see their games and hours.`;
    games = `<section class="panel"><h2>Games</h2><div class="body empty">${why}</div></section>`;
  } else if (!p.games || !p.games.length) {
    games = `<section class="panel"><h2>Games</h2><div class="body empty">No hours recorded yet.${self ? ` <a href="/download">Get the app</a> and play something.` : ""}</div></section>`;
  } else {
    const row = g => `<tr><td>${g.steam_appid ? `<img src="${artUrl(g.steam_appid)}" alt="" loading="lazy" onerror="this.style.visibility='hidden'">` : `<span style="display:inline-block;width:64px;margin-right:8px;text-align:center">${icon(g.game_id, g.name)}</span>`}${g.listed === false && !self ? `<span class="empty">${esc(g.name)}</span>` : esc(g.name)}${g.tier && g.tier !== "tracked" ? ` <span class="tier">${esc(g.tier)}</span>` : ""}</td>
      <td class="n">${hours(g.minutes_7d)}</td><td class="n">${hours(g.total_min)}</td><td class="n hide-sm">${lastPlayed(g.last_played_at)}</td></tr>`;
    const top = p.games.slice(0, 8), rest = p.games.slice(8);
    games = `<section class="panel"><h2>Games</h2><table class="gt" style="width:100%;border-collapse:collapse"><thead><tr><th>Game</th><th class="n">Past 7 days</th><th class="n">Total ▾</th><th class="n hide-sm">Last played</th></tr></thead>
      <tbody>${top.map(row).join("")}</tbody>${rest.length ? `<tbody class="more" hidden>${rest.map(row).join("")}</tbody>` : ""}
      <tfoot><tr class="tot"><td>Overall hours</td><td class="n">${hours(p.overall.minutes_7d)}</td><td class="n">${hours(p.overall.total_min)}</td><td class="hide-sm"></td></tr></tfoot></table>
      ${rest.length ? `<div class="body" style="padding-top:6px"><a href="#" data-more>Show entire gaming history (${rest.length} more)</a></div>` : ""}
      <div class="note">Hours counted by the Fraglink app while you play, plus imported game counters. Estimates are never added to totals.</div></section>`;
  }

  // Mini activity report: per day, per game (total time and sessions)
  let act = "";
  if (p.history_visible) {
    const days = new Map();
    (p.recent || []).forEach(r => {
      const d = new Date(r.ended_at), k = d.toDateString();
      if (!days.has(k)) days.set(k, { at: d, games: new Map() });
      const g = days.get(k).games, gk = r.game_id + "|" + r.game_name;
      const cur = g.get(gk) || { id: r.game_id, name: r.game_name, secs: 0, n: 0 };
      cur.secs += r.duration_s || 0; cur.n += 1; g.set(gk, cur);
    });
    if (live) act += `<div class="day">Now</div><div class="act">${icon(pres.game_id, pres.game_name)}<span>${isUnlisted(pres.game_id) ? "Playing a game" : `Playing <b>${esc(pres.game_name)}</b>`}</span>${pres.session_started_at ? `<span class="tm">${since(pres.session_started_at)}</span>` : ""}</div>`;
    Array.from(days.values()).slice(0, 7).forEach(d => {
      act += `<div class="day">${dayLabel(d.at)}</div>`;
      Array.from(d.games.values()).sort((a, b) => b.secs - a.secs).forEach(g => {
        act += `<div class="act">${icon(g.id, g.name)}<span>${isUnlisted(g.id) ? "A game" : `<b>${esc(g.name)}</b>`}${g.n > 1 ? ` <span style="color:var(--dim)">· ${g.n} sessions</span>` : ""}</span><span class="tm">${dur(g.secs)}</span></div>`;
      });
    });
  }
  const mini = `<section class="panel"><h2>Mini activity report</h2><div class="body" style="padding-top:2px">${act || `<span class="empty">${p.history_visible ? "Nothing in the last two weeks." : "Activity is visible to friends when shared."}</span>`}</div></section>`;

  const fr = Array.isArray(p.friends) ? `<section class="panel"><h2>Friends (${p.friend_count})${self ? ` <a class="more" href="/friends">See all</a>` : ""}</h2><div class="body">${p.friends.length ? `<div class="fg">${p.friends.map(f => `<div><a href="/u/${encodeURIComponent(f.handle)}">${avatar(f.avatar, 56)}${esc(f.display_name || f.handle)}</a><div class="st ${f.game_name ? "pl" : f.status === "online" ? "on" : ""}">${f.game_name ? esc(f.game_name) : f.status === "online" ? "Online" : f.status === "away" ? "Away" : "Offline"}</div></div>`).join("")}</div>` : `<span class="empty">No friends yet.</span>`}</div></section>` : "";
  const RIG = [["manufacturer", "Manufacturer"], ["cpu", "Processor"], ["gpu", "Video card"], ["memory", "Memory"], ["storage", "Storage"], ["motherboard", "Motherboard"], ["monitor", "Monitor"], ["headset", "Headset"], ["keyboard", "Keyboard"], ["mouse", "Mouse"], ["mousepad", "Mouse pad"], ["case", "Case"], ["os", "Operating system"]];
  const rig = p.rig ? `<section class="panel"><h2>Gaming rig${self ? ` <a class="more" href="/settings#rig">Edit</a>` : ""}</h2><div class="body"><dl class="rig">${RIG.filter(([k]) => p.rig[k]).map(([k, l]) => `<dt>${l}</dt><dd>${esc(p.rig[k])}</dd>`).join("")}</dl></div></section>`
    : self ? `<section class="panel"><h2>Gaming rig</h2><div class="body empty">Show off your setup. <a href="/settings#rig">Add your gaming rig</a>.</div></section>` : "";
  const shareUrl = location.origin + "/u/" + encodeURIComponent(p.handle);
  const share = self ? `<section class="panel"><h2>Share your profile</h2><div class="body">${p.public_profile
      ? `<div class="share"><input class="t" readonly value="${esc(shareUrl)}"><button class="btn sm" data-copy="${esc(shareUrl)}">Copy</button></div>
         <p style="margin:10px 0 4px;color:var(--muted)">Signature image (updates with what you're playing):</p><img class="sigimg" src="/api/v1/sig/${encodeURIComponent(p.handle)}.png" alt="Signature" width="460" height="100">
         <div class="share" style="margin-top:6px"><input class="t" readonly value="${esc("https://playfraglink.com/api/v1/sig/" + encodeURIComponent(p.handle) + ".png")}"><button class="btn sm" data-copy="https://playfraglink.com/api/v1/sig/${esc(encodeURIComponent(p.handle))}.png">Copy</button></div>`
      : `<span class="empty">Your profile is visible to signed-in players only. Turn on <a href="/settings#privacy">Public profile</a> to get a shareable link and a signature image.</span>`}</div></section>` : "";
  const discord = `<section class="panel"><h2>Discord server</h2><div class="body discord-box"><div class="dl"><img src="/assets/discord-symbol-white.svg" alt=""></div><div><a href="${DISCORD}" rel="noopener">Fraglink on Discord</a><small>Chat, feedback and release news</small></div></div></section>`;

  root.innerHTML = head + pendingDelete + welcome + `<div class="g2v"><div class="col">${details}${games}${mini}</div><aside class="col">${fr}${rig}${share}${discord}</aside></div>`;

  const add = $("[data-add]", root), acc = $("[data-accept]", root), more = $("[data-more]", root), rep = $("[data-report]", root);
  if (add) add.addEventListener("click", async () => { if (!token()) return requireLogin(); add.disabled = true; try { await api("/friends/request", { method: "POST", body: { handle: p.handle } }); add.textContent = "Friend request sent"; } catch (e) { add.disabled = false; alert(e.message); } });
  if (acc) acc.addEventListener("click", async () => { acc.disabled = true; try { await api("/friends/respond", { method: "POST", body: { handle: p.handle, accept: true } }); location.reload(); } catch (e) { acc.disabled = false; alert(e.message); } });
  if (more) more.addEventListener("click", e => { e.preventDefault(); $("tbody.more", root).hidden = false; more.parentNode.remove(); });
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
  async function load() {
    let fl, act;
    try { [fl, act] = await Promise.all([api("/friends"), api("/friends/activity").catch(() => ({ items: [] }))]); }
    catch (e) { if (!token()) return requireLogin(); showErr(msg, e); return; }
    await loadIcons([].concat(fl.friends.map(f => f.presence && f.presence.game_id), act.items.map(i => i.game_id)));
    const sorted = fl.friends.slice().sort((a, b) => rank(b.presence) - rank(a.presence) || a.handle.localeCompare(b.handle));
    const on = sorted.filter(f => rank(f.presence) > 0), off = sorted.filter(f => rank(f.presence) === 0);
    const btns = `<button class="btn sm" data-act="remove">Remove</button>`;
    const reqIn = fl.incoming.map(r => `<div class="fr" data-h="${esc(r.handle)}"><div class="fa ${tint(r.handle)}">${initial(r.display_name || r.handle)}</div><div class="who"><div class="nm"><a href="/u/${encodeURIComponent(r.handle)}">${esc(r.display_name || r.handle)}</a></div><div class="gm">wants to be friends</div></div>
      <div class="acts"><button class="btn sm dl" data-act="accept">Accept</button><button class="btn sm" data-act="decline">Decline</button><button class="btn sm" data-act="block">Block</button></div></div>`).join("");
    const reqOut = fl.outgoing.map(r => `<div class="fr" data-h="${esc(r.handle)}"><div class="fa ${tint(r.handle)}">${initial(r.handle)}</div><div class="who"><div class="nm"><a href="/u/${encodeURIComponent(r.handle)}">${esc(r.display_name || r.handle)}</a></div><div class="gm">request sent ${dateFmt.format(new Date(r.created_at))}</div></div>
      <div class="acts"><button class="btn sm" data-act="cancel">Cancel</button></div></div>`).join("");
    let feed = "", last = "";
    act.items.slice(0, 20).forEach(i => {
      const d = dayLabel(i.at); if (d !== last) { feed += `<div class="day">${d}</div>`; last = d; }
      const g = isUnlisted(i.game_id) ? "a game" : `<b>${esc(i.game_name)}</b>`;
      feed += `<div class="ev">${icon(i.game_id, i.game_name)}<p><a href="/u/${encodeURIComponent(i.handle)}">${esc(i.display_name || i.handle)}</a> ${i.kind === "playing" ? `is playing ${g}` : `played ${g} for ${dur(i.duration_s)}`}</p></div>`;
    });
    root.innerHTML = `<div class="grid"><div class="col">
      ${fl.incoming.length ? `<section class="panel"><h2>Friend requests (${fl.incoming.length})</h2><div>${reqIn}</div></section>` : ""}
      <section class="panel"><h2>Online (${on.length})</h2><div>${on.length ? on.map(f => friendRow(f, { actions: btns })).join("") : `<div class="body empty">Nobody's online right now.</div>`}</div></section>
      <section class="panel"><h2>Offline (${off.length})</h2><div>${off.length ? off.map(f => friendRow(f, { actions: btns })).join("") : `<div class="body empty">${fl.friends.length ? "Everyone's online." : "No friends yet. Add someone by their Fraglink username."}</div>`}</div></section>
      ${fl.outgoing.length ? `<section class="panel"><h2>Sent requests (${fl.outgoing.length})</h2><div>${reqOut}</div></section>` : ""}
    </div><aside class="col">
      <section class="panel"><h2>Add a friend</h2><div class="body"><form class="f" id="addf"><label>Username<input type="text" name="handle" maxlength="20" required placeholder="their Fraglink username" autocomplete="off"></label><div class="row"><button class="btn">Send request</button></div></form></div></section>
      <section class="panel"><h2>Friends' activity</h2><div>${feed || `<div class="body empty">Nothing in the last 7 days.</div>`}</div></section>
      ${fl.blocked.length ? `<section class="panel"><h2>Blocked</h2><div>${fl.blocked.map(h => `<div class="fr" data-h="${esc(h)}"><div class="who">${esc(h)}</div><div class="acts"><button class="btn sm" data-act="unblock">Unblock</button></div></div>`).join("")}</div></section>` : ""}
    </aside></div>`;
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
      ${row("Username", `<b>${esc(u.handle)}</b>`, "Friends add you by this name. It can't be changed yet.")}
      ${row("Display name", `<input class="t" name="display_name" maxlength="32" value="${esc(u.display_name || "")}">`, "Shown on your profile and to friends. Leave empty to use your username.")}
      ${row("Email", `${esc(u.email || "")} ${verified}`)}
      ${row("", `<button class="btn dl">Save</button>`)}</div></form>
    <h3>About you</h3><form data-f="about">${msg}<div class="frm">
      ${row("About me", `<textarea class="t" name="about" maxlength="300">${esc(u.about || "")}</textarea>`, "Up to 300 characters.")}
      ${row("Location", `<input class="t" name="location" maxlength="40" value="${esc(u.location || "")}" placeholder="e.g. Sydney, Australia">`, "Optional. Shown on your profile as typed.")}
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
    el.innerHTML = `<h3>Played games</h3>${msg}<div class="empty-v2">Loading…</div>`;
    let g; try { g = await api("/me/games"); } catch (e) { bad(el, e); return; }
    await loadIcons(g.games.map(x => x.game_id));
    el.innerHTML = `<h3>Played games</h3>${msg}<p class="empty-v2" style="margin:0">Hidden games stay in your own totals but disappear from your profile, signature and the home page. Resetting deletes every hour recorded for that game.</p>
      ${g.games.length ? `<table class="pg"><thead><tr><th>Game</th><th class="n">Past 7 days</th><th class="n">Total</th><th class="n">Last played</th><th></th></tr></thead><tbody>${g.games.map(x => `<tr data-g="${esc(x.game_id)}" class="${x.hidden ? "hid" : ""}"><td>${icon(x.game_id, x.name)} ${esc(x.name)}</td><td class="n">${hours(x.minutes_7d)}</td><td class="n">${hours(x.total_min)}</td><td class="n">${lastPlayed(x.last_played_at)}</td>
        <td class="n" style="white-space:nowrap"><label style="margin-right:8px"><input type="checkbox" data-hide ${x.hidden ? "checked" : ""}> Hide</label><button class="btn sm" type="button" data-reset>Reset hours</button></td></tr>`).join("")}</tbody></table>` : `<div class="empty-v2">No games yet.</div>`}`;
    el.addEventListener("change", async e => { const c = e.target.closest("[data-hide]"); if (!c) return; const tr = c.closest("tr");
      try { await api("/me/games/hide", { method: "POST", body: { game_id: tr.dataset.g, hidden: c.checked } }); tr.classList.toggle("hid", c.checked); } catch (x) { c.checked = !c.checked; bad(el, x); } });
    el.addEventListener("click", async e => { const b = e.target.closest("[data-reset]"); if (!b) return; const tr = b.closest("tr");
      const pw = prompt(`Delete every hour recorded for ${tr.cells[0].textContent.trim()}? This can't be undone.\n\nEnter your password to confirm:`); if (!pw) return;
      try { await api("/me/games/reset", { method: "POST", body: { game_id: tr.dataset.g, password: pw } }); tr.remove(); ok(el, "Hours reset."); } catch (x) { bad(el, x); } });
  }
  function privacyTab() {
    const share = location.origin + "/u/" + encodeURIComponent(u.handle);
    return `<h3>Privacy</h3><form data-f="privacy">${msg}<div class="frm">
      ${row("Play history", `<label><input type="checkbox" name="share_activity" ${u.share_activity ? "checked" : ""}>Share my games, hours and recent activity with friends</label>`, "Live status follows the app: pick Invisible there to hide it.")}
      ${row("Public profile", `<label><input type="checkbox" name="public_profile" ${u.public_profile ? "checked" : ""}>Anyone with the link can see my profile</label>`, "Also turns on your signature image, and you may appear on the public home page (Now playing, Top players, Newest members). Off: only signed-in players can open your profile.")}
      ${row("Friends list", `<label><input type="checkbox" name="show_friends" ${u.show_friends ? "checked" : ""}>Show my friends on my profile</label>`)}
      ${row("Your link", `<div class="share"><input class="t" readonly value="${esc(share)}"><button class="btn sm" type="button" data-copy="${esc(share)}">Copy</button></div>`)}
      ${u.public_profile ? row("Signature image", `<img class="sigimg" src="/api/v1/sig/${encodeURIComponent(u.handle)}.png?t=${Date.now()}" alt="" width="460" height="100"><div class="share" style="margin-top:6px"><input class="t" readonly value="https://playfraglink.com/api/v1/sig/${esc(encodeURIComponent(u.handle))}.png"></div>`, "Paste the image link into forum signatures. It refreshes about once a minute.") : ""}
      </div></form>`;
  }
  function notificationsTab() {
    const n = u.notify_prefs || {};
    const cb = (k, t) => `<label style="display:block;margin:2px 0"><input type="checkbox" name="${k}" ${n[k] ? "checked" : ""}>${t}</label>`;
    return `<h3>Notifications</h3><form data-f="notify">${msg}<div class="frm">
      ${row("Email", cb("signin_email", "New sign-in to my account") + cb("friend_request_email", "Someone sends me a friend request") + cb("news_email", "Fraglink news (a few times a year)"), u.mail_live ? "" : "Email delivery isn't switched on yet. Your choices are saved for when it is.")}
      ${row("Desktop app", cb("friend_request_client", "Friend requests") + cb("friend_online_client", "A friend comes online or starts a game"), "The app's own pop-up settings still apply.")}
      </div></form>`;
  }
  function connectionsTab() {
    const steam = (m.linked_accounts || []).find(l => l.provider === "steam");
    return `<h3>Connections</h3>${msg}<div style="padding:0 14px">
      <div class="conn"><b style="width:90px">Steam</b><span>${steam ? `Linked${steam.verified ? " (verified)" : ""}${steam.last_sync_at ? " · last sync " + fullDate.format(new Date(steam.last_sync_at)) : ""}` : `<span class="empty">Not linked</span>`}</span><span class="btn" aria-disabled="true" style="margin-left:auto">${steam ? "Linked" : "Link from the app"}</span></div>
      <div class="conn"><b style="width:90px">Discord</b><span class="empty">Linking Discord is coming soon.</span><span class="btn" aria-disabled="true" style="margin-left:auto">Soon</span></div>
      <p class="sub" style="padding:8px 0">Linked accounts show as badges on your profile when they're verified.</p></div>`;
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
      <div style="padding:10px 14px"><button class="btn red" type="button" data-revoke-all>Log out of all other devices</button></div>`;
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
    else if (t === "connections") el.innerHTML = connectionsTab();
    else if (t === "security") securityTab(el);
  }
  async function refresh() { u = (await api("/me")).user; keep(); renderHeader(); }
  window.addEventListener("hashchange", render);
  root.addEventListener("submit", async e => {
    const f = e.target.closest("form[data-f]"); if (!f) return; e.preventDefault();
    const k = f.dataset.f, v = n => f.elements[n] ? f.elements[n].value : undefined;
    try {
      if (k === "name") { const r = await api("/me/display_name", { method: "POST", body: { display_name: v("display_name") } }); u.display_name = r.display_name; keep(); renderHeader(); ok(f, "Saved."); }
      else if (k === "about" || k === "rig") {
        const rig = Object.assign({}, u.rig || {}); if (k === "rig") RIG_FIELDS.forEach(([n]) => { rig[n] = v(n); });
        const body = { about: k === "about" ? v("about") : u.about, location: k === "about" ? v("location") : u.location, gaming_style: (k === "about" ? v("gaming_style") : u.gaming_style) || null, rig };
        const r = await api("/me/profile", { method: "POST", body }); Object.assign(u, r); ok(f, "Saved.");
      }
      else if (k === "email") { await api("/me/email", { method: "POST", body: { current_password: v("current_password"), new_email: v("new_email").trim() } }); await refresh(); render(); ok($("#pane"), "Email changed."); }
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
      const file = e.target.files[0]; if (!file) return; const pane = $("#pane");
      if (file.size > 2 * 1024 * 1024) return bad(pane, new Error("Images can be up to 2 MB."));
      try {
        const r = await fetch(API + "/me/avatar", { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": "application/octet-stream" }, body: file });
        const j = await r.json().catch(() => null); if (!r.ok) throw new Error((j && j.message) || "Upload failed (HTTP " + r.status + ").");
        await refresh(); render(); ok($("#pane"), "Picture updated.");
      } catch (x) { bad(pane, x); }
      return;
    }
    if (!f) return;
    try {
      if (f.dataset.f === "privacy") { const r = await api("/me/privacy", { method: "POST", body: { [e.target.name]: e.target.checked } }); Object.assign(u, r); keep(); render(); ok($("#pane"), "Saved."); }
      else if (f.dataset.f === "notify") { u.notify_prefs = await api("/me/notifications", { method: "POST", body: { [e.target.name]: e.target.checked } }); ok(f, "Saved."); }
    } catch (x) { e.target.checked = !e.target.checked; bad(f, x); }
  });
  root.addEventListener("click", async e => {
    const b = e.target.closest("button,a[data-resend]"); if (!b) return; const pane = $("#pane");
    try {
      if (b.matches("[data-upload]")) $("#av-file").click();
      else if (b.matches("[data-avreset]")) { await api("/me/avatar/reset", { method: "POST" }); await refresh(); render(); ok($("#pane"), "Back to the default picture."); }
      else if (b.matches("[data-preset]")) { await api("/me/avatar/preset", { method: "POST", body: { preset: Number(b.dataset.preset) } }); await refresh(); render(); ok($("#pane"), "Picture updated."); }
      else if (b.matches("[data-resend]")) { e.preventDefault(); if (!u.mail_live) return; await api("/me/verify/resend", { method: "POST" }); ok(pane, "Sent. Check your inbox."); }
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
        $("[data-mfa]").innerHTML = `${msg}<div class="frm">${row("Status", `<b style="color:var(--online)">On</b>`)}${row("Backup codes", `<div class="codes">${r.backup_codes.map(c => `<span>${esc(c)}</span>`).join("")}</div>`, "Save these somewhere safe. Each one signs you in once if you lose your phone. They won't be shown again.")}${row("", `<button class="btn" type="button" onclick="location.reload()">Done</button>`)}</div>`;
      }
      else if (b.matches("[data-mfa-off]") || b.matches("[data-mfa-codes]")) {
        const body = { password: $('[data-mfa] [name="password"]').value, code: $('[data-mfa] [name="code"]').value.trim() };
        if (b.matches("[data-mfa-off]")) { await api("/me/mfa/disable", { method: "POST", body }); await refresh(); render(); ok($("#pane"), "Two-step sign-in is off."); }
        else { const r = await api("/me/mfa/backup_codes", { method: "POST", body }); $("[data-mfa-step]").innerHTML = `<div class="frm">${row("New backup codes", `<div class="codes">${r.backup_codes.map(c => `<span>${esc(c)}</span>`).join("")}</div>`, "The old ones no longer work.")}</div>`; }
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
    if (e.target.matches("[data-vb-send]")) { try { await api("/me/verify/resend", { method: "POST" }); e.target.textContent = "Sent"; } catch (x) { e.target.textContent = x.message; } }
  });
  ($(".preview-flag") || $(".top")).after(d);
}

/* ---------- site banner (staff announcements) ---------- */
async function banner() {
  try {
    const r = await fetch(API + "/announcement");
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
  } catch (e) { /* no banner */ }
}

/* ---------- admin (staff only; the page itself is a 404 for everyone else) ---------- */
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

/* ---------- boot ---------- */
document.addEventListener("DOMContentLoaded", () => {
  renderHeader();
  const y = $("#year"); if (y) y.textContent = new Date().getFullYear();
  banner();
  const f = pages[document.body.dataset.page];
  if (f) Promise.resolve(f()).catch(e => { console.error(e); });
  // keep the header in sync with the real account (handle/display name changes)
  if (token() && document.body.dataset.page !== "settings") api("/me").then(m => { setSession(token(), { id: m.user.id, handle: m.user.handle, display_name: m.user.display_name, email: m.user.email, staff_role: m.user.staff_role || null, avatar: m.user.avatar, email_verified: m.user.email_verified, mail_live: m.user.mail_live }); renderHeader(); verifyBar(); }).catch(() => { if (!token()) renderHeader(); });
  else verifyBar();
  if (qs("deleted")) { const d = new Date(qs("deleted")); const el = document.createElement("div"); el.className = "verify-bar delete-bar"; el.innerHTML = `<div class="wrap">Your account is scheduled for deletion${isNaN(d) ? "" : " on " + fullDate.format(d)}. Sign in again before then to keep it.</div>`; ($(".preview-flag") || $(".top")).after(el); }
});
})();
