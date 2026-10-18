/* Fraglink staff tools (/admin). Loaded only after the server confirmed a staff role;
   every action is checked again on the server and written to the audit log. */
window.FLAdmin = function (who) {
"use strict";
const { api, esc, $, $$, hours, fullDate, token, icon, loadIcons } = window.FL;
const can = who.can || {};
const root = $("#admin-root");
const dt = new Intl.DateTimeFormat("en-AU", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
const when = iso => iso ? dt.format(new Date(iso)) : "-";
const day = iso => iso ? fullDate.format(new Date(iso)) : "-";
const hrs = min => (Math.round((min || 0) / 6) / 10).toLocaleString("en") + " h";
const bytes = b => b == null ? "-" : b > 1e9 ? (b / 1e9).toFixed(1) + " GB" : (b / 1e6).toFixed(0) + " MB";
const pct = (a, b) => b ? Math.round(a / b * 100) : 0;
const rolePill = r => r ? `<span class="pill ${esc(r)}">${esc(r)}</span>` : "";
function msg(el, text, ok) { el.className = "alert " + (ok ? "ok" : "err"); el.textContent = text; el.hidden = false; }
async function act(btn, fn) {
  if (btn) btn.disabled = true;
  try { return await fn(); } catch (e) { alert(e.message); } finally { if (btn) btn.disabled = false; }
}

const TABS = [
  ["approvals", "Game approvals", can.approvals],
  ["games", "Games", can.games],
  ["users", "Users", can.users],
  ["hours", "Hours review", can.hours],
  ["avatars", "Reported pictures", can.avatars],
  ["server", "Server", can.server],
  ["announcements", "Announcements", can.announcements],
  ["news", "News", can.news],
  ["settings", "Site settings", can.settings],
  ["staff", "Staff", can.staff],
].filter(t => t[2]);

root.innerHTML = `<div class="wrap adm">
  <div class="adm-head"><h1>Admin</h1><span class="role">${esc(who.role)}</span><span class="dimt">Signed in as ${esc(who.handle)}. Every action is logged.</span></div>
  <nav class="tabs">${TABS.map(t => `<a href="#${t[0]}" data-tab="${t[0]}">${esc(t[1])}<span class="n" hidden></span></a>`).join("")}</nav>
  <div id="view"></div></div>`;
const view = $("#view");
let timer = null;

function route() {
  clearInterval(timer);
  view.onclick = null; view.onchange = null;
  const [tab, arg] = (location.hash.slice(1) || TABS[0][0]).split("/");
  const t = tab === "user" ? "users" : tab;
  $$(".tabs a").forEach(a => a.classList.toggle("on", a.dataset.tab === t));
  const fn = { approvals, games, users, hours: hoursReview, avatars: avatarReports, server, announcements, news, settings: siteSettings, staff, user: userDetail }[tab];
  if (!fn || !TABS.some(x => x[0] === t)) { location.hash = TABS[0][0]; return; }
  view.innerHTML = `<div class="panel"><div class="body empty">Loading…</div></div>`;
  Promise.resolve(fn(arg && decodeURIComponent(arg))).catch(e => { view.innerHTML = `<div class="alert err">${esc(e.message)}</div>`; });
}
window.addEventListener("hashchange", route);

/* ---------- 1. game approvals ---------- */
async function approvals() {
  const decided = sessionStorage.getItem("adm_sugg") === "decided";
  const d = await api("/admin/suggestions" + (decided ? "?status=decided" : ""));
  const rows = d.suggestions;
  const unk = decided ? [] : (await api("/admin/unknown_games").catch(() => ({ unknown: [] }))).unknown;
  if (!decided) badge("approvals", rows.length + unk.length);
  const unkHtml = unk.length ? `<section class="panel"><h2>Unknown game IDs from the app (${unk.length})</h2>
    <div class="note">The app reported a game id the server doesn't know. Until you decide, it shows as "Playing a game". Map it to an existing game (moves its sessions, and future reports go there), approve it as a new game, or reject it.</div>
    <table><thead><tr><th>Game id</th><th>Exe</th><th class="num">Players</th><th class="num">Hours</th><th>First seen</th><th class="num"></th></tr></thead><tbody>
    ${unk.map(u => `<tr data-unk="${esc(u.game_id)}"><td class="mono">${esc(u.game_id)}</td><td class="mono dimt">${esc(u.exe || "unknown")}</td><td class="num">${u.players}</td><td class="num">${hrs(u.seconds / 60)}</td><td>${day(u.first_seen)}</td>
      <td class="num"><div class="acts"><input class="in" list="unk-games" data-map placeholder="Map to game…" size="20" aria-label="Map to existing game"><button class="btn sm" data-domap>Map</button>
        <input class="in" data-uname placeholder="Official name" size="16" aria-label="Name for a new game"><button class="btn sm dl" data-uapprove>Approve new</button><button class="btn sm danger" data-ureject>Reject</button></div></td></tr>`).join("")}</tbody></table>
    <datalist id="unk-games"></datalist></section>` : "";
  view.innerHTML = unkHtml + `<section class="panel"><h2>${decided ? "Decided" : "Pending"} game suggestions <span class="more"><a href="#" data-flip>${decided ? "Show pending" : "Show decided"}</a></span></h2>
    <div class="note">When someone picks "Track it as a game?" in the app, the program lands here. Approve it with an official name and every app detects it and shows that name. "Not a game" keeps it as "Playing a game".</div>
    ${rows.length ? `<table><thead><tr><th>Program</th><th>Names submitted</th><th class="num">Reports</th><th class="num">Users</th><th>First seen</th><th class="num"></th></tr></thead><tbody>
    ${rows.map(r => {
      const top = r.names[0] ? r.names[0].name : "";
      const others = r.names.slice(1).map(n => `${esc(n.name)} (${n.reports})`).join(", ");
      return `<tr data-exe="${esc(r.exe)}"><td class="mono">${esc(r.exe)}</td>
        <td>${esc(top)}${others ? `<div class="dimt">also: ${others}</div>` : ""}</td>
        <td class="num">${r.reports}</td><td class="num">${r.users}</td><td>${day(r.first_seen)}</td>
        <td class="num"><div class="acts">${decided
          ? `<span class="pill ${r.status === "accepted" ? "ok" : "bad"}">${r.status === "accepted" ? "approved" : "not a game"}</span>${r.approved_name ? `<span class="muted">${esc(r.approved_name)}</span>` : ""}<button class="btn sm" data-undo>Undo</button>`
          : `<input class="in" name="name" value="${esc(top)}" maxlength="80" size="22" aria-label="Official name"><button class="btn sm dl" data-approve>Approve</button><button class="btn sm danger" data-reject>Not a game</button>`}</div></td></tr>`;
    }).join("")}</tbody></table>` : `<div class="body empty">${decided ? "Nothing decided yet." : "No pending suggestions."}</div>`}</section>`;
  $("[data-flip]", view).addEventListener("click", e => { e.preventDefault(); sessionStorage.setItem("adm_sugg", decided ? "pending" : "decided"); approvals(); });
  let mt;
  view.oninput = e => {
    const i = e.target.closest("[data-map]"); if (!i) return; clearTimeout(mt);
    mt = setTimeout(async () => { const v = i.value.trim(); if (v.length < 2) return; const d2 = await api("/admin/games?q=" + encodeURIComponent(v)); $("#unk-games").innerHTML = d2.games.filter(g => g.source !== "unlisted").map(g => `<option value="${esc(g.id)}">${esc(g.public_name || g.name)}</option>`).join(""); }, 250);
  };
  view.onclick = async e => {
    const ut = e.target.closest("tr[data-unk]");
    if (ut) {
      const id = ut.dataset.unk, b = e.target.closest("button"); if (!b) return;
      if (b.matches("[data-domap]")) { const to = $("[data-map]", ut).value.trim(); if (!to) return alert("Type a game and pick it from the list."); if (!confirm(`Move everything recorded as ${id} to ${to}?`)) return; await act(b, async () => { await api(`/admin/unknown_games/${encodeURIComponent(id)}/map`, { method: "POST", body: { to } }); ut.remove(); }); }
      else if (b.matches("[data-uapprove]")) { const name = $("[data-uname]", ut).value.trim(); if (!name) return alert("Enter the official name first."); await act(b, async () => { await api(`/admin/unknown_games/${encodeURIComponent(id)}/approve`, { method: "POST", body: { name } }); ut.remove(); }); }
      else if (b.matches("[data-ureject]")) await act(b, async () => { await api(`/admin/unknown_games/${encodeURIComponent(id)}/reject`, { method: "POST" }); ut.remove(); });
      return;
    }
    const tr = e.target.closest("tr[data-exe]"); if (!tr) return;
    const exe = tr.dataset.exe, b = e.target.closest("button"); if (!b) return;
    if (b.matches("[data-approve]")) {
      const name = $("input[name=name]", tr).value.trim();
      if (!name) return alert("Enter the official name first.");
      await act(b, async () => { await api("/admin/suggestions/approve", { method: "POST", body: { exe, name } }); tr.remove(); });
    } else if (b.matches("[data-reject]")) {
      await act(b, async () => { await api("/admin/suggestions/reject", { method: "POST", body: { exe } }); tr.remove(); });
    } else if (b.matches("[data-undo]")) {
      await act(b, async () => { await api("/admin/suggestions/undo", { method: "POST", body: { exe } }); tr.remove(); });
    }
  };
}
/* ---------- reported pictures ---------- */
async function avatarReports() {
  const d = await api("/admin/avatar_reports");
  badge("avatars", d.reports.length);
  const pic = a => a && a.url_64 ? `<img src="${esc(a.url_64)}" width="64" height="64" style="border-radius:4px;border:1px solid var(--line)" alt="">` : `<span class="muted">(already reset)</span>`;
  view.innerHTML = `<section class="panel"><h2>Reported pictures</h2><div class="note">Players report pictures from profiles. Remove resets it to the default and blocks that exact image from being uploaded again.</div>
    ${d.reports.length ? `<table><thead><tr><th>Picture</th><th>Player</th><th class="num">Reports</th><th>Latest reason</th><th>Last report</th><th class="num"></th></tr></thead><tbody>
    ${d.reports.map(r => `<tr data-id="${esc(r.user_id)}"><td>${pic(r.avatar)}</td><td><a href="#user/${esc(r.user_id)}">${esc(r.handle)}</a></td><td class="num">${r.reports}</td><td>${esc(r.reason || "")}</td><td>${when(r.last_at)}</td>
      <td class="num"><div class="acts"><button class="btn sm danger" data-rm>Remove</button><button class="btn sm" data-ok>Keep</button></div></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No open reports.</div>`}</section>`;
  view.onclick = async e => {
    const b = e.target.closest("button"); if (!b) return; const tr = b.closest("tr[data-id]"); const id = tr.dataset.id;
    if (b.matches("[data-rm]")) await act(b, async () => { await api(`/admin/users/${id}/avatar/remove`, { method: "POST" }); tr.remove(); });
    else if (b.matches("[data-ok]")) await act(b, async () => { await api(`/admin/avatar_reports/${id}/dismiss`, { method: "POST" }); tr.remove(); });
    else if (b.matches("[data-pok]")) await act(b, async () => { await api(`/admin/profile_reports/${id}/dismiss`, { method: "POST" }); tr.remove(); });
  };
  const pr = await api("/admin/profile_reports");
  view.insertAdjacentHTML("beforeend", `<section class="panel"><h2>Reported profiles</h2><div class="note">From "Report this profile". Open the player to change their nickname, ban them or reset details; Dismiss closes the reports.</div>
    ${pr.reports.length ? `<table><thead><tr><th>Player</th><th class="num">Reports</th><th>Latest reason</th><th>Last report</th><th class="num"></th></tr></thead><tbody>
    ${pr.reports.map(r => `<tr data-id="${esc(r.user_id)}"><td><a href="#user/${esc(r.user_id)}">${esc(r.handle)}</a> · <a href="/u/${encodeURIComponent(r.handle)}" target="_blank">profile</a></td><td class="num">${r.reports}</td><td>${esc(r.reason || "")}</td><td>${when(r.last_at)}</td>
      <td class="num"><button class="btn sm" data-pok>Dismiss</button></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No open reports.</div>`}</section>`);
  badge("avatars", d.reports.length + pr.reports.length);
}
function badge(tab, n) { const s = $(`.tabs a[data-tab=${tab}] .n`); if (s) { s.textContent = n; s.hidden = !n; } }

/* ---------- 2. games ---------- */
async function games(q) {
  q = q || "";
  view.innerHTML = `<div class="split"><section class="panel"><h2>Games</h2>
    <form class="tools" id="gs"><input class="in grow" name="q" value="${esc(q)}" placeholder="Search by name, id or exe" autocomplete="off">
      <select class="in" id="gshow" aria-label="Show"><option value="">All games with hours</option><option value="edited">Edited / retired / status set</option></select></form>
    <div id="glist"><div class="body empty">Loading…</div></div></section>
    <section class="panel" id="gedit"><h2>Edit game</h2><div class="body empty">Pick a game on the left.</div></section></div>`;
  $("#gs").addEventListener("submit", e => { e.preventDefault(); history.replaceState(null, "", "#games/" + encodeURIComponent(e.target.q.value.trim())); listGames(e.target.q.value.trim()); });
  liveSearch($("#gs"));
  GS.offset = 0;
  $("#gshow").onchange = e => { GS.show = e.target.value; GS.offset = 0; listGames($("#gs").q.value.trim()); };
  listGames(q);
}
const GS = { sort: "players", show: "", offset: 0, lastQ: "" };
/* Search as you type (debounced); Enter still works. */
function liveSearch(form) {
  let t; const i = form.q; i.type = "search";
  i.addEventListener("input", () => { clearTimeout(t); t = setTimeout(() => form.requestSubmit(), 300); });
}
async function listGames(q) {
  const box = $("#glist");
  if (q !== GS.lastQ) { GS.offset = 0; GS.lastQ = q; }
  const d = await api("/admin/games?" + new URLSearchParams({ q, sort: GS.sort, show: GS.show, offset: GS.offset }));
  if (!d.games.length) { box.innerHTML = `<div class="body empty">${q ? `No games match “${esc(q)}”.` : "No games."}</div>`; return; }
  const th = (k, l, c) => `<th class="${c || ""} srt${GS.sort === k ? " on" : ""}" data-sort="${k}" title="Sort by ${l.toLowerCase()}">${l}${GS.sort === k ? (k === "name" ? " ▴" : " ▾") : ""}</th>`;
  const from = GS.offset + 1, to = GS.offset + d.games.length;
  box.innerHTML = `<div class="note">${q ? "Matches" : GS.show ? "Edited, retired or with a status" : "Every game with hours"} · players and hours count both tiers (tracked and imported; best figure per player) and include test accounts (the public site leaves those out).</div>
    <table><thead><tr><th></th>${th("name", "Name")}<th>Id</th><th>Source</th>${th("players", "Players", "num")}${th("hours", "Hours", "num")}${th("recent", "Last played", "num")}</tr></thead><tbody>
    ${d.games.map(g => {
      const o = g.override || {};
      const flags = (o.name ? `<span class="pill warn">renamed</span>` : "") + (o.hidden ? `<span class="pill bad">hidden</span>` : "") +
        (o.has_icon ? `<span class="pill">icon</span>` : "") + ((o.extra_exes || []).length ? `<span class="pill">+${o.extra_exes.length} exe</span>` : "") +
        (g.retired ? `<span class="pill bad">${g.retired.partial ? "exe retired" : "retired"}</span>` : "") +
        (g.status && g.status.lifecycle !== "live" ? `<span class="pill ${g.status.lifecycle === "ended" ? "bad" : "warn"}">${esc(LIFE[g.status.lifecycle] || g.status.lifecycle)}</span>` : "");
      const bd = [["tracked", g.tracked_min], ["imported", g.imported_min]].filter(x => x[1] > 0).map(([k, m]) => `${k} ${hrs(m)}`).join(" · ");
      return `<tr class="click" data-id="${esc(g.id)}"><td class="ic"><img src="${esc(g.icon)}" width="20" height="20" alt="" onerror="this.style.visibility='hidden'"></td>
        <td>${esc(g.public_name || g.name)}${flags}</td><td class="mono dimt">${esc(g.id)}</td><td class="muted">${esc(g.source)}</td>
        <td class="num">${g.players}</td><td class="num" title="${esc(bd || "no hours")}">${hrs(g.total_min)}${bd ? `<div class="dimt" style="font-size:10.5px;white-space:nowrap">${esc(bd)}</div>` : ""}</td><td class="num dimt">${g.last_played_at ? day(g.last_played_at) : "-"}</td></tr>`;
    }).join("")}</tbody></table>
    ${d.total > d.limit ? `<div class="pager"><button class="btn sm" data-goff="${Math.max(0, GS.offset - d.limit)}" ${GS.offset ? "" : "disabled"}>‹ Prev</button><span>${from}–${to} of ${d.total.toLocaleString("en")}</span><button class="btn sm" data-goff="${GS.offset + d.limit}" ${to < d.total ? "" : "disabled"}>Next ›</button></div>` : ""}`;
  box.onclick = e => {
    const s_ = e.target.closest("th[data-sort]"); if (s_) { GS.sort = s_.dataset.sort; GS.offset = 0; listGames(q); return; }
    const pg = e.target.closest("[data-goff]"); if (pg) { GS.offset = +pg.dataset.goff; listGames(q).then(() => box.scrollIntoView({ block: "start" })); return; }
    const tr = e.target.closest("tr[data-id]"); if (tr) editGame(tr.dataset.id);
  };
}
const LIFE = { live: "Live", beta: "Beta", test: "Test", early_access: "Early Access", ended: "Ended" };
function statusHtml(g) {
  const s = g.status || { lifecycle: "live" };
  return `<form class="f" id="gst"><label>Status</label>
    <div class="row"><select class="in" name="lifecycle">${Object.entries(LIFE).map(([k, v]) => `<option value="${k}"${s.lifecycle === k ? " selected" : ""}>${v}</option>`).join("")}</select>
      <label style="margin:0">Expected end / launch <input type="date" name="date" value="${esc(s.date || "")}"></label></div>
    <label>Note<input type="text" name="note" maxlength="500" value="${esc(s.note || "")}" placeholder="e.g. beta ends when the full game launches"></label>
    <label class="check"><input type="checkbox" name="checked" checked> Mark as checked now</label>
    <div class="row"><button class="btn">Save status</button><span class="dimt">Last checked: ${s.checked_at ? `${when(s.checked_at)} by ${esc(s.checked_by || "?")}` : "never"}</span></div>
    <div class="alert" id="stmsg" hidden></div></form>`;
}
function retireHtml(g) {
  const r = g.retired, exes = g.exes || [];
  if (r && r.merged) return `<div class="f"><label>Merged</label><div>Merged into <a href="#" data-goto="${esc(r.successor)}" class="mono">${esc(r.successor)}</a> on ${day(r.at)}. Its play and hours live there now, and apps reporting this id count for that game.</div></div>`;
  const mf = (g.merged_from || []).length ? `<div class="dimt">Merged in: ${g.merged_from.map(x => `<a href="#" data-goto="${esc(x)}" class="mono">${esc(x)}</a>`).join(", ")}</div>` : "";
  if (r) return `<div class="f"><label>Retired</label><div>${r.partial ? `Detached ${r.exes.map(e => `<span class="mono">${esc(e)}</span>`).join(", ")} on ${day(r.at)}; the game stays live.` : `Retired ${day(r.at)}. New play goes to <a href="#" data-goto="${esc(r.successor)}" class="mono">${esc(r.successor)}</a>.`}</div>
    <div class="row"><button class="btn" id="gunret" type="button">Undo retire</button><small>Only while the new entry has no hours.</small></div><div class="alert" id="rmsg" hidden></div></div>`;
  return `<div class="f"><label>Retire</label>${mf}${g.retired_from ? `<div class="dimt">New entry for retired <a href="#" data-goto="${esc(g.retired_from)}" class="mono">${esc(g.retired_from)}</a>.</div>` : ""}
    ${exes.length ? `<div>Programs to detach:</div>${exes.map(e => `<label class="check"><input type="checkbox" name="rx" value="${esc(e)}" checked> <span class="mono">${esc(e)}</span></label>`).join("")}` : `<div class="dimt">No known program names; add an extra exe first.</div>`}
    <div class="row"><button class="btn danger" id="gret" type="button"${exes.length ? "" : " disabled"}>Retire…</button></div>
    <small>All programs: the entry is frozen (name, icon and hours stay) and new play starts a fresh entry in Game approvals. Some programs: only those move to fresh entries; the game stays live.</small>
    <div class="alert" id="rmsg" hidden></div></div>`;
}
async function editGame(id) {
  const box = $("#gedit");
  const g = await api("/admin/games/" + encodeURIComponent(id));
  const o = g.override || {};
  const bust = "?t=" + Date.now();
  box.innerHTML = `<h2>Edit game</h2><div class="body"><form class="f" id="gf">
    <div><b>${esc(g.public_name || g.name || g.id)}</b> <span class="mono dimt">${esc(g.id)}</span><div class="dimt">From: ${esc(g.source)} · ${g.players} players · ${hrs(g.total_min)} (tracked ${hrs(g.tracked_min)} · imported ${hrs(g.imported_min)})</div></div>
    <label>Display name override<input type="text" name="name" maxlength="80" value="${esc(o.name || "")}" placeholder="${esc(g.name)}"><small>Leave empty to use the list name.</small></label>
    <label class="check"><input type="checkbox" name="hidden" ${o.hidden ? "checked" : ""}> Hide from public (shown as "Playing a game")</label>
    <label>Extra exe names (one per line)<textarea class="in" name="exes" placeholder="GameClient-Win64.exe">${esc((o.extra_exes || []).join("\n"))}</textarea><small>Apps detect these as this game (picked up within a day or on restart).</small></label>
    <div class="row"><button class="btn dl">Save</button><span class="dimt">Edits stay in place when the games list refreshes.</span></div>
    <div class="alert" id="gmsg" hidden></div></form>
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    <div id="media"><div class="dimt">Loading pictures…</div></div>
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    ${statusHtml(g)}
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">${retireHtml(g)}
    ${g.retired ? "" : `<hr style="border:0;border-top:1px solid var(--line);margin:14px 0"><div class="f" id="mrg"><label>Merge into… <span class="dimt">(for a game the games list re-added under a new id)</span></label>
      <div class="row"><input class="in" id="mrgq" list="mrg-games" placeholder="Search the game to keep…" style="flex:1" autocomplete="off"><datalist id="mrg-games"></datalist><button class="btn" type="button" id="mrgp">Preview</button></div>
      <div id="mrgout"></div><div class="alert" id="mrgmsg" hidden></div></div>`}
    ${g.deletable ? `<hr style="border:0;border-top:1px solid var(--line);margin:14px 0"><div class="f"><label>Delete</label>
      <div class="dimt">Removes this unlisted entry for good${g.sessions ? `, including <b>${g.sessions} session${g.sessions === 1 ? "" : "s"}</b> from <b>${g.players} player${g.players === 1 ? "" : "s"}</b> (${hrs(g.total_min)})` : " (no sessions recorded)"}. Logged in the audit log.</div>
      <div class="row"><button class="btn danger" id="gdelete" type="button">Delete game…</button></div><div class="alert" id="dmsg" hidden></div></div>` : ""}</div>`;
  const gid = "/admin/games/" + encodeURIComponent(id);
  $("#gst").addEventListener("submit", async e => {
    e.preventDefault(); const f = e.target;
    try { await api(gid + "/status", { method: "POST", body: { lifecycle: f.lifecycle.value, date: f.date.value, note: f.note.value, checked: f.checked.checked } });
      editGame(id); listGames($("#gs").q.value.trim());
    } catch (x) { msg($("#stmsg"), x.message); }
  });
  box.querySelectorAll("[data-goto]").forEach(a => a.addEventListener("click", e => { e.preventDefault(); editGame(a.dataset.goto); }));
  const ret = $("#gret"), unret = $("#gunret");
  if (ret) ret.addEventListener("click", () => {
    const all = [...box.querySelectorAll("input[name=rx]")], pick = all.filter(c => c.checked).map(c => c.value);
    if (!pick.length) return msg($("#rmsg"), "Pick at least one program.");
    const whole = pick.length === all.length;
    if (!confirm(whole ? `Retire ${g.public_name || g.name || id}?\n\nIts hours stay as they are; the next time ${pick.join(", ")} runs, play goes to a new entry (in Game approvals).`
      : `Detach ${pick.join(", ")} from ${g.public_name || g.name || id}?\n\nThose programs get new entries (in Game approvals); the game stays live with its other programs.`)) return;
    act(ret, async () => { await api(gid + "/retire", { method: "POST", body: { exes: pick } }); editGame(id); listGames($("#gs").q.value.trim()); }).catch(x => msg($("#rmsg"), x.message));
  });
  if (unret) unret.addEventListener("click", () => act(unret, async () => { await api(gid + "/unretire", { method: "POST" }); editGame(id); listGames($("#gs").q.value.trim()); }).catch(x => msg($("#rmsg"), x.message)));
  $("#gf").addEventListener("submit", async e => {
    e.preventDefault(); const f = e.target;
    try {
      await api("/admin/games/" + encodeURIComponent(id), { method: "POST", body: { name: f.name.value.trim() || null, hidden: f.hidden.checked, extra_exes: f.exes.value.split(/\n|,/).map(s => s.trim()).filter(Boolean) } });
      msg($("#gmsg"), "Saved.", true); listGames($("#gs").q.value.trim());
    } catch (x) { msg($("#gmsg"), x.message); }
  });
  renderMedia(id, g);
  wireMerge(id);
  const gd = $("#gdelete");
  if (gd) gd.addEventListener("click", () => {
    const typed = prompt(`Delete "${g.public_name || g.name || id}"${g.sessions ? ` and ${g.sessions} session(s) from ${g.players} player(s)` : ""}? This can't be undone.\n\nType the game id to confirm:\n${id}`);
    if (typed === null) return;
    if (typed.trim() !== id) return msg($("#dmsg"), "The id didn't match; nothing was deleted.");
    act(gd, async () => { await api("/admin/games/" + encodeURIComponent(id) + "/delete", { method: "POST", body: { confirm: typed.trim() } }); $("#gedit").innerHTML = `<h2>Edit game</h2><div class="body empty">Deleted ${esc(id)}.</div>`; listGames($("#gs").q.value.trim()); }).catch(x => msg($("#dmsg"), x.message));
  });
}

/* ---------- 3. users ---------- */
async function users(q) {
  q = q || "";
  view.innerHTML = `<section class="panel"><h2>Users</h2>
    <form class="tools" id="us"><input class="in grow" name="q" value="${esc(q)}" placeholder="Username, display name or email" autocomplete="off"></form>
    <div id="ulist"></div></section>`;
  $("#us").addEventListener("submit", e => { e.preventDefault(); const v = e.target.q.value.trim(); history.replaceState(null, "", "#users/" + encodeURIComponent(v)); listUsers(v); });
  liveSearch($("#us"));
  listUsers(q);
}
async function listUsers(q) {
  const d = await api("/admin/users?q=" + encodeURIComponent(q));
  $("#ulist").innerHTML = d.users.length ? `<div class="note">${q ? "Matches" : "Newest accounts"}</div><table><thead><tr><th>Username</th><th>Display name</th><th>Email</th><th>Age</th><th>Joined</th><th>Status</th></tr></thead><tbody>
    ${d.users.map(u => `<tr class="click" data-id="${esc(u.id)}"><td><a href="#user/${esc(u.id)}">${esc(u.handle)}</a>${rolePill(u.role)}${u.is_test ? `<span class="pill" title="Test account: hidden from everyone else">test</span>` : ""}${u.banned ? `<span class="pill bad">banned</span>` : ""}${u.age_review_at ? `<span class="pill bad" title="Entered a birthday under 13. Can't sign in until reviewed.">age check</span>` : ""}${u.hours_flag_at ? `<span class="pill bad" title="Unusual tracked hours. Look at Sessions; nothing was removed.">hours</span>` : ""}</td>
      <td>${esc(u.display_name)}</td><td class="muted">${esc(u.email || "-")}</td><td>${u.is_minor ? `<span class="pill" title="Under 18: private by default; can go public">13–17</span>` : `<span class="dimt">18+</span>`}</td><td>${day(u.created_at)}</td>
      <td><span class="status"><i class="dot ${esc(u.status === "invisible" ? "offline" : u.status)}"></i>${esc(u.status)}</span>${u.last_seen ? ` <span class="dimt">${when(u.last_seen)}</span>` : ""}</td></tr>`).join("")}</tbody></table>`
    : `<div class="body empty">No accounts match.</div>`;
  $("#ulist").onclick = e => { const tr = e.target.closest("tr[data-id]"); if (tr && !e.target.closest("a")) location.hash = "user/" + tr.dataset.id; };
}
async function userDetail(id) {
  const u = await api("/admin/users/" + encodeURIComponent(id));
  await loadIcons(u.games.map(g => g.game_id));
  const p = u.presence || {};
  const pres = p.status && p.status !== "offline" ? `${esc(p.status)}${p.game_id ? " · playing " + esc(p.game_name || p.game_id) + (p.game_hidden ? " (hidden from friends)" : "") : ""}${p.game_detail ? " · " + esc(p.game_detail) : ""}` : "offline";
  view.innerHTML = `<p><a href="#users">← Users</a></p>
  <div class="split"><div class="col">
    <section class="panel"><h2>${esc(u.handle)} ${rolePill(u.role)}${u.banned_at ? `<span class="pill bad">banned</span>` : ""}${u.age_review_at ? `<span class="pill bad">age check</span>` : ""}${u.hours_flag_at ? `<span class="pill bad">unusual hours</span>` : ""}${u.is_minor ? `<span class="pill">13–17</span>` : ""}<span class="more"><a href="/u/${encodeURIComponent(u.handle)}">Public profile</a></span></h2><div class="body">
      <dl class="kv"><dt>Display name</dt><dd>${esc(u.display_name)}</dd><dt>Email</dt><dd>${esc(u.email || "-")}${u.email ? (u.email_verified ? " (verified)" : " (not verified)") : ""}</dd>
      <dt>Member since</dt><dd>${day(u.created_at)}</dd><dt>Status</dt><dd>${pres}${p.connected ? ` <span class="pill ok">connected</span>` : ""}</dd>
      <dt>Friends</dt><dd>${u.friend_count}</dd><dt>Shares activity</dt><dd>${u.share_activity ? "Yes" : "No"}</dd>
      ${u.linked.length ? `<dt>Linked</dt><dd>${u.linked.map(l => esc(l.provider) + " " + esc(l.display)).join(", ")}</dd>` : ""}
      ${u.banned_at ? `<dt>Banned</dt><dd>${when(u.banned_at)}${u.ban_reason ? " · " + esc(u.ban_reason) : ""}</dd>` : ""}
      <dt>Date of birth</dt><dd>${u.birth_date ? esc(u.birth_date) : "<span class=\"dimt\">(not set)</span>"}${u.is_minor ? " · 13–17 (private by default; can go public)" : " · 18 or older"}${u.minor_set_at ? ` · since ${day(u.minor_set_at)}` : ""}</dd>
      ${u.hours_flag_at ? `<dt>Unusual hours</dt><dd>Flagged ${when(u.hours_flag_at)}: ${esc(u.hours_flag_reason || "")}. Nothing was removed (idle and away time count). Check the sessions below.</dd>` : ""}
      ${u.age_review_at ? `<dt>Age check</dt><dd>Paused ${when(u.age_review_at)}: entered a birthday under 13 (not saved). Can't sign in until cleared.</dd>` : ""}
      <dt>User id</dt><dd class="mono dimt">${esc(u.id)}</dd></dl></div></section>
    <section class="panel"><h2>Games</h2>${u.games.length ? `<table><thead><tr><th></th><th>Game</th><th class="num">Past 7 days</th><th class="num">Total</th><th class="num"></th></tr></thead><tbody>
      ${u.games.map(g => `<tr data-game="${esc(g.game_id)}"><td class="ic">${icon(g.game_id, g.name)}</td><td>${esc(g.name)}${g.name === "Unlisted game" && g.stored_name ? ` <span class="dimt">(${esc(g.stored_name)})</span>` : ""}<div class="mono dimt">${esc(g.game_id)}</div></td>
        <td class="num">${hrs(g.minutes_7d)}</td><td class="num">${hrs(g.display_min)}${g.display_tier !== "tracked" ? `<span class="tier">${esc(g.display_tier)}</span>` : ""}</td>
        <td class="num"><div class="acts"><button class="btn sm" data-cap>Lower…</button><button class="btn sm danger" data-remove>Remove</button></div></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No hours.</div>`}</section>
    <section class="panel"><h2>Recent sessions</h2>${u.sessions.length ? `<table><thead><tr><th>Game</th><th>Started</th><th class="num">Length</th><th>Source</th><th></th></tr></thead><tbody>
      ${u.sessions.map(s => `<tr data-sid="${esc(s.id)}"><td>${esc(s.name)}</td><td>${when(s.started_at)}</td><td class="num">${hrs(s.duration_s / 60)}${s.duration_s > 18 * 3600 ? `<span class="pill bad">long</span>` : ""}</td><td class="muted">${esc(s.source)}</td><td class="num"><button class="btn sm danger" data-delsess>Delete</button></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No sessions.</div>`}</section>
  </div><div class="col">
    <section class="panel"><h2>Actions</h2><div class="body f">
      ${u.hours_flag_at ? `<div class="row"><button class="btn" id="hoursclear">Clear hours flag</button></div><small>Only removes the flag. To fix hours, use the session tools.</small><hr style="border:0;border-top:1px solid var(--line);margin:6px 0">` : ""}
      ${u.age_review_at ? `<div class="row"><button class="btn" id="ageclear">Clear age check</button></div><small>Lets them sign in again. If they really are under 13, ban or delete the account instead. Nothing is deleted automatically.</small><hr style="border:0;border-top:1px solid var(--line);margin:6px 0">` : ""}
      ${u.banned_at ? `<button class="btn" id="unban">Unban</button>` : `<label>Ban reason (optional, kept in the audit log)<input type="text" id="reason" maxlength="200"></label><div class="row"><button class="btn danger" id="ban">Ban account</button></div><small>Banned accounts are signed out everywhere, can't sign in, and their profile is hidden.</small>`}
      ${can.reset_password ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div class="row"><button class="btn" id="resetpw">Reset password</button></div><small>Makes a temporary password (shown once) and signs the account out everywhere. Send it to the user yourself; email isn't set up yet.</small><div id="pwout"></div>` : ""}
      ${can.reset_password ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><label>Date of birth (YYYY-MM-DD)<input type="date" id="newdob" min="1900-01-01" max="2099-12-31" value="${esc(u.birth_date || "")}"></label><div class="row"><button class="btn" id="chdob">Save date of birth</button></div><small>Only staff can change this. Under 13 is refused and not stored. 13–17 forces a private profile; turning 18 clears the under-18 flag but leaves the profile private.</small>` : ""}
      ${can.change_handle ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><label>Username<input type="text" id="newhandle" maxlength="20" value="${esc(u.handle)}"></label><div class="row"><button class="btn" id="chhandle">Change username</button></div><small>Users can't change their username themselves. The old one keeps redirecting to them and can't be taken by anyone else. Logged in the audit log.</small>` : ""}
      <hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div class="row"><button class="btn" id="rmav">Remove picture</button>${can.mfa_reset ? ` <button class="btn" id="mfareset">Reset two-step sign-in</button>` : ""}</div><small>Removing a picture resets it to the default and blocks that exact image. Resetting two-step sign-in lets them sign in with just their password (check it's really them first).</small>
      <div class="alert" id="amsg" hidden></div></div></section>
    <section class="panel"><h2>Signed-in devices</h2>${u.devices.length ? u.devices.map(d => `<div class="fr"><div class="who"><div class="nm">${esc(d.name || (d.kind === "web" ? "Web browser" : "Fraglink desktop"))} <small>${esc(d.kind)}</small></div><div class="gm">signed in ${day(d.created_at)} · last used ${when(d.last_used_at)}</div></div></div>`).join("") : `<div class="body empty">None.</div>`}</section>
  </div></div>`;
  const amsg = $("#amsg");
  const ban = $("#ban"), unban = $("#unban"), reset = $("#resetpw");
  const chh = $("#chhandle");
  if (chh) chh.addEventListener("click", () => { const nh = $("#newhandle").value.trim(); if (nh && nh !== u.handle && confirm(`Change username ${u.handle} → ${nh}? Their profile address changes; the old one redirects.`)) act(chh, async () => { await api(`/admin/users/${id}/handle`, { method: "POST", body: { handle: nh } }); userDetail(id); }); });
  const chdob = $("#chdob");
  if (chdob) chdob.addEventListener("click", () => {
    const d = ($("#newdob").value || "").trim();
    if (!d) { msg(amsg, "Enter a date of birth."); return; }
    if (!confirm(`Set date of birth for ${u.handle} to ${d}?`)) return;
    act(chdob, async () => { await api(`/admin/users/${id}/birth_date`, { method: "POST", body: { birth_date: d } }); userDetail(id); }).catch(x => msg(amsg, x.message));
  });
  if (ban) ban.addEventListener("click", () => { if (confirm("Ban " + u.handle + "?")) act(ban, async () => { await api(`/admin/users/${id}/ban`, { method: "POST", body: { reason: $("#reason").value } }); userDetail(id); }).catch(x => msg(amsg, x.message)); });
  const hcl = $("#hoursclear");
  if (hcl) hcl.addEventListener("click", () => act(hcl, async () => { await api(`/admin/users/${id}/hours_flag/clear`, { method: "POST" }); userDetail(id); }).catch(x => msg(amsg, x.message)));
  const agec = $("#ageclear");
  if (agec) agec.addEventListener("click", () => { if (confirm("Clear the age check for " + u.handle + "? They'll be able to sign in again.")) act(agec, async () => { await api(`/admin/users/${id}/age_review/clear`, { method: "POST" }); userDetail(id); }).catch(x => msg(amsg, x.message)); });
  if (unban) unban.addEventListener("click", () => act(unban, async () => { await api(`/admin/users/${id}/unban`, { method: "POST" }); userDetail(id); }));
  const rmav = $("#rmav"), mfar = $("#mfareset");
  if (rmav) rmav.addEventListener("click", () => { if (confirm("Remove " + u.handle + "'s picture and block it?")) act(rmav, async () => { await api(`/admin/users/${id}/avatar/remove`, { method: "POST" }); msg(amsg, "Picture removed.", true); }); });
  if (mfar) mfar.addEventListener("click", () => { if (confirm("Turn off two-step sign-in for " + u.handle + "?")) act(mfar, async () => { const r = await api(`/admin/users/${id}/mfa/reset`, { method: "POST" }); msg(amsg, r.was_enabled ? "Two-step sign-in reset." : "Two-step sign-in wasn't on.", true); }); });
  if (reset) reset.addEventListener("click", () => {
    if (!confirm("Reset " + u.handle + "'s password and sign them out everywhere?")) return;
    act(reset, async () => { const r = await api(`/admin/users/${id}/reset_password`, { method: "POST" });
      $("#pwout").innerHTML = `<p>Temporary password (shown once):</p><div class="secret">${esc(r.temp_password)}</div><p class="dimt">Ask them to change it in Account settings after signing in.</p>`; });
  });
  view.onclick = async e => {
    const b = e.target.closest("button"); if (!b) return;
    const tr = b.closest("tr");
    if (b.matches("[data-cap]")) {
      const g = tr.dataset.game, cur = u.games.find(x => x.game_id === g);
      const v = prompt(`New tracked total for ${cur.name} in hours (now ${hrs(cur.tracked_min)}). Every session is shortened by the same factor.`);
      if (v == null || v.trim() === "") return;
      await act(b, async () => { await api(`/admin/users/${id}/hours`, { method: "POST", body: { game_id: g, action: "cap", hours: Number(v) } }); userDetail(id); });
    } else if (b.matches("[data-remove]")) {
      const g = tr.dataset.game;
      if (!confirm("Remove ALL hours for this game from " + u.handle + "? (tracked, imported and Steam)")) return;
      await act(b, async () => { await api(`/admin/users/${id}/hours`, { method: "POST", body: { game_id: g, action: "remove" } }); userDetail(id); });
    } else if (b.matches("[data-delsess]")) {
      if (!confirm("Delete this session?")) return;
      await act(b, async () => { await api(`/admin/users/${id}/sessions/${tr.dataset.sid}`, { method: "DELETE" }); tr.remove(); });
    }
  };
}

/* ---------- 4. hours review ---------- */
const FLAG = { day_over_20h: "Over 20 h in a day", session_over_18h: "Session over 18 h", overlapping_sessions: "Overlapping sessions", faster_than_real_time: "Faster than real time" };
async function hoursReview(days) {
  days = Number(days) || 30;
  const d = await api("/admin/hours/flags?days=" + days);
  view.innerHTML = `<section class="panel"><h2>Hours review</h2>
    <div class="tools"><span class="muted">Suspicious totals in the last</span><select class="in" id="hd">${[7, 30, 90, 365].map(n => `<option ${n === days ? "selected" : ""}>${n}</option>`).join("")}</select><span class="muted">days. Days are UTC.</span></div>
    ${d.flags.length ? `<table><thead><tr><th>Flag</th><th>User</th><th>Game</th><th class="num">Hours</th><th>When</th><th>Detail</th><th></th></tr></thead><tbody>
    ${d.flags.map(f => `<tr><td><span class="pill warn">${esc(FLAG[f.kind] || f.kind)}</span></td><td><a href="#user/${esc(f.user_id)}">${esc(f.handle)}</a></td>
      <td>${esc(f.game_name || "-")}</td><td class="num">${hrs(f.minutes)}</td><td>${f.kind === "day_over_20h" ? esc(f.when) : when(f.when)}</td><td class="muted">${esc(f.detail)}</td>
      <td class="num">${f.session_id ? `<button class="btn sm danger" data-sid="${esc(f.session_id)}" data-uid="${esc(f.user_id)}">Delete session</button>` : `<a class="btn sm" href="#user/${esc(f.user_id)}">Review</a>`}</td></tr>`).join("")}</tbody></table>`
    : `<div class="body empty">Nothing suspicious.</div>`}</section>`;
  $("#hd").addEventListener("change", e => { location.hash = "hours/" + e.target.value; });
  view.onclick = e => { const b = e.target.closest("button[data-sid]"); if (b && confirm("Delete this session?")) act(b, async () => { await api(`/admin/users/${b.dataset.uid}/sessions/${b.dataset.sid}`, { method: "DELETE" }); hoursReview(days); }); };
}

/* ---------- 5. server ---------- */
async function server() {
  const draw = async () => {
    const s = await api("/admin/server");
    const m = s.memory, dk = s.disk || {};
    const memUsed = m.total - m.available, up = n => n > 86400 ? Math.floor(n / 86400) + "d " + Math.floor(n % 86400 / 3600) + "h" : Math.floor(n / 3600) + "h " + Math.floor(n % 3600 / 60) + "m";
    const card = (l, v, sub, p) => `<div class="card"><span>${l}</span><b>${v}</b>${sub ? `<small>${sub}</small>` : ""}${p != null ? `<div class="bar"><i class="${p > 85 ? "hot" : ""}" style="width:${p}%"></i></div>` : ""}</div>`;
    view.innerHTML = `<section class="panel"><h2>Server <span class="more">checked ${when(s.checked_at)} · refreshes every 15 s</span></h2><div class="cards">
      ${card("Online now", s.online.users, s.online.playing + " playing · " + s.online.sockets + " connections")}
      ${card("Accounts", s.accounts.total, s.accounts.banned ? s.accounts.banned + " banned" : "")}
      ${card("Sign-ups", s.accounts.signups_24h, "24 h · " + s.accounts.signups_7d + " in 7 days · " + s.accounts.signups_30d + " in 30")}
      ${card("Play, last 24 h", hrs(s.sessions_24h.minutes), s.sessions_24h.count + " sessions")}
      ${card("Pending approvals", s.pending_approvals, "")}
      ${card("CPU", (s.cpu.percent ?? "-") + "%", "load " + s.cpu.load.join(" / ") + " · " + s.cpu.cores + " core" + (s.cpu.cores > 1 ? "s" : ""), Math.min(100, s.cpu.percent || 0))}
      ${card("Memory", bytes(memUsed), "of " + bytes(m.total) + (m.swap_total ? " · swap " + bytes(m.swap_total - m.swap_free) : ""), pct(memUsed, m.total))}
      ${card("Disk", bytes(dk.used), "of " + bytes(dk.total) + " · database " + bytes(s.database_bytes), pct(dk.used, dk.total))}
      ${card("Version", esc(s.version), "server up " + up(s.process_uptime_s) + " · system " + up(s.system_uptime_s))}
    </div></section>
    <div class="split" style="margin-top:14px"><section class="panel"><h2>Recent server warnings and errors (48 h)</h2>${s.errors == null ? `<div class="body empty">The journal isn't readable by the server.</div>` : s.errors.length ? `<pre class="log">${esc(s.errors.map(l => l.replace(/ \S+ fraglink-server\[\d+\]: \S+Z /, "  ")).join("\n"))}</pre>` : `<div class="body empty">None.</div>`}</section>
    <section class="panel"><h2>Services</h2>${s.services.map(v => `<div class="fr"><i class="dot ${v.state === "active" ? "online" : "away"}"></i><div class="who mono">${esc(v.name)}</div><span class="${v.state === "active" ? "" : "pill bad"}">${esc(v.state)}</span></div>`).join("")}</section></div>`;
  };
  await draw();
  timer = setInterval(() => { if (document.visibilityState === "visible") draw().catch(() => {}); }, 15000);
}

/* ---------- site settings (feature switches) ---------- */
async function siteSettings() {
  const edit = !!can.settings_edit;
  const [d, al] = await Promise.all([api("/admin/settings"), edit ? api("/admin/alerts").catch(() => null) : null]);
  const row = (s, kind) => `<tr><td style="width:56px"><label class="switch"><input type="checkbox" data-${kind}="${esc(s.key)}" ${s.enabled ? "checked" : ""} ${edit ? "" : "disabled"}><i></i></label></td>
    <td><b>${esc(s.label)}</b>${s.key === "maintenance" && s.enabled ? ` <span class="pill bad">ON</span>` : ""}${s.default !== undefined && s.enabled !== s.default ? ` <span class="pill">changed</span>` : ""}<div class="dimt">${esc(s.help)}</div></td>
    <td class="dimt" style="white-space:nowrap">${s.updated_at ? `${when(s.updated_at)}${s.updated_by ? " by " + esc(s.updated_by) : ""}` : "default"}</td></tr>`;
  const alerts = al ? `<section class="panel"><h2>Email alerts</h2><div class="body">
    <p class="note">Staff alerts by email, using the site's sending address. To keep within Cloudflare's 200 emails a day, alerts are batched (see each one) and capped at ${al.cap} a day. Sent in the last 24 hours: <b>${al.sent_24h}</b> of ${al.cap}.${al.mail_live ? "" : ` <span class="pill bad">Email sending is off on the server</span>`}</p>
    <form class="f" id="al-to" style="max-width:560px"><label>Send alerts to<div class="row"><input class="in" type="email" name="to" value="${esc(al.recipient)}" required maxlength="200" style="flex:1"><button class="btn">Save</button></div></label>
      <div class="dimt">Default: ${esc(al.default_recipient)}</div><div class="alert" id="al-msg" hidden></div></form>
    <table class="settings"><tbody>${al.alerts.map(a => row(a, "alert")).join("")}</tbody></table>
    <div class="row" style="margin-top:12px"><button class="btn" id="al-test">Send a test alert</button><span class="dimt">Sends today's summary marked [Test] to the address above. Counts toward the daily cap.</span></div>
    ${al.recent.length ? `<h3 style="margin:16px 0 6px">Recent alerts</h3><table class="t"><tbody>${al.recent.map(r => `<tr><td class="dimt" style="white-space:nowrap">${when(r.at)}</td><td>${esc(r.subject)}</td><td>${r.ok ? `<span class="pill ok">sent</span>` : `<span class="pill warn" title="${esc(r.error || "")}">not sent</span>`}</td></tr>`).join("")}</tbody></table>` : ""}
    </div></section>` : "";
  view.innerHTML = `<section class="panel"><h2>Site settings</h2><div class="body">
    <p class="note">Switches apply to the website and the desktop app within about 30 seconds, and the server enforces them (hiding a button is not the only effect). Every change is written to the audit log.${edit ? "" : " Only admins can change them."}</p>
    <table class="settings"><tbody>${d.settings.map(s => row(s, "key")).join("")}</tbody></table></div></section>${alerts}`;
  view.onchange = async e => {
    const a = e.target.closest("input[data-alert]");
    if (a) {
      const on = a.checked; a.disabled = true;
      try { await api("/admin/alerts", { method: "POST", body: { key: a.dataset.alert, enabled: on } }); siteSettings(); }
      catch (x) { a.checked = !on; a.disabled = false; alert(x.message); }
      return;
    }
    const c = e.target.closest("input[data-key]"); if (!c) return;
    const on = c.checked, key = c.dataset.key;
    if (key === "maintenance" && on && !confirm("Turn on maintenance mode? Everyone except staff will see a maintenance page and the app will stop syncing.")) { c.checked = false; return; }
    c.disabled = true;
    try { await api("/admin/settings", { method: "POST", body: { key, enabled: on } }); siteSettings(); }
    catch (x) { c.checked = !on; c.disabled = false; alert(x.message); }
  };
  const f = $("#al-to");
  if (f) f.addEventListener("submit", async e => {
    e.preventDefault();
    try { await api("/admin/alerts", { method: "POST", body: { recipient: f.to.value.trim() } }); await siteSettings(); msg($("#al-msg"), "Saved.", true); }
    catch (x) { msg($("#al-msg"), x.message); }
  });
  const t = $("#al-test");
  if (t) t.addEventListener("click", () => act(t, async () => {
    const r = await api("/admin/alerts/test", { method: "POST" });
    await siteSettings(); alert(r.test_sent ? "Test alert sent to " + r.recipient + "." : "Not sent: " + ((r.recent[0] && r.recent[0].error) || "unknown reason"));
  }));
}

/* ---------- 6. announcements ---------- */
async function announcements() {
  const d = await api("/admin/announcements");
  const a = d.active;
  view.innerHTML = `<div class="split"><section class="panel"><h2>Announcement</h2><div class="body">
    ${a ? `<p class="muted">Showing now:</p><div class="announce ${esc(a.style)}" style="border:1px solid;border-radius:3px"><div class="wrap" style="padding:6px 10px"><span>${esc(a.message)}${a.link ? ` <a href="${esc(a.link)}">More</a>` : ""}</span></div></div>
      <p class="dimt">${a.ends_at ? "Ends " + when(a.ends_at) : "No end time"}</p><button class="btn danger" id="clr">Clear announcement</button>` : `<p class="empty">No announcement is showing.</p>`}
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    <p class="note">Where it shows: <b>(1)</b> a banner at the top of every website page, <b>(2)</b> in the desktop app as a banner under the header within about 2 minutes for everyone signed in (2 minutes from the next app build; the current build checks about every 15 minutes; clicking it opens the link). It disappears everywhere at the end time or when you clear it. It no longer goes into the home page News box; longer updates go in <a href="#news">News</a>.</p>
    <form class="f" id="af"><label>Message<input type="text" name="message" maxlength="300" required placeholder="Servers restart at 10 pm AEDT for an update."></label>
      <div class="row"><label>Style<select class="in" name="style"><option value="info">Info (blue)</option><option value="warning">Warning (amber)</option></select></label>
      <label>Ends (optional)<input class="in" type="datetime-local" name="ends"></label></div>
      <label>Link (optional)<input type="text" name="link" placeholder="https://discord.gg/… or /download"></label>
      <div class="row"><button class="btn dl">Publish</button><span class="dimt">Replaces the current announcement.</span></div>
      <div class="alert" id="anmsg" hidden></div></form></div></section>
    <section class="panel"><h2>Recent</h2>${d.recent.length ? d.recent.map(r => `<div class="fr"><div class="who"><div class="nm"><span>${esc(r.message)}</span></div><div class="gm"><span class="pill ${r.style === "warning" ? "warn" : ""}">${esc(r.style)}</span> ${when(r.created_at)}${r.cleared_at ? " · cleared " + when(r.cleared_at) : r.ends_at ? " · ends " + when(r.ends_at) : ""}</div></div></div>`).join("") : `<div class="body empty">None yet.</div>`}</section></div>`;
  const clr = $("#clr");
  if (clr) clr.addEventListener("click", () => act(clr, async () => { await api("/admin/announcements/clear", { method: "POST" }); announcements(); }));
  $("#af").addEventListener("submit", async e => {
    e.preventDefault(); const f = e.target;
    try {
      await api("/admin/announcements", { method: "POST", body: { message: f.message.value.trim(), style: f.style.value, link: f.link.value.trim() || null, ends_at: f.ends.value ? new Date(f.ends.value).toISOString() : null } });
      announcements();
    } catch (x) { msg($("#anmsg"), x.message); }
  });
}

/* ---------- 6b. news ---------- */
/* Posts for the home page News box, /news and /news/<slug>. Separate from the one-line
   announcement banner; "Also show as banner" publishes a banner that links to the post. */
async function news(arg) {
  const PER = 20; let pg = 1, q = "";
  view.innerHTML = `<div class="split"><section class="panel"><h2 id="nf-h">New post</h2><div class="body">
    <p class="note">News posts show in the <b>News</b> box on the home page (latest 4, pinned first), on <a href="/news" target="_blank">/news</a>, and each has its own page. For a short alert on every page and in the app, use <a href="#announcements">Announcements</a>, or tick “Also show as banner” below.</p>
    <form class="f" id="nf"><input type="hidden" name="id">
      <label>Title<input type="text" name="title" maxlength="140" required placeholder="Fraglink 0.3.0 beta 1 is out"></label>
      <label>Post<textarea class="in" name="body" rows="10" maxlength="20000" placeholder="Blank line = new paragraph. https:// links become clickable."></textarea></label>
      <label>Link (optional)<input type="text" name="link" placeholder="https://discord.gg/… or /download"></label>
      <div class="row"><label>Date<input class="in" type="datetime-local" name="at"></label>
        <label style="display:flex;flex-direction:row;align-items:center;gap:6px;align-self:flex-end;padding-bottom:8px"><input type="checkbox" name="pinned"> Pin to top</label>
        <label style="display:flex;flex-direction:row;align-items:center;gap:6px;align-self:flex-end;padding-bottom:8px" id="nf-ban"><input type="checkbox" name="banner"> Also show as banner</label></div>
      <p class="dimt">Leave the date empty to publish now. A future date keeps the post hidden until then.</p>
      <div class="row"><button class="btn dl" id="nf-go">Publish</button><button type="button" class="btn" id="nf-cancel" hidden>Cancel edit</button></div>
      <div class="alert" id="nfmsg" hidden></div></form></div></section>
    <section class="panel"><h2>Posts <span class="dimt" id="n-count"></span></h2><div class="body" style="padding-bottom:4px"><input class="in" type="search" id="n-q" placeholder="Search posts" style="width:100%"></div><div id="n-list"></div></section></div>`;
  const f = $("#nf"), local = iso => { const d = new Date(iso); d.setMinutes(d.getMinutes() - d.getTimezoneOffset()); return d.toISOString().slice(0, 16); };
  let items = [];
  const reset = () => { f.reset(); f.id.value = ""; $("#nf-h").textContent = "New post"; $("#nf-go").textContent = "Publish"; $("#nf-cancel").hidden = true; $("#nf-ban").hidden = false; };
  const fill = p => {
    f.id.value = p.id; f.title.value = p.title; f.body.value = p.body || ""; f.link.value = p.link || ""; f.at.value = local(p.at); f.pinned.checked = p.pinned;
    $("#nf-h").textContent = "Edit post"; $("#nf-go").textContent = "Save changes"; $("#nf-cancel").hidden = false; $("#nf-ban").hidden = true; f.title.focus(); window.scrollTo(0, 0);
  };
  const load = async () => {
    const d = await api(`/admin/news?page=${pg}&per=${PER}${q ? "&q=" + encodeURIComponent(q) : ""}`);
    items = d.items; $("#n-count").textContent = d.total ? `(${d.total.toLocaleString("en")})` : "";
    $("#n-list").innerHTML = items.length ? `<table class="t newsadm"><tbody>${items.map(p => `<tr><td class="t"><b><a href="${esc(p.url)}" target="_blank">${esc(p.title)}</a></b><small>${day(p.at)}${p.scheduled ? ' · <span class="pill warn">scheduled</span>' : ""}${p.pinned ? ' · <span class="pin">Pinned</span>' : ""}</small></td>
      <td class="a"><button class="btn sm" data-pin="${p.id}">${p.pinned ? "Unpin" : "Pin"}</button> <button class="btn sm" data-edit="${p.id}">Edit</button> <button class="btn sm danger" data-del="${p.id}">Delete</button></td></tr>`).join("")}</tbody></table>
      ${d.pages > 1 ? `<div class="pager"><button class="btn sm" data-pg="${pg - 1}" ${pg > 1 ? "" : "disabled"}>‹ Prev</button><span>Page ${pg} of ${d.pages}</span><button class="btn sm" data-pg="${pg + 1}" ${pg < d.pages ? "" : "disabled"}>Next ›</button></div>` : ""}`
      : `<p class="empty" style="padding:10px 16px">${q ? "No posts match." : "No posts yet. Write the first one on the left."}</p>`;
  };
  let t = null;
  $("#n-q").addEventListener("input", e => { clearTimeout(t); t = setTimeout(() => { q = e.target.value.trim(); pg = 1; load(); }, 250); });
  $("#nf-cancel").addEventListener("click", reset);
  $("#n-list").addEventListener("click", e => {
    const b = e.target.closest("button"); if (!b) return;
    const p = items.find(x => String(x.id) === (b.dataset.pin || b.dataset.edit || b.dataset.del));
    if (b.dataset.pg) { pg = Number(b.dataset.pg); load(); }
    else if (b.dataset.edit && p) fill(p);
    else if (b.dataset.pin && p) act(b, async () => { await api(`/admin/news/${p.id}`, { method: "POST", body: { pinned: !p.pinned } }); load(); });
    else if (b.dataset.del && p && confirm(`Delete “${p.title}”? This can't be undone (the audit log keeps a copy).`)) act(b, async () => { await api(`/admin/news/${p.id}`, { method: "DELETE" }); if (f.id.value === String(p.id)) reset(); load(); });
  });
  f.addEventListener("submit", async e => {
    e.preventDefault();
    const body = { title: f.title.value.trim(), body: f.body.value, link: f.link.value.trim(), pinned: f.pinned.checked };
    if (f.at.value) body.published_at = new Date(f.at.value).toISOString();
    try {
      const r = f.id.value ? await api(`/admin/news/${f.id.value}`, { method: "POST", body }) : await api("/admin/news", { method: "POST", body });
      if (!f.id.value && f.banner.checked && can.announcements) await api("/admin/announcements", { method: "POST", body: { message: r.post.title.slice(0, 300), style: "info", link: r.post.url } });
      const was = f.id.value; reset(); pg = 1; await load(); msg($("#nfmsg"), was ? "Saved." : "Published.", true);
    } catch (x) { msg($("#nfmsg"), x.message); }
  });
  await load();
}

/* ---------- 7. staff ---------- */
async function staff() {
  const [d, log] = await Promise.all([api("/admin/staff"), api("/admin/audit?limit=100")]);
  const owner = who.role === "owner";
  const roleOpts = owner ? ["admin", "moderator"] : ["moderator"];
  const short = v => v == null ? "" : esc(JSON.stringify(v)).slice(0, 300);
  view.innerHTML = `<div class="split"><div class="col"><section class="panel"><h2>Staff</h2>
    <table><thead><tr><th>Username</th><th>Role</th><th>Member since</th><th class="num"></th></tr></thead><tbody>
    ${d.staff.map(s => {
      const canEdit = s.role !== "owner" && s.handle !== who.handle && (owner || s.role === "moderator");
      return `<tr data-h="${esc(s.handle)}"><td><a href="#user/${esc(s.id)}">${esc(s.handle)}</a></td><td>${rolePill(s.role)}</td><td>${day(s.member_since)}</td>
        <td class="num"><div class="acts">${canEdit ? `${owner ? `<select class="in" data-role>${["admin", "moderator"].map(r => `<option ${r === s.role ? "selected" : ""}>${r}</option>`).join("")}</select>` : ""}<button class="btn sm danger" data-rm>Remove</button>` : `<span class="dimt">${s.role === "owner" ? "owner" : ""}</span>`}</div></td></tr>`;
    }).join("")}</tbody></table>
    <form class="tools" id="sf"><input class="in grow" name="handle" placeholder="Username" required><select class="in" name="role">${roleOpts.map(r => `<option>${r}</option>`).join("")}</select><button class="btn">Add to staff</button></form>
    <div class="note">Owner: everything, and the only one who adds or removes admins. Admin: everything except managing admins. Moderator: game approvals, user ban/unban and hours review.</div>
    <div class="alert" id="smsg" hidden style="margin:0 12px 12px"></div></section>
    ${owner ? `<section class="panel"><h2>Transfer ownership</h2><div class="body"><form class="f" id="tf"><label>New owner's username<input type="text" name="handle" required></label>
      <label>Type your own username to confirm<input type="text" name="confirm" required></label><div class="row"><button class="btn danger">Transfer</button><span class="dimt">You become an admin.</span></div><div class="alert" id="tmsg" hidden></div></form></div></section>` : ""}
    </div>
    <section class="panel"><h2>Audit log</h2>${log.entries.length ? log.entries.map(x => `<div class="fr"><div class="who"><div class="nm"><span><b>${esc(x.actor)}</b> ${esc(x.action)} <span class="muted">${esc(x.target || "")}</span></span><small>${when(x.at)}</small></div>
      ${x.before != null || x.after != null ? `<div class="diff">${x.before != null ? short(x.before) : "-"} → ${x.after != null ? short(x.after) : "-"}</div>` : ""}</div></div>`).join("") : `<div class="body empty">Nothing yet.</div>`}</section></div>`;
  const smsg = $("#smsg");
  $("#sf").addEventListener("submit", async e => {
    e.preventDefault(); const f = e.target;
    try { await api("/admin/staff", { method: "POST", body: { handle: f.handle.value.trim(), role: f.role.value } }); staff(); } catch (x) { msg(smsg, x.message); }
  });
  view.onclick = async e => {
    const b = e.target.closest("button[data-rm]"); if (!b) return;
    const h = b.closest("tr").dataset.h;
    if (confirm("Remove " + h + " from staff?")) await act(b, async () => { await api("/admin/staff", { method: "POST", body: { handle: h, role: "none" } }); staff(); });
  };
  view.onchange = async e => {
    const s = e.target.closest("select[data-role]"); if (!s) return;
    const h = s.closest("tr").dataset.h;
    try { await api("/admin/staff", { method: "POST", body: { handle: h, role: s.value } }); staff(); } catch (x) { msg(smsg, x.message); }
  };
  const tf = $("#tf");
  if (tf) tf.addEventListener("submit", async e => {
    e.preventDefault();
    if (!confirm("Hand ownership to " + tf.handle.value.trim() + "? You'll become an admin.")) return;
    try { await api("/admin/staff/transfer", { method: "POST", body: { handle: tf.handle.value.trim(), confirm: tf.confirm.value.trim() } }); location.reload(); } catch (x) { msg($("#tmsg"), x.message); }
  });
}

// Edit game > Merge into…: preview what moves, then a typed confirm.
function wireMerge(id) {
  const q = $("#mrgq"); if (!q) return;
  const gid = "/admin/games/" + encodeURIComponent(id), out = $("#mrgout"), m = $("#mrgmsg");
  let tm;
  q.addEventListener("input", () => { clearTimeout(tm); tm = setTimeout(async () => {
    const v = q.value.trim(); if (v.length < 2) return;
    const d = await api("/admin/games?q=" + encodeURIComponent(v)).catch(() => ({ games: [] }));
    $("#mrg-games").innerHTML = d.games.filter(x => x.id !== id).map(x => `<option value="${esc(x.id)}">${esc(x.public_name || x.name)}</option>`).join("");
  }, 250); });
  const row = (label, t) => `<tr><td>${esc(label)}</td><td class="num">${t.players}</td><td class="num">${t.sessions}</td><td class="num">${hrs(t.tracked_min)}</td><td class="num">${hrs(t.imported_min)}</td><td class="num">${t.hidden_by}</td></tr>`;
  $("#mrgp").addEventListener("click", e => act(e.target, async () => {
    const into = q.value.trim(); if (!into) throw new Error("Pick the game to merge into.");
    const p = await api(gid + "/merge?into=" + encodeURIComponent(into));
    out.innerHTML = `<table class="mrgt"><thead><tr><th></th><th class="num">Players</th><th class="num">Sessions</th><th class="num">Tracked</th><th class="num">Imported</th><th class="num">Hidden by</th></tr></thead><tbody>
      ${row("Moves from " + (p.from.name || id), p.from.totals)}${row("Into " + (p.into.name || into) + " (now)", p.into.totals)}</tbody></table>
      <div class="dimt">Everything above moves to <b>${esc(p.into.name || into)}</b> <span class="mono">${esc(into)}</span>. This entry is retired, and apps that still report <span class="mono">${esc(id)}</span> count for the target from now on. Logged in the audit log.</div>
      <div class="row"><button class="btn danger" type="button" id="mrgdo">Merge…</button></div>`;
    $("#mrgdo").addEventListener("click", ev => {
      const typed = prompt(`Merge "${p.from.name || id}" into "${p.into.name || into}"? This can't be undone.\n\nType the id of the game being merged to confirm:\n${id}`);
      if (typed === null) return;
      if (typed.trim() !== id) return msg(m, "The id didn't match; nothing was merged.");
      act(ev.target, async () => { await api(gid + "/merge", { method: "POST", body: { into, confirm: typed.trim() } }); listGames($("#gs").q.value.trim()); editGame(into); }).catch(x => msg(m, x.message));
    });
  }).catch(x => msg(m, x.message)));
}

// Edit game > Pictures: pick where the icon and the banner art come from.
async function renderMedia(id, g) {
  const box = $("#media"), gid = "/admin/games/" + encodeURIComponent(id), t = "?t=" + Date.now();
  let m;
  try { m = await api(gid + "/media"); } catch (x) { box.innerHTML = `<div class="alert">${esc(x.message)}</div>`; return; }
  const own = { icon: g.icon64 + t, art: `/api/v1/game-art/${encodeURIComponent(id)}.jpg${t}` };
  const ph = k => `<span class="mtile-none ${k}">none</span>`;
  const img = (k, src) => src ? `<img class="${k}" src="${esc(src)}" alt="" onerror="this.outerHTML='<span class=&quot;mtile-none ${k}&quot;>none</span>'">` : ph(k);
  const tile = (k, v, label, src, note) => {
    const on = m[k].pref === v, dis = v !== "auto" && v !== "custom" && !src;
    return `<label class="mtile${on ? " on" : ""}${dis ? " dis" : ""}"><input type="radio" name="${k}src" value="${v}" ${on ? "checked" : ""} ${dis ? "disabled" : ""}>${img(k, src)}<b>${label}</b><small>${note}</small></label>`;
  };
  const block = (k, title, help, accept) => `<div class="f mpick" data-k="${k}"><label>${title} <span class="dimt">${help}</span></label>
    <div class="mtiles">
      ${tile(k, "auto", "Automatic", own[k], "Best available")}
      ${tile(k, "games_list", "Games list", m[k].games_list, m[k].games_list ? "From the games list" : "Not in games list")}
      ${tile(k, "steam", "Steam", m[k].steam, m[k].steam ? "Steam app " + m.steam_appid : "No Steam app id")}
      ${tile(k, "custom", "Link / Upload", m[k].custom ? (k === "icon" ? own.icon : own.art) : "", m[k].custom ? "Your picture" : "Add one below")}
    </div>
    <div class="row"><input type="text" class="in" data-url placeholder="https://… link to a picture" style="flex:1"><button class="btn" type="button" data-link>Use link</button>
      <input type="file" data-file accept="${accept}"><button class="btn" type="button" data-up>Upload</button></div>
    <div class="alert" data-msg hidden></div></div>`;
  box.innerHTML = `${block("icon", "Icon", "(shown at 16, 32 and 64 px; PNG or JPEG up to 1 MB)", "image/png,image/jpeg")}
    ${block("art", "Banner art", "(cropped to 460×215; up to 2 MB)", "image/png,image/jpeg,image/webp,image/gif")}
    <div class="f"><label>Steam app id <span class="dimt">(used for Steam pictures)</span></label>
      <div class="row"><input type="number" class="in" id="msteam" min="0" value="${m.steam_appid_set || ""}" placeholder="${m.steam_appid ? "auto: " + m.steam_appid : "none found"}" style="width:180px">
      ${m.steam_suggest ? `<button class="btn" type="button" id="msug">Use ${m.steam_suggest.appid} (${esc(m.steam_suggest.name)})</button>` : ""}
      ${m.steam_appid ? `<a class="lnk" href="https://store.steampowered.com/app/${m.steam_appid}" target="_blank" rel="noopener">Steam page</a>` : ""}</div>
      <small>Leave empty to find it automatically. A suggestion from a Steam search shows when none was found.</small></div>
    <div class="row"><button class="btn dl" type="button" id="msave">Save pictures</button><span class="dimt">Automatic picks the best available source.</span></div>
    <div class="alert" id="mmsg" hidden></div>`;
  const sug = $("#msug", box);
  if (sug) sug.addEventListener("click", () => { $("#msteam").value = m.steam_suggest.appid; });
  $("#msave", box).addEventListener("click", e => act(e.target, async () => {
    const v = k => (box.querySelector(`input[name=${k}src]:checked`) || {}).value || "auto";
    if (v("icon") === "custom" && !m.icon.custom) throw new Error("Add an icon by link or upload first.");
    if (v("art") === "custom" && !m.art.custom) throw new Error("Add banner art by link or upload first.");
    const sid = $("#msteam").value.trim();
    await api(gid + "/media", { method: "POST", body: { icon: v("icon"), art: v("art"), steam_appid: sid ? +sid : 0 } });
    renderMedia(id, g); listGames($("#gs").q.value.trim());
  }).catch(x => msg($("#mmsg"), x.message)));
  box.querySelectorAll(".mpick").forEach(p => {
    const k = p.dataset.k, out = $("[data-msg]", p);
    const pick = async () => { await api(gid + "/media", { method: "POST", body: { [k]: "custom" } }); renderMedia(id, g); };
    $("[data-link]", p).addEventListener("click", e => act(e.target, async () => {
      await api(gid + (k === "icon" ? "/icon/url" : "/art/url"), { method: "POST", body: { url: $("[data-url]", p).value.trim() } }); await pick();
    }).catch(x => msg(out, x.message)));
    $("[data-up]", p).addEventListener("click", e => act(e.target, async () => {
      const file = $("[data-file]", p).files[0]; if (!file) throw new Error("Pick an image first.");
      const r = await fetch(FL.API + gid + (k === "icon" ? "/icon" : "/art"), { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": file.type || "application/octet-stream" }, body: file });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.message || "Upload failed (HTTP " + r.status + ").");
      await pick();
    }).catch(x => msg(out, x.message)));
  });
}

route();
if (can.approvals && !location.hash.startsWith("#approvals")) Promise.all([api("/admin/suggestions"), api("/admin/unknown_games").catch(() => ({ unknown: [] }))]).then(([d, u]) => badge("approvals", d.suggestions.length + u.unknown.length)).catch(() => {});
};
