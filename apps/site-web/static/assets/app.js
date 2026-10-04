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
        if (v && v.icon_path) src = v.icon_path.replace(/^\/icons\/[a-z]+\//, "/icons/g/");
        else if (v && v.icon_url) src = v.icon_url;
        iconCache[id] = src;
      });
    } catch (e) { chunk.forEach(id => { iconCache[id] = null; }); }
  }
}
function icon(id, name, cls) {
  const src = iconCache[id];
  if (src) return `<img class="${cls || ""}" src="${esc(src)}" alt="" loading="lazy" width="20" height="20">`;
  return `<span class="gi ${cls || ""}">${isUnlisted(id) ? "?" : initial(name)}</span>`;
}

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
      <div class="who-menu"><button type="button" aria-haspopup="true"><span class="mini-av">${initial(u.display_name || u.handle)}</span>${esc(u.handle)} ▾</button>
      <div class="menu" role="menu"><a href="/u/${encodeURIComponent(u.handle)}">My profile</a><a href="/friends">Friends</a><a href="/settings">Account settings</a><hr><button type="button" data-logout>Sign out</button></div></div>`;
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

pages.home = () => {
  fillDownload();
  const u = me();
  if (u) { const c = $("#home-signed"); if (c) { c.hidden = false; $("a", c).href = "/u/" + encodeURIComponent(u.handle); } $$("[data-signedout]").forEach(e => e.hidden = true); }
};
pages.download = () => fillDownload();

pages.login = () => {
  if (token() && me()) { location.replace(safeNext(qs("next")) || "/u/" + encodeURIComponent(me().handle)); return; }
  const f = $("#login"), err = $("#err");
  if (qs("signedout")) showOk(err, "You're signed out.");
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true; const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/login", { method: "POST", body: { email: f.login.value.trim(), password: f.password.value } });
      setSession(r.token, r.user);
      location.href = safeNext(qs("next")) || "/u/" + encodeURIComponent(r.user.handle);
    } catch (x) { showErr(err, x); b.disabled = false; }
  });
};

pages.signup = () => {
  if (token() && me()) { location.replace("/u/" + encodeURIComponent(me().handle)); return; }
  const f = $("#signup"), err = $("#err");
  f.addEventListener("submit", async e => {
    e.preventDefault(); err.hidden = true;
    if (f.password.value !== f.password2.value) { showErr(err, new Error("The passwords don't match.")); return; }
    if (!f.terms.checked) { showErr(err, new Error("Please accept the Terms and Privacy Policy.")); return; }
    const b = $("button", f); b.disabled = true;
    try {
      const r = await api("/auth/signup", { method: "POST", body: { email: f.email.value.trim(), handle: f.handle.value.trim(), password: f.password.value } });
      setSession(r.token, r.user);
      location.href = "/u/" + encodeURIComponent(r.user.handle) + "?welcome=1";
    } catch (x) { showErr(err, x); b.disabled = false; }
  });
};

pages.forgot = () => {
  const f = $("#forgot");
  f.addEventListener("submit", e => { e.preventDefault(); $("#err").hidden = false; });
};

/* ----- profile ----- */
async function profileData(handle) {
  try { return await api("/users/" + encodeURIComponent(handle) + "/profile"); }
  catch (e) {
    if (e.status !== 404 || e.code === "user_not_found") throw e;
  }
  // Older server without the profile endpoint: build what we can from existing calls.
  const u = me();
  if (u && u.handle.toLowerCase() === handle.toLowerCase()) {
    const [m, st, fl, sm] = await Promise.all([api("/me"), api("/stats"), api("/friends"), api("/friends/" + encodeURIComponent(u.handle) + "/summary").catch(() => null)]);
    const games = (st.games || []).filter(g => g.display_min > 0 || g.minutes_7d > 0).map(g => ({ game_id: g.game_id, name: g.name, minutes_7d: g.minutes_7d, total_min: g.display_min, tier: g.display_tier, last_played_at: g.last_played_at, listed: g.name !== "Unlisted game" }));
    const recent = games.filter(g => g.last_played_at && Date.now() - new Date(g.last_played_at) < 14 * 86400000)
      .sort((a, b) => new Date(b.last_played_at) - new Date(a.last_played_at)).map(g => ({ game_id: g.listed ? g.game_id : "unlisted", game_name: g.name, ended_at: g.last_played_at, duration_s: null }));
    return { handle: m.user.handle, display_name: m.user.display_name, relation: "self", friend_count: fl.friends.length, shares_activity: m.user.share_activity, history_visible: true,
      presence: sm ? sm.presence : null, games, overall: { total_min: games.reduce((a, g) => a + g.total_min, 0), minutes_7d: games.reduce((a, g) => a + g.minutes_7d, 0) }, recent, member_since: null, partial: true };
  }
  const [s, act] = await Promise.all([api("/friends/" + encodeURIComponent(handle) + "/summary"), api("/friends/activity").catch(() => ({ items: [] }))]);
  const recent = (act.items || []).filter(i => i.kind === "played" && i.handle.toLowerCase() === s.handle.toLowerCase())
    .map(i => ({ game_id: i.game_id, game_name: i.game_name, ended_at: i.at, duration_s: i.duration_s }));
  return { handle: s.handle, display_name: s.display_name, relation: "friend", presence: s.presence, shares_activity: s.shares_activity, history_visible: s.shares_activity,
    games: [], overall: { total_min: null, minutes_7d: Math.round((s.last_7d_seconds || 0) / 60) }, recent, top_game_7d: s.top_game_7d, friend_count: null, partial: true };
}

pages.profile = async () => {
  const root = $("#profile");
  const handle = decodeURIComponent(location.pathname.replace(/^\/u\/?/, "").replace(/\/$/, ""));
  if (!handle) { location.replace(me() ? "/u/" + encodeURIComponent(me().handle) : "/login"); return; }
  document.title = handle + " · Fraglink";
  if (!token()) {
    root.innerHTML = `<div class="narrow panel"><h2>${esc(handle)}</h2><div class="body"><p>Sign in to see Fraglink profiles.</p><p><a class="btn" href="/login?next=${encodeURIComponent(location.pathname)}">Sign in</a> <a class="btn dl" href="/signup">Create an account</a></p></div></div>`;
    return;
  }
  let p, fl = null;
  try {
    p = await profileData(handle);
  } catch (e) {
    if (!token()) { requireLogin(); return; }
    root.innerHTML = e.status === 404
      ? `<div class="narrow panel"><h2>Not found</h2><div class="body"><p>There's no Fraglink player called <b>${esc(handle)}</b>.</p></div></div>`
      : `<div class="narrow alert err">${esc(e.message)}</div>`;
    return;
  }
  if (p.relation === "self") { try { fl = await api("/friends"); } catch (e) { fl = null; } }
  const pres = p.presence;
  const ids = [].concat((p.games || []).map(g => g.game_id), (p.recent || []).map(r => r.game_id), pres && pres.game_id ? [pres.game_id] : [],
    fl ? fl.friends.map(f => f.presence && f.presence.game_id) : []);
  await loadIcons(ids);
  document.title = (p.display_name || p.handle) + " · Fraglink";

  const live = pres && pres.status !== "offline" && pres.game_name;
  const st = pres ? pres.status : null;
  const lastGame = (p.recent && p.recent[0]) || null;
  let sub = pres ? `<span class="status"><span class="dot ${esc(st)}"></span>${statusText(pres)}</span>` : "";
  if (!live && lastGame) sub += (sub ? " · " : "") + `last played ${isUnlisted(lastGame.game_id) ? "a game" : esc(lastGame.game_name)}`;
  sub += (sub ? " · " : "") + `<span class="url">playfraglink.com/u/${esc(p.handle)}</span>`;

  let actions = "";
  if (p.relation === "none") actions = `<button class="btn" data-add>+ Add friend</button>`;
  else if (p.relation === "outgoing") actions = `<span class="btn" aria-disabled="true">Friend request sent</span>`;
  else if (p.relation === "incoming") actions = `<button class="btn dl" data-accept>Accept friend request</button>`;
  else if (p.relation === "self") actions = `<a class="btn" href="/settings">Edit account</a>`;
  else if (p.relation === "friend") actions = `<span class="btn" aria-disabled="true">✓ Friends</span>`;

  const np = live ? `<div class="np">${icon(pres.game_id, pres.game_name)}<div><div class="lbl">Now playing</div>
      <div class="g">${isUnlisted(pres.game_id) ? "Playing a game" : esc(pres.game_name)}${pres.game_detail ? ` <span class="d">· ${esc(pres.game_detail)}</span>` : ""}</div></div>
      ${pres.session_started_at ? `<div class="t">for ${since(pres.session_started_at)}</div>` : ""}</div>` : "";

  const welcome = qs("welcome") && p.relation === "self"
    ? `<div class="alert ok" style="margin-bottom:14px">Welcome to Fraglink. <a href="/download">Download the app</a> and sign in with the same account to start tracking.</div>` : "";

  // details
  const total = p.overall ? p.overall.total_min : 0;
  const mainGame = (p.games || []).find(g => g.listed !== false);
  const details = `<section class="panel"><h2>Details</h2><div class="body details"><div><dl class="kv">
      <dt>Username</dt><dd>${esc(p.handle)}</dd>
      ${p.display_name && p.display_name !== p.handle ? `<dt>Display name</dt><dd>${esc(p.display_name)}</dd>` : ""}
      ${mainGame ? `<dt>Main game</dt><dd>${esc(mainGame.name)}</dd>` : p.top_game_7d ? `<dt>Top game (7 days)</dt><dd>${esc(p.top_game_7d)}</dd>` : ""}
      <dt>Status</dt><dd>${pres ? statusText(pres) + (live ? " · " + presenceLine(pres) : "") : "Only friends can see this"}</dd>
    </dl></div><div class="statbox">
      ${p.member_since ? `<div><span>Member since</span><b>${fullDate.format(new Date(p.member_since))}</b></div>` : ""}
      ${p.history_visible && total != null ? `<div><span>Total hours</span><b class="big">${total > 0 && total < 60 ? "&lt;1" : nf.format(Math.round(total / 60))}</b></div>` : ""}
      ${p.history_visible ? `
      <div><span>Past 7 days</span><b>${hours(p.overall.minutes_7d, { zero: true })}</b></div>` : ""}
      ${p.friend_count == null ? "" : `<div><span>Friends</span><b>${p.relation === "self" ? `<a href="/friends">${p.friend_count} friend${p.friend_count === 1 ? "" : "s"}</a>` : p.friend_count + " friend" + (p.friend_count === 1 ? "" : "s")}</b></div>`}
    </div></div></section>`;

  // games
  let games;
  if (!p.history_visible) {
    const why = p.relation === "friend" ? `${esc(p.handle)} keeps their play history private.` : `Add ${esc(p.handle)} as a friend to see their games and hours.`;
    games = `<section class="panel"><h2>Games</h2><div class="body empty">${why}</div></section>`;
  } else if (!p.games || !p.games.length) {
    games = `<section class="panel"><h2>Games</h2><div class="body empty">${p.partial && p.relation === "friend" ? "Game history needs the latest server update." : "No hours recorded yet." + (p.relation === "self" ? ` <a href="/download">Get the app</a> and play something.` : "")}</div></section>`;
  } else {
    const row = g => `<tr><td class="ic">${icon(g.game_id, g.name)}</td><td>${g.listed === false && p.relation !== "self" ? `<span class="empty">${esc(g.name)}</span>` : esc(g.name)}${g.tier && g.tier !== "tracked" ? `<span class="tier" title="${g.tier === "imported" ? "Includes hours imported from the game or Steam" : "Estimate"}">${esc(g.tier)}</span>` : ""}</td>
      <td class="num ${g.minutes_7d ? "" : "dash"}">${hours(g.minutes_7d)}</td><td class="num">${hours(g.total_min)}</td><td class="num hide-sm">${lastPlayed(g.last_played_at)}</td></tr>`;
    const top = p.games.slice(0, 6), rest = p.games.slice(6);
    games = `<section class="panel"><h2>Games</h2><table><thead><tr><th colspan="2">Game</th><th class="num">Past 7 days</th><th class="num">Total ▾</th><th class="num hide-sm">Last played</th></tr></thead>
      <tbody>${top.map(row).join("")}</tbody></table>
      ${rest.length ? `<details class="hist"><summary><span class="s">Show entire gaming history (${rest.length} more)</span><span class="h">Hide full history</span></summary><table><tbody>${rest.map(row).join("")}</tbody></table></details>` : ""}
      <table><tfoot><tr><td colspan="2">Overall hours</td><td class="num">${hours(p.overall.minutes_7d)}</td><td class="num">${hours(p.overall.total_min)}</td><td class="num hide-sm"></td></tr></tfoot></table>
      <div class="note">Hours counted by the Fraglink app while you play, plus imported game counters. Estimates are never added to totals.</div></section>`;
  }

  // mini activity
  const evs = [];
  if (live) evs.push({ at: new Date(), html: `${icon(pres.game_id, pres.game_name)}<p>${isUnlisted(pres.game_id) ? "Playing a game" : `Playing <b>${esc(pres.game_name)}</b>`} now</p>` });
  (p.recent || []).forEach(r => evs.push({ at: new Date(r.ended_at), html: `${icon(r.game_id, r.game_name)}<p>Played ${isUnlisted(r.game_id) ? "a game" : `<b>${esc(r.game_name)}</b>`}${r.duration_s ? " for " + dur(r.duration_s) : ""}</p>` }));
  if (p.member_since && (Date.now() - new Date(p.member_since)) < (p.recent_days || 14) * 86400000)
    evs.push({ at: new Date(p.member_since), html: `<span class="gi r">★</span><p>Joined Fraglink</p>` });
  let act = "";
  if (evs.length) {
    let last = "";
    evs.slice(0, 12).forEach(e => { const d = dayLabel(e.at); if (d !== last) { act += `<div class="day">${d}</div>`; last = d; } act += `<div class="ev">${e.html}</div>`; });
  } else act = `<div class="body empty">${p.history_visible ? "Nothing in the last two weeks." : "Activity is visible to friends who share it."}</div>`;
  const mini = `<section class="panel"><h2>Mini activity report</h2><div>${act}</div></section>`;

  // friends panel (own profile)
  let friends = "";
  if (fl) {
    const list = fl.friends.slice().sort((a, b) => rank(b.presence) - rank(a.presence) || a.handle.localeCompare(b.handle)).slice(0, 6);
    friends = `<section class="panel"><h2>Friends <a class="more" href="/friends">See all (${fl.friends.length})</a></h2><div>${list.length ? list.map(friendRow).join("") : `<div class="body empty">No friends yet. <a href="/friends">Add someone</a>.</div>`}</div></section>`;
  }
  const discord = `<section class="panel"><h2>Discord server</h2><div class="body discord-box"><div class="dl"><img src="/assets/discord-symbol-white.svg" alt=""></div><div><a href="${DISCORD}" rel="noopener">Fraglink on Discord</a><small>Chat, feedback and release news</small></div></div></section>`;

  root.innerHTML = `<section class="head"><div class="avatar">${initial(p.display_name || p.handle)}</div><div>
      <h1>${esc(p.display_name || p.handle)}${p.display_name && p.display_name !== p.handle ? `<small>${esc(p.handle)}</small>` : ""}</h1><div class="sub">${sub}</div></div>
      <div class="actions">${actions}</div></section>${welcome}${np}
    <div class="grid"><div class="col">${details}${games}</div><aside class="col">${mini}${friends}${discord}</aside></div>`;

  const add = $("[data-add]", root), acc = $("[data-accept]", root);
  if (add) add.addEventListener("click", async () => { add.disabled = true; try { await api("/friends/request", { method: "POST", body: { handle: p.handle } }); add.textContent = "Friend request sent"; } catch (e) { add.disabled = false; alert(e.message); } });
  if (acc) acc.addEventListener("click", async () => { acc.disabled = true; try { await api("/friends/respond", { method: "POST", body: { handle: p.handle, accept: true } }); location.reload(); } catch (e) { acc.disabled = false; alert(e.message); } });
};
function rank(p) { return !p ? 0 : p.status === "online" ? (p.game_name ? 3 : 2) : p.status === "away" ? 1 : 0; }
function friendRow(f, opts) {
  const p = f.presence || {};
  const st = p.status === "online" ? (p.game_name ? "● playing" : "● online") : p.status === "away" ? "away" : "offline";
  const game = p.status !== "offline" && p.game_name ? `<div class="gm">${icon(p.game_id, p.game_name)}${isUnlisted(p.game_id) ? "Playing a game" : esc(p.game_name)}</div>` : "";
  return `<div class="fr" data-h="${esc(f.handle)}"><div class="fa ${tint(f.handle)}">${initial(f.display_name || f.handle)}</div><div class="who">
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

/* ----- settings ----- */
pages.settings = async () => {
  if (!requireLogin()) return;
  let m;
  try { m = await api("/me"); } catch (e) { if (!token()) return requireLogin(); showErr($("#msg"), e); return; }
  const u = m.user;
  setSession(token(), { id: u.id, handle: u.handle, display_name: u.display_name, email: u.email });
  renderHeader();
  $("#s-handle").textContent = u.handle;
  $("#s-email").textContent = u.email || "none";
  const fn = $("#f-name"), fp = $("#f-privacy"), fpw = $("#f-pw"), fe = $("#f-email"), fd = $("#f-delete");
  fn.display_name.value = u.display_name || "";
  fp.share.checked = !!u.share_activity;
  const ok = (f, t) => showOk($(".alert", f), t), bad = (f, e) => showErr($(".alert", f), e);
  fn.addEventListener("submit", async e => {
    e.preventDefault();
    try { const r = await api("/me/display_name", { method: "POST", body: { display_name: fn.display_name.value } });
      fn.display_name.value = r.display_name; setSession(token(), Object.assign(me(), { display_name: r.display_name })); renderHeader(); ok(fn, "Saved."); }
    catch (x) { bad(fn, x.status === 404 || x.status === 405 ? new Error("Changing your display name needs the next server update.") : x); }
  });
  fp.share.addEventListener("change", async () => {
    try { await api("/me/privacy", { method: "POST", body: { share_activity: fp.share.checked } }); ok(fp, "Saved."); }
    catch (x) { fp.share.checked = !fp.share.checked; bad(fp, x); }
  });
  fpw.addEventListener("submit", async e => {
    e.preventDefault();
    if (fpw.new_password.value !== fpw.new_password2.value) return bad(fpw, new Error("The new passwords don't match."));
    try { await api("/me/password", { method: "POST", body: { current_password: fpw.current_password.value, new_password: fpw.new_password.value } }); fpw.reset(); ok(fpw, "Password changed. Other signed-in devices stay signed in."); }
    catch (x) { bad(fpw, x); }
  });
  fe.addEventListener("submit", async e => {
    e.preventDefault();
    try { const r = await api("/me/email", { method: "POST", body: { current_password: fe.current_password.value, new_email: fe.new_email.value.trim() } });
      $("#s-email").textContent = fe.new_email.value.trim(); fe.reset(); ok(fe, "Email changed."); }
    catch (x) { bad(fe, x); }
  });
  fd.addEventListener("submit", async e => {
    e.preventDefault();
    if (!confirm("Delete your Fraglink account and all your data now? This can't be undone.")) return;
    try { await api("/me", { method: "DELETE", body: { password: fd.password.value } }); clearSession(); location.href = "/?deleted=1"; }
    catch (x) { bad(fd, x); }
  });
  // devices
  try {
    const d = await api("/devices"), box = $("#devices");
    box.innerHTML = d.devices.length ? d.devices.map(x => `<div class="fr" data-id="${esc(x.id)}"><div class="who"><div class="nm">${esc(x.device_name || "Fraglink desktop")}</div><div class="gm">signed in ${fullDate.format(new Date(x.created_at))}${x.last_used_at ? " · last used " + fullDate.format(new Date(x.last_used_at)) : ""}</div></div><div class="acts"><button class="btn sm" data-revoke>Sign out</button></div></div>`).join("")
      : `<div class="body empty">No desktop apps signed in.</div>`;
    box.addEventListener("click", async e => {
      const b = e.target.closest("[data-revoke]"); if (!b) return;
      const row = b.closest("[data-id]"); b.disabled = true;
      try { await api("/devices/" + encodeURIComponent(row.dataset.id), { method: "DELETE" }); row.remove(); } catch (x) { b.disabled = false; alert(x.message); }
    });
  } catch (e) { /* not fatal */ }
  $("#export").addEventListener("click", async () => {
    try {
      const r = await fetch(API + "/export.zip", { headers: { Authorization: "Bearer " + token() } });
      if (!r.ok) throw new Error("Export failed (HTTP " + r.status + ").");
      const a = document.createElement("a"); a.href = URL.createObjectURL(await r.blob()); a.download = "fraglink-export.zip"; a.click();
    } catch (x) { alert(x.message); }
  });
};

/* ---------- boot ---------- */
document.addEventListener("DOMContentLoaded", () => {
  renderHeader();
  const y = $("#year"); if (y) y.textContent = new Date().getFullYear();
  const f = pages[document.body.dataset.page];
  if (f) Promise.resolve(f()).catch(e => { console.error(e); });
  // keep the header in sync with the real account (handle/display name changes)
  if (token() && document.body.dataset.page !== "settings") api("/me").then(m => { setSession(token(), { id: m.user.id, handle: m.user.handle, display_name: m.user.display_name, email: m.user.email }); renderHeader(); }).catch(() => { if (!token()) renderHeader(); });
});
})();
