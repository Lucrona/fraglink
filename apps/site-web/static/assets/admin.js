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
  const fn = { approvals, games, users, hours: hoursReview, avatars: avatarReports, server, announcements, staff, user: userDetail }[tab];
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
  if (!decided) badge("approvals", rows.length);
  view.innerHTML = `<section class="panel"><h2>${decided ? "Decided" : "Pending"} game suggestions <span class="more"><a href="#" data-flip>${decided ? "Show pending" : "Show decided"}</a></span></h2>
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
  view.onclick = async e => {
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
    <form class="tools" id="gs"><input class="in grow" name="q" value="${esc(q)}" placeholder="Search by name, id or exe" autocomplete="off"></form>
    <div id="glist"><div class="body empty">Loading…</div></div></section>
    <section class="panel" id="gedit"><h2>Edit game</h2><div class="body empty">Pick a game on the left.</div></section></div>`;
  $("#gs").addEventListener("submit", e => { e.preventDefault(); history.replaceState(null, "", "#games/" + encodeURIComponent(e.target.q.value.trim())); listGames(e.target.q.value.trim()); });
  liveSearch($("#gs"));
  listGames(q);
}
/* Search as you type (debounced); Enter still works. */
function liveSearch(form) {
  let t; const i = form.q; i.type = "search";
  i.addEventListener("input", () => { clearTimeout(t); t = setTimeout(() => form.requestSubmit(), 300); });
}
async function listGames(q) {
  const box = $("#glist");
  const d = await api("/admin/games?q=" + encodeURIComponent(q));
  if (!d.games.length) { box.innerHTML = `<div class="body empty">No games match.</div>`; return; }
  box.innerHTML = `<div class="note">${q ? "Matches" : "Edited games, then the most played in the last 30 days."}</div>
    <table><thead><tr><th></th><th>Name</th><th>Id</th><th>Source</th><th class="num">Players</th><th class="num">Hours</th></tr></thead><tbody>
    ${d.games.map(g => {
      const o = g.override || {};
      const flags = (o.name ? `<span class="pill warn">renamed</span>` : "") + (o.hidden ? `<span class="pill bad">hidden</span>` : "") +
        (o.has_icon ? `<span class="pill">icon</span>` : "") + ((o.extra_exes || []).length ? `<span class="pill">+${o.extra_exes.length} exe</span>` : "") +
        (g.retired ? `<span class="pill bad">${g.retired.partial ? "exe retired" : "retired"}</span>` : "") +
        (g.status && g.status.lifecycle !== "live" ? `<span class="pill ${g.status.lifecycle === "ended" ? "bad" : "warn"}">${esc(LIFE[g.status.lifecycle] || g.status.lifecycle)}</span>` : "");
      return `<tr class="click" data-id="${esc(g.id)}"><td class="ic"><img src="${esc(g.icon)}" width="20" height="20" alt="" onerror="this.style.visibility='hidden'"></td>
        <td>${esc(g.public_name || g.name)}${flags}</td><td class="mono dimt">${esc(g.id)}</td><td class="muted">${esc(g.source)}</td>
        <td class="num">${g.players}</td><td class="num">${hrs(g.tracked_min)}</td></tr>`;
    }).join("")}</tbody></table>`;
  box.onclick = e => { const tr = e.target.closest("tr[data-id]"); if (tr) editGame(tr.dataset.id); };
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
  if (r) return `<div class="f"><label>Retired</label><div>${r.partial ? `Detached ${r.exes.map(e => `<span class="mono">${esc(e)}</span>`).join(", ")} on ${day(r.at)}; the game stays live.` : `Retired ${day(r.at)}. New play goes to <a href="#" data-goto="${esc(r.successor)}" class="mono">${esc(r.successor)}</a>.`}</div>
    <div class="row"><button class="btn" id="gunret" type="button">Undo retire</button><small>Only while the new entry has no hours.</small></div><div class="alert" id="rmsg" hidden></div></div>`;
  return `<div class="f"><label>Retire</label>${g.retired_from ? `<div class="dimt">New entry for retired <a href="#" data-goto="${esc(g.retired_from)}" class="mono">${esc(g.retired_from)}</a>.</div>` : ""}
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
    <div><b>${esc(g.public_name || g.name || g.id)}</b> <span class="mono dimt">${esc(g.id)}</span><div class="dimt">From: ${esc(g.source)} · ${g.players} players · ${hrs(g.tracked_min)} tracked</div></div>
    <label>Display name override<input type="text" name="name" maxlength="80" value="${esc(o.name || "")}" placeholder="${esc(g.name)}"><small>Leave empty to use the list name.</small></label>
    <label class="check"><input type="checkbox" name="hidden" ${o.hidden ? "checked" : ""}> Hide from public (shown as "Playing a game")</label>
    <label>Extra exe names (one per line)<textarea class="in" name="exes" placeholder="GameClient-Win64.exe">${esc((o.extra_exes || []).join("\n"))}</textarea><small>Apps detect these as this game (picked up within a day or on restart).</small></label>
    <div class="row"><button class="btn dl">Save</button><span class="dimt">Edits stay in place when the games list refreshes.</span></div>
    <div class="alert" id="gmsg" hidden></div></form>
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    <div class="f"><label>Icon</label>
      <div class="icons-prev"><img src="${esc(g.icon64.replace("/64/", "/16/"))}${bust}" width="16" height="16" alt=""><img src="${esc(g.icon)}${bust}" width="32" height="32" alt=""><img src="${esc(g.icon64)}${bust}" width="64" height="64" alt=""></div>
      <div class="row"><input type="file" id="gicon" accept="image/png,image/jpeg"><button class="btn" id="gup" type="button">Upload</button>${o.has_icon ? `<button class="btn danger" id="gdel" type="button">Remove uploaded icon</button>` : ""}</div>
      <small>PNG or JPEG up to 1 MB, square works best. Stored on our server and resized to 16, 32 and 64 px. Browsers may keep the old icon for a while.</small>
      <div class="alert" id="imsg" hidden></div></div>
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    <div class="f"><label>Banner art <span class="dimt">(now: ${esc({ override: "uploaded here", steam: "Steam store header", discord: "Discord app directory cover", none: "none: gradient with icon" }[g.art_source] || g.art_source || "?")})</span></label>
      <img id="gartimg" src="/api/v1/game-art/${encodeURIComponent(id)}.jpg${bust}" width="230" height="108" alt="" style="border:1px solid var(--line);border-radius:3px;object-fit:cover" onerror="this.replaceWith(Object.assign(document.createElement('div'),{className:'dimt',textContent:'No art found.'}))">
      <div class="row"><input type="file" id="gart" accept="image/png,image/jpeg,image/webp,image/gif"><button class="btn" id="gartup" type="button">Upload</button></div>
      <div class="row"><input type="text" class="in" id="garturl" placeholder="https://… link to a picture" style="flex:1"><button class="btn" id="garturlb" type="button">Use link</button>${g.art_source === "override" ? `<button class="btn danger" id="gartdel" type="button">Remove override</button>` : ""}</div>
      <small>Any size; cropped to 460×215 (Steam header shape), up to 2 MB.</small>
      <div class="alert" id="amsg" hidden></div></div>
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    ${statusHtml(g)}
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">${retireHtml(g)}</div>`;
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
  $("#gup").addEventListener("click", async () => {
    const file = $("#gicon").files[0]; if (!file) return msg($("#imsg"), "Pick an image first.");
    try {
      const r = await fetch(FL.API + "/admin/games/" + encodeURIComponent(id) + "/icon", { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": file.type || "application/octet-stream" }, body: file });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.message || "Upload failed (HTTP " + r.status + ").");
      editGame(id); listGames($("#gs").q.value.trim());
    } catch (x) { msg($("#imsg"), x.message); }
  });
  const del = $("#gdel");
  if (del) del.addEventListener("click", () => act(del, async () => { await api("/admin/games/" + encodeURIComponent(id) + "/icon", { method: "DELETE" }); editGame(id); }));
  $("#gartup").addEventListener("click", async () => {
    const file = $("#gart").files[0]; if (!file) return msg($("#amsg"), "Pick an image first.");
    try {
      const r = await fetch(FL.API + gid + "/art", { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": file.type || "application/octet-stream" }, body: file });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.message || "Upload failed (HTTP " + r.status + ").");
      editGame(id);
    } catch (x) { msg($("#amsg"), x.message); }
  });
  $("#garturlb").addEventListener("click", () => act($("#garturlb"), async () => { await api(gid + "/art/url", { method: "POST", body: { url: $("#garturl").value.trim() } }); editGame(id); }).catch(x => msg($("#amsg"), x.message)));
  const adel = $("#gartdel");
  if (adel) adel.addEventListener("click", () => act(adel, async () => { await api(gid + "/art", { method: "DELETE" }); editGame(id); }).catch(x => msg($("#amsg"), x.message)));
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
  $("#ulist").innerHTML = d.users.length ? `<div class="note">${q ? "Matches" : "Newest accounts"}</div><table><thead><tr><th>Username</th><th>Display name</th><th>Email</th><th>Joined</th><th>Status</th></tr></thead><tbody>
    ${d.users.map(u => `<tr class="click" data-id="${esc(u.id)}"><td><a href="#user/${esc(u.id)}">${esc(u.handle)}</a>${rolePill(u.role)}${u.banned ? `<span class="pill bad">banned</span>` : ""}</td>
      <td>${esc(u.display_name)}</td><td class="muted">${esc(u.email || "-")}</td><td>${day(u.created_at)}</td>
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
    <section class="panel"><h2>${esc(u.handle)} ${rolePill(u.role)}${u.banned_at ? `<span class="pill bad">banned</span>` : ""}<span class="more"><a href="/u/${encodeURIComponent(u.handle)}">Public profile</a></span></h2><div class="body">
      <dl class="kv"><dt>Display name</dt><dd>${esc(u.display_name)}</dd><dt>Email</dt><dd>${esc(u.email || "-")}${u.email ? (u.email_verified ? " (verified)" : " (not verified)") : ""}</dd>
      <dt>Member since</dt><dd>${day(u.created_at)}</dd><dt>Status</dt><dd>${pres}${p.connected ? ` <span class="pill ok">connected</span>` : ""}</dd>
      <dt>Friends</dt><dd>${u.friend_count}</dd><dt>Shares activity</dt><dd>${u.share_activity ? "Yes" : "No"}</dd>
      ${u.linked.length ? `<dt>Linked</dt><dd>${u.linked.map(l => esc(l.provider) + " " + esc(l.display)).join(", ")}</dd>` : ""}
      ${u.banned_at ? `<dt>Banned</dt><dd>${when(u.banned_at)}${u.ban_reason ? " · " + esc(u.ban_reason) : ""}</dd>` : ""}
      <dt>User id</dt><dd class="mono dimt">${esc(u.id)}</dd></dl></div></section>
    <section class="panel"><h2>Games</h2>${u.games.length ? `<table><thead><tr><th></th><th>Game</th><th class="num">Past 7 days</th><th class="num">Total</th><th class="num"></th></tr></thead><tbody>
      ${u.games.map(g => `<tr data-game="${esc(g.game_id)}"><td class="ic">${icon(g.game_id, g.name)}</td><td>${esc(g.name)}${g.name === "Unlisted game" && g.stored_name ? ` <span class="dimt">(${esc(g.stored_name)})</span>` : ""}<div class="mono dimt">${esc(g.game_id)}</div></td>
        <td class="num">${hrs(g.minutes_7d)}</td><td class="num">${hrs(g.display_min)}${g.display_tier !== "tracked" ? `<span class="tier">${esc(g.display_tier)}</span>` : ""}</td>
        <td class="num"><div class="acts"><button class="btn sm" data-cap>Lower…</button><button class="btn sm danger" data-remove>Remove</button></div></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No hours.</div>`}</section>
    <section class="panel"><h2>Recent sessions</h2>${u.sessions.length ? `<table><thead><tr><th>Game</th><th>Started</th><th class="num">Length</th><th>Source</th><th></th></tr></thead><tbody>
      ${u.sessions.map(s => `<tr data-sid="${esc(s.id)}"><td>${esc(s.name)}</td><td>${when(s.started_at)}</td><td class="num">${hrs(s.duration_s / 60)}${s.duration_s > 18 * 3600 ? `<span class="pill bad">long</span>` : ""}</td><td class="muted">${esc(s.source)}</td><td class="num"><button class="btn sm danger" data-delsess>Delete</button></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No sessions.</div>`}</section>
  </div><div class="col">
    <section class="panel"><h2>Actions</h2><div class="body f">
      ${u.banned_at ? `<button class="btn" id="unban">Unban</button>` : `<label>Ban reason (optional, kept in the audit log)<input type="text" id="reason" maxlength="200"></label><div class="row"><button class="btn danger" id="ban">Ban account</button></div><small>Banned accounts are signed out everywhere, can't sign in, and their profile is hidden.</small>`}
      ${can.reset_password ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div class="row"><button class="btn" id="resetpw">Reset password</button></div><small>Makes a temporary password (shown once) and signs the account out everywhere. Send it to the user yourself; email isn't set up yet.</small><div id="pwout"></div>` : ""}
      ${can.change_handle ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><label>Username<input type="text" id="newhandle" maxlength="20" value="${esc(u.handle)}"></label><div class="row"><button class="btn" id="chhandle">Change username</button></div><small>Users can't change their username themselves. The old one keeps redirecting to them and can't be taken by anyone else. Logged in the audit log.</small>` : ""}
      <hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div class="row"><button class="btn" id="rmav">Remove picture</button>${can.mfa_reset ? ` <button class="btn" id="mfareset">Reset two-step sign-in</button>` : ""}</div><small>Removing a picture resets it to the default and blocks that exact image. Resetting two-step sign-in lets them sign in with just their password (check it's really them first).</small>
      <div class="alert" id="amsg" hidden></div></div></section>
    <section class="panel"><h2>Signed-in devices</h2>${u.devices.length ? u.devices.map(d => `<div class="fr"><div class="who"><div class="nm">${esc(d.name || (d.kind === "web" ? "Web browser" : "Fraglink desktop"))} <small>${esc(d.kind)}</small></div><div class="gm">signed in ${day(d.created_at)} · last used ${when(d.last_used_at)}</div></div></div>`).join("") : `<div class="body empty">None.</div>`}</section>
  </div></div>`;
  const amsg = $("#amsg");
  const ban = $("#ban"), unban = $("#unban"), reset = $("#resetpw");
  const chh = $("#chhandle");
  if (chh) chh.addEventListener("click", () => { const nh = $("#newhandle").value.trim(); if (nh && nh !== u.handle && confirm(`Change username ${u.handle} → ${nh}? Their profile address changes; the old one redirects.`)) act(chh, async () => { await api(`/admin/users/${id}/handle`, { method: "POST", body: { handle: nh } }); userDetail(id); }); });
  if (ban) ban.addEventListener("click", () => { if (confirm("Ban " + u.handle + "?")) act(ban, async () => { await api(`/admin/users/${id}/ban`, { method: "POST", body: { reason: $("#reason").value } }); userDetail(id); }).catch(x => msg(amsg, x.message)); });
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
      if (!confirm("Remove ALL hours for this game from " + u.handle + "? (tracked, imported, estimated and Steam)")) return;
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

/* ---------- 6. announcements ---------- */
async function announcements() {
  const d = await api("/admin/announcements");
  const a = d.active;
  view.innerHTML = `<div class="split"><section class="panel"><h2>Site banner</h2><div class="body">
    ${a ? `<p class="muted">Showing now:</p><div class="announce ${esc(a.style)}" style="border:1px solid;border-radius:3px"><div class="wrap" style="padding:6px 10px"><span>${esc(a.message)}${a.link ? ` <a href="${esc(a.link)}">More</a>` : ""}</span></div></div>
      <p class="dimt">${a.ends_at ? "Ends " + when(a.ends_at) : "No end time"}</p><button class="btn danger" id="clr">Clear banner</button>` : `<p class="empty">No banner is showing.</p>`}
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">
    <form class="f" id="af"><label>Message<input type="text" name="message" maxlength="300" required placeholder="Servers restart at 10 pm AEDT for an update."></label>
      <div class="row"><label>Style<select class="in" name="style"><option value="info">Info (blue)</option><option value="warning">Warning (amber)</option></select></label>
      <label>Ends (optional)<input class="in" type="datetime-local" name="ends"></label></div>
      <label>Link (optional)<input type="text" name="link" placeholder="https://discord.gg/… or /download"></label>
      <div class="row"><button class="btn dl">Publish</button><span class="dimt">Replaces the current banner. Shown on every page of the website; apps can read it from /api/v1/announcement.</span></div>
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

route();
if (can.approvals && !location.hash.startsWith("#approvals")) api("/admin/suggestions").then(d => badge("approvals", d.suggestions.length)).catch(() => {});
};
