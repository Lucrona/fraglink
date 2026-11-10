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
const th = t => (Number(t || 0) / 10).toFixed(1) + " h";
const hrs = min => (Math.round((min || 0) / 6) / 10).toLocaleString("en") + " h";
const bytes = b => b == null ? "-" : b > 1e9 ? (b / 1e9).toFixed(1) + " GB" : (b / 1e6).toFixed(0) + " MB";
const pct = (a, b) => b ? Math.round(a / b * 100) : 0;
const rolePill = r => r ? `<span class="pill ${esc(r)}">${esc(r)}</span>` : "";
function msg(el, text, ok) { el.className = "alert " + (ok ? "ok" : "err"); el.textContent = text; el.hidden = false; }
async function act(btn, fn) {
  if (btn) btn.disabled = true;
  try { return await fn(); } catch (e) { alert(e.message); } finally { if (btn) btn.disabled = false; }
}

/* Sidebar console: [route, label, allowed, group]. Groups keep their order. */
const TABS = [
  ["approvals", "Game approvals", can.approvals, "To do"],
  ["hours", "Hours review", can.hours, "To do"],
  ["hourscheck", "Hours checker", can.hours, "To do"],
  ["avatars", "Reported pictures", can.avatars, "To do"],
  ["games", "Games", can.games, "Games"],
  ["detection", "Detection rules", can.games, "Games"],
  ["suggested", "Suggested Merge", can.approvals || can.games, "Games"],
  ["detect", "Detection tester", can.games, "Games"],
  ["artwork", "Missing artwork", can.games, "Games"],
  ["checks", "Data checks", can.games, "Games"],
  ["bulk", "Bulk fixes", can.bulk, "Games"],
  ["users", "Users", can.users, "Community"],
  ["news", "News", can.news, "Community"],
  ["polls", "Polls", can.news, "Community"],
  ["changelog", "Changelog", can.changelog, "Community"],
  ["announcements", "Announcements", can.announcements, "Community"],
  ["incidents", "Status incidents", can.server, "Site"],
  ["server", "Server", can.server, "Site"],
  ["health", "Health", can.health, "Site"],
  ["settings", "Site settings", can.settings, "Site"],
  ["history", "Review history", can.audit, "Site"],
  ["undo", "Undo", can.audit, "Site"],
  ["staff", "Staff", can.staff, "Site"],
].filter(t => t[2]);
const GROUPS = [...new Set(TABS.map(t => t[3]))];

root.innerHTML = `<div class="wrap adm adm-console">
  <div class="adm-head"><h1>Admin</h1><span class="role">${esc(who.role)}</span><span class="dimt">Signed in as ${esc(who.handle)}. Every action is logged.</span></div>
  <div class="adm-lay"><nav class="adm-side" aria-label="Admin sections">${GROUPS.map(g => `<h3>${esc(g)}</h3>` + TABS.filter(t => t[3] === g).map(t => `<a href="#${t[0]}" data-tab="${t[0]}">${esc(t[1])}<span class="n" hidden></span></a>`).join("")).join("")}</nav>
  <div id="view"></div></div></div>`;
const view = $("#view");
let timer = null;

function route() {
  clearInterval(timer);
  view.onclick = null; view.onchange = null;
  const hash = location.hash.slice(1) || TABS[0][0];
  const slash = hash.indexOf("/");
  const tab = slash < 0 ? hash : hash.slice(0, slash);
  const arg = slash < 0 ? "" : hash.slice(slash + 1);
  if (tab === "allgames") { location.replace("#games" + (arg ? "/" + arg : "")); return; }
  const t = tab === "user" ? "users" : tab;
  $$(".adm-side a").forEach(a => a.classList.toggle("on", a.dataset.tab === t));
  { const tb = TABS.find(x => x[0] === t); if (window.flCrumb) window.flCrumb(tb ? [[tb[1], "#" + t], ...(tab === "user" && arg ? [[decodeURIComponent(arg)]] : [])] : []); }
  const fn = { approvals, games, detection, users, hours: hoursReview, avatars: avatarReports, server, announcements, news, polls, incidents, settings: siteSettings, staff, user: userDetail, hourscheck: hoursCheck, bulk: bulkFixes, changelog, health, suggested: suggestedMerge, detect: detectTest, artwork: missingArt, checks: dataChecks, history: reviewHistory, undo: undoRecent }[tab];
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
  const mrgHtml = `<section class="panel"><h2>Suggested Merge</h2><div class="body"><p class="note">Programs the games list now detects, and catalogue rows that share a launcher id, are in <a href="#suggested">Suggested Merge</a>. Nothing is merged from this page.</p></div></section>`;
  const unkHtml = unk.length ? `<section class="panel"><h2>Unknown game IDs from the app (${unk.length})</h2>
    <div class="note">The app reported a game id the server doesn't know. Until you decide, it shows as "Playing a game". Map it to an existing game (moves its sessions, and future reports go there), approve it as a new game, or reject it.</div>
    <table><thead><tr><th>Game id</th><th>Exe</th><th class="num">Players</th><th class="num">Hours</th><th>First seen</th><th class="num"></th></tr></thead><tbody>
    ${unk.map(u => `<tr data-unk="${esc(u.game_id)}"><td class="mono">${esc(u.game_id)}</td><td class="mono dimt">${esc(u.exe || "unknown")}</td><td class="num">${u.players}</td><td class="num">${hrs(u.seconds / 60)}</td><td>${day(u.first_seen)}</td>
      <td class="num"><div class="acts"><input class="in" list="unk-games" data-map placeholder="Map to game…" size="20" aria-label="Map to existing game"><button class="btn sm" data-domap>Map</button>
        <input class="in" data-uname placeholder="Official name" size="16" aria-label="Name for a new game"><button class="btn sm dl" data-uapprove>Approve new</button><button class="btn sm danger" data-ureject>Reject</button></div></td></tr>`).join("")}</tbody></table>
    <datalist id="unk-games"></datalist></section>` : "";
  view.innerHTML = mrgHtml + unkHtml + `<section class="panel"><h2>${decided ? "Decided" : "Pending"} game suggestions <span class="more"><a href="#" data-flip>${decided ? "Show pending" : "Show decided"}</a></span></h2>
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
    const mt2 = e.target.closest("tr[data-mrg]");
    if (mt2) {
      const id = mt2.dataset.mrg, into = mt2.dataset.into, b = e.target.closest("button"); if (!b) return;
      if (b.matches("[data-merge]")) {
        const p = await api(`/admin/games/${encodeURIComponent(id)}/merge?into=${encodeURIComponent(into)}`).catch(x => { alert(x.message); return null; }); if (!p) return;
        const typed = prompt(`Merge "${p.from.name || id}" into "${p.into.name || into}"?\n${p.from.totals.players} players, ${p.from.totals.sessions} sessions move there. This can't be undone.\n\nType the id of the game being merged to confirm:\n${id}`);
        if (typed === null) return; if (typed.trim() !== id) return alert("The id didn't match; nothing was merged.");
        await act(b, async () => { await api(`/admin/games/${encodeURIComponent(id)}/merge`, { method: "POST", body: { into, confirm: typed.trim() } }); mt2.remove(); }); }
      else if (b.matches("[data-dismiss]")) await act(b, async () => { await api(`/admin/merge_suggestions/${encodeURIComponent(id)}/dismiss`, { method: "POST" }); mt2.remove(); });
      return;
    }
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
function badge(tab, n) { const s = $(`.adm-side a[data-tab=${tab}] .n`); if (s) { s.textContent = n; s.hidden = !n; } }

/* ---------- detection fix-ups ---------- */
const DKIND = { steam: "Steam app id", id: "Game id", path: "Folder + exe" };
const DS = { q: "", kind: "", sort: "", offset: 0 };
async function lastUpdated() {
  const m = await api("/admin/games-meta").catch(() => null); if (!m) return "";
  const ss = m.steam_sync, gl = m.games_list;
  return `<div class="note">Last updated · Steam sync: ${ss ? `checked ${when(ss.last_run_at)}${ss.detail ? ` (${esc(ss.detail)})` : ""}` : "not recorded yet"}${m.steam_last_account_sync ? ` · newest account refresh ${when(m.steam_last_account_sync)}` : ""}
    · Games list: ${gl && gl.updated ? `${esc(gl.updated)} (${(gl.games || 0).toLocaleString("en")} games)` : "built-in copy"}
    · Game edit: ${m.last_game_edit ? when(m.last_game_edit) : "never"} · Fix-up: ${m.last_fixup_change ? when(m.last_fixup_change) : "never"}</div>`;
}
async function detection() {
  view.innerHTML = `<section class="panel"><h2>Detection fix-ups</h2><div id="dmeta"></div>
    <div class="note">Sent to every app with the game rules, no app update needed. <b>Steam app id</b>: that Steam game counts as the game you pick (in the app and in Steam imports). <b>Game id</b>: one detected id counts as another. <b>Folder + exe</b>: the end of the program's path, e.g. <span class="mono">world of warcraft/_retail_/wow.exe</span>; checked before the usual exe names.</div>
    <div class="tools"><select class="in" id="dk">${Object.entries(DKIND).map(([k, v]) => `<option value="${k}">${esc(v)}</option>`).join("")}</select>
      <input class="in mono" id="dp" placeholder="793460 / old_id / folder/game.exe" size="30" aria-label="Pattern">
      <span class="muted">counts as</span><input class="in mono" id="dg" placeholder="game id" size="16" aria-label="Game id">
      <input class="in" id="dn" placeholder="Note (optional)" size="22" aria-label="Note"><button class="btn sm dl" id="dadd">Add</button></div>
    <div class="tools"><input class="in grow" type="search" id="dq" placeholder="Search pattern, game or note" value="${esc(DS.q)}" autocomplete="off">
      <select class="in" id="dkf" aria-label="Kind"><option value="">All kinds</option>${Object.entries(DKIND).map(([k, v]) => `<option value="${k}"${DS.kind === k ? " selected" : ""}>${esc(v)}</option>`).join("")}</select></div>
    <div id="dlist"><div class="body empty">Loading…</div></div></section>`;
  lastUpdated().then(h => { const e = $("#dmeta"); if (e) e.innerHTML = h; });
  let t; $("#dq").oninput = e => { clearTimeout(t); t = setTimeout(() => { DS.q = e.target.value.trim(); DS.offset = 0; listDetection(); }, 300); };
  $("#dkf").onchange = e => { DS.kind = e.target.value; DS.offset = 0; listDetection(); };
  view.onclick = async e => {
    const s_ = e.target.closest("th[data-dsort]"); if (s_) { DS.sort = s_.dataset.dsort; DS.offset = 0; listDetection(); return; }
    const pg = e.target.closest("[data-doff]"); if (pg) { DS.offset = +pg.dataset.doff; listDetection(); return; }
    const b = e.target.closest("button"); if (!b) return;
    if (b.id === "dadd") {
      const body = { kind: $("#dk").value, pattern: $("#dp").value.trim(), game_id: $("#dg").value.trim(), note: $("#dn").value.trim() };
      if (!body.pattern || !body.game_id) return alert("Fill in the pattern and the game id.");
      await act(b, async () => { await api("/admin/detection", { method: "POST", body }); detection(); });
    } else if (b.matches("[data-edit]")) {
      const tr = b.closest("tr"), g = prompt("Counts as (game id):", tr.dataset.game); if (g === null) return;
      const n = prompt("Note:", tr.dataset.note || ""); if (n === null) return;
      await act(b, async () => { await api(`/admin/detection/${tr.dataset.id}`, { method: "POST", body: { game_id: g.trim(), note: n.trim() } }); listDetection(); });
    } else if (b.matches("[data-del]")) {
      const tr = b.closest("tr"); if (!confirm("Delete this fix-up?")) return;
      await act(b, async () => { await api(`/admin/detection/${tr.dataset.id}/delete`, { method: "POST" }); listDetection(); });
    }
  };
  listDetection();
}
async function listDetection() {
  const box = $("#dlist"); if (!box) return;
  const d = await api("/admin/detection?" + new URLSearchParams(DS));
  if (!d.rules.length) { box.innerHTML = `<div class="body empty">${DS.q || DS.kind ? "No fix-ups match." : "No fix-ups yet."}</div>`; return; }
  const th = (k, l) => `<th class="srt${DS.sort === k ? " on" : ""}" data-dsort="${k}">${l}${DS.sort === k ? " ▾" : ""}</th>`;
  const to = d.offset + d.rules.length;
  box.innerHTML = `<table><thead><tr>${th("", "Kind")}<th>Pattern</th>${th("game", "Counts as")}<th>Note</th>${th("updated", "Last changed")}<th class="num"></th></tr></thead><tbody>
    ${d.rules.map(r => `<tr data-id="${r.id}" data-game="${esc(r.game_id)}" data-note="${esc(r.note || "")}"><td>${esc(DKIND[r.kind] || r.kind)}</td><td class="mono">${esc(r.pattern)}</td>
      <td>${esc(r.game_name || "")} <span class="mono dimt">${esc(r.game_id)}</span></td><td class="muted">${esc(r.note || "")}</td>
      <td class="dimt">${when(r.updated_at)}${r.updated_by ? ` · ${esc(r.updated_by)}` : ""}</td>
      <td class="num"><button class="btn sm" data-edit>Edit</button> <button class="btn sm danger" data-del>Delete</button></td></tr>`).join("")}</tbody></table>
    ${d.total > d.limit ? `<div class="pager"><button class="btn sm" data-doff="${Math.max(0, d.offset - d.limit)}" ${d.offset ? "" : "disabled"}>‹ Prev</button><span>${d.offset + 1}–${to} of ${d.total.toLocaleString("en")}</span><button class="btn sm" data-doff="${d.offset + d.limit}" ${to < d.total ? "" : "disabled"}>Next ›</button></div>` : `<div class="dimt">${d.total} fix-up${d.total === 1 ? "" : "s"}</div>`}`;
}
const SRC_LBL = { steam: "Steam", "games list": "Games list", "built-in": "Built-in", "folder rule": "Folder rule", "fix-up": "Fix-up", "extra exe": "Extra exe", approved: "Approved", unlisted: "Unlisted" };
function srcChips(g) {
  return (g.sources || [g.source]).map(k => k === "steam" && (g.steam_appids || []).length
    ? g.steam_appids.map(a => `<a class="pill" href="https://store.steampowered.com/app/${a}" target="_blank" rel="noopener" title="Steam app ${a}">Steam ${a}</a>`).join("")
    : `<span class="pill${k === "unlisted" ? " warn" : ""}">${esc(SRC_LBL[k] || k)}</span>`).join("");
}

/* ---------- All games: everything Fraglink knows, from every source ---------- */
const AG = { q: "", src: "", status: "", flag: "", sort: "updated", offset: 0, open: "" };
/* ids ticked in All games, for Bulk fixes (kept for the browser tab) */
const SEL = new Set(JSON.parse(sessionStorage.getItem("adm_bulk_ids") || "[]"));
const saveSel = () => { sessionStorage.setItem("adm_bulk_ids", JSON.stringify([...SEL])); const b = $("#agsel"); if (b) { b.hidden = !SEL.size; $("b", b).textContent = num(SEL.size); } };
const AG_SRC = [["games list", "Games list", "l"], ["built-in", "Built-in", "b"], ["steam", "Steam", "s"], ["folder rule", "Folder rule", "f"], ["fix-up", "Fix-up", "x"], ["extra exe", "Extra exe", "e"], ["approved", "Approved", "a"], ["unlisted", "Unlisted", "u"]];
const AG_ST = { active: ["Active", "ok"], hidden: ["Hidden", "hid"], retired: ["Retired", "ret"], merged: ["Merged", "mer"] };
const AG_FLAG = [["new", "New on the list · 7 days"], ["changed", "List changed · 7 days"], ["edited", "Staff edited"], ["hours", "Has recorded hours"], ["unverified", "Unverified"]];
const AG_ADDED = { fraglink: "first played on Fraglink", list: "first seen on the Games list", before_tracking: "on the Games list before tracking began" };
const num = n => (n || 0).toLocaleString("en");
const srcTag = k => { const s = AG_SRC.find(x => x[0] === k); return `<span class="src ${s ? s[2] : ""}">${esc(s ? s[1] : k)}</span>`; };
function stTag(r) {
  const s = AG_ST[r.status] || [r.status, ""];
  const life = r.lifecycle && r.lifecycle !== "live" ? ` <span class="st beta">${esc(LIFE[r.lifecycle] || r.lifecycle)}</span>` : "";
  return `<span class="st ${s[1]}">${esc(r.partial && r.status === "active" ? "Exe retired" : s[0])}</span>${life}`;
}
async function games(arg) {
  arg = arg || "";
  try { arg = decodeURIComponent(arg); } catch (e) { /* the hash was already decoded */ }
  if (arg.startsWith("id:")) {
    AG.open = arg.slice(3); AG.q = AG.open; AG.offset = 0; AG.src = ""; AG.status = ""; AG.flag = "";
  } else if (arg) { AG.q = arg; AG.offset = 0; AG.open = ""; }
  return allGames();
}
async function allGames() {
  view.innerHTML = `<section class="panel" style="margin-bottom:12px"><h2>Where the list comes from</h2><div id="agfresh"><div class="body empty">Loading…</div></div></section>
    <section class="panel"><h2>Games <span class="dimt" id="agcount" style="text-transform:none;letter-spacing:0;font-weight:400"></span></h2>
    <div class="ag-tb"><input class="in" type="search" id="agq" placeholder="Search name, id, exe, alias or Steam app id" value="${esc(AG.q)}" autocomplete="off" aria-label="Search games">
      <select class="in" id="agflag" aria-label="Show only"><option value="">Everything</option>${AG_FLAG.map(([k, v]) => `<option value="${k}"${AG.flag === k ? " selected" : ""}>${esc(v)}</option>`).join("")}</select>
      <select class="in" id="agsort" aria-label="Sort"><option value="updated">Last change first</option><option value="added">Newest added</option><option value="name">Name A–Z</option><option value="players">Most players</option><option value="hours">Most hours</option></select>
      <span class="dimt" style="margin-left:auto">50 per page · click a row to open it</span></div>
    <div class="ag-sel" id="agsel" hidden><b>0</b> selected <a class="btn sm dl" href="#bulk">Fix in Bulk fixes ›</a><button class="btn sm" data-selclear>Clear</button></div>
    <div class="ag-flt" id="agsrc"></div><div class="ag-flt" id="agst"></div>
    <div id="aglist"><div class="body empty">Loading…</div></div></section>`;
  $("#agsort").value = AG.sort;
  let t; $("#agq").oninput = e => { clearTimeout(t); t = setTimeout(() => { AG.q = e.target.value.trim(); AG.offset = 0; AG.open = ""; listAll(); }, 300); };
  $("#agflag").onchange = e => { AG.flag = e.target.value; AG.offset = 0; listAll(); };
  $("#agsort").onchange = e => { AG.sort = e.target.value; AG.offset = 0; listAll(); };
  view.onclick = e => {
    const c = e.target.closest("[data-agsrc]"); if (c) { AG.src = AG.src === c.dataset.agsrc ? "" : c.dataset.agsrc; AG.offset = 0; listAll(); return; }
    const s_ = e.target.closest("[data-agst]"); if (s_) { AG.status = AG.status === s_.dataset.agst ? "" : s_.dataset.agst; AG.offset = 0; listAll(); return; }
    const so = e.target.closest("th[data-agsort]"); if (so) { AG.sort = so.dataset.agsort; $("#agsort").value = AG.sort; AG.offset = 0; listAll(); return; }
    if (e.target.closest("[data-selclear]")) { SEL.clear(); saveSel(); $$("#aglist input[data-sel]").forEach(c => { c.checked = false; }); return; }
    const ck = e.target.closest("input[data-sel]"); if (ck) { if (ck.dataset.sel === "*") { $$("#aglist input[data-sel]:not([data-sel='*'])").forEach(c => { c.checked = ck.checked; SEL[ck.checked ? "add" : "delete"](c.dataset.sel); }); } else SEL[ck.checked ? "add" : "delete"](ck.dataset.sel); saveSel(); return; }
    if (e.target.closest("td.ck")) return;
    const pg = e.target.closest("[data-agoff]"); if (pg) { AG.offset = +pg.dataset.agoff; AG.open = ""; listAll().then(() => $("#aglist").scrollIntoView({ block: "start" })); return; }
    if (e.target.closest("a,button,tr.exp")) return;
    const tr = e.target.closest("tr.r"); if (tr) toggleAll(tr);
  };
  await listAll();
}
function freshHtml(m) {
  const gl = m.games_list, glj = m.games_list_job, ss = m.steam || {}, ch = m.list_changes_7d || {};
  const chTxt = Object.entries(ch).map(([k, v]) => `${v} ${k}`).join(" · ");
  const box = (k, v, d) => `<div><div class="k">${k}</div><div class="v">${v}</div><div class="d">${d}</div></div>`;
  return `<div class="ag-fresh">
    ${box("Games list", gl ? `${num(gl.games)} games` : "Not downloaded", `${gl && gl.updated ? `version ${esc(gl.updated)} · ` : ""}${glj ? `checked ${when(glj.last_run_at)}` : "not checked yet"}${chTxt ? ` · 7 days: ${esc(chTxt)}` : ""}`)}
    ${box("Built-in signatures", `${num(m.builtin.games)} games`, `ships with the app · version ${esc(m.builtin.version || "-")}`)}
    ${box("Steam", `${num(ss.games)} linked games`, `${ss.job ? `last import ${when(ss.job.last_run_at)}${ss.job.detail ? ` (${esc(ss.job.detail)})` : ""}` : "no import recorded yet"}${ss.last_change ? ` · last change ${when(ss.last_change)}` : ""}`)}
    ${box("Staff edits", `${num(m.edits.count)} games`, m.edits.last ? `last ${when(m.edits.last)}` : "none yet")}
    ${box("Detection fix-ups", `${num(m.fixups.count)} rules`, `${m.fixups.last ? `last ${when(m.fixups.last)}` : "none yet"} · ${num(m.folder_rules)} folder rules`)}
    ${box("Approved by staff", `${num(m.approved.count)} games`, m.approved.last ? `last ${when(m.approved.last)}` : "none yet")}</div>`;
}
async function listAll() {
  const box = $("#aglist"); if (!box) return;
  const d = await api("/admin/games-all?" + new URLSearchParams({ q: AG.q, src: AG.src, status: AG.status, flag: AG.flag, sort: AG.sort, offset: AG.offset }));
  AG.offset = d.offset;
  $("#agfresh").innerHTML = freshHtml(d.meta);
  $("#agcount").textContent = d.total === d.everything ? `· ${num(d.everything)} games` : `· ${num(d.total)} of ${num(d.everything)} games`;
  const cs = d.counts.src || {}, ct = d.counts.status || {};
  $("#agsrc").innerHTML = `<span class="lb">Source</span><button class="chip${AG.src ? "" : " on"}" data-agsrc="">All</button>` +
    AG_SRC.map(([k, l]) => `<button class="chip${AG.src === k ? " on" : ""}" data-agsrc="${esc(k)}"${cs[k] || AG.src === k ? "" : " disabled"}>${esc(l)} <b>${num(cs[k])}</b></button>`).join("");
  $("#agst").innerHTML = `<span class="lb">Status</span><button class="chip${AG.status ? "" : " on"}" data-agst="">All</button>` +
    Object.keys({ ...AG_ST, ...ct }).map(k => `<button class="chip${AG.status === k ? " on" : ""}" data-agst="${esc(k)}"${ct[k] || AG.status === k ? "" : " disabled"}>${esc((AG_ST[k] || [{ partial: "Exe retired", removed: "Off Games list" }[k] || LIFE[k] || k])[0])} <b>${num(ct[k])}</b></button>`).join("");
  if (!d.rows.length) { box.innerHTML = `<div class="body empty">No games match these filters.</div>`; return; }
  const th = (k, l, c) => `<th class="srt${AG.sort === k ? " on" : ""} ${c || ""}" data-agsort="${k}">${l}</th>`;
  box.innerHTML = `<table class="gl"><thead><tr><th class="ck"><input type="checkbox" data-sel="*" aria-label="Select this page"></th>${th("name", "Game")}<th>Sources</th><th>Exes</th><th>Status</th>${th("players", "Players", "nr")}${th("hours", "Hours", "nr")}${th("added", "Added")}${th("updated", "Last change")}</tr></thead><tbody>
    ${d.rows.map(r => `<tr class="r" data-id="${esc(r.id)}"><td class="ck"><input type="checkbox" data-sel="${esc(r.id)}"${SEL.has(r.id) ? " checked" : ""} aria-label="Select ${esc(r.name)}"></td><td><div class="gm"><img src="${esc(r.icon)}" alt="" data-hide-broken><div><div class="nm">${esc(r.name)}${r.renamed ? ` <span class="pill warn" title="Official name: ${esc(r.official)}">renamed</span>` : ""}${r.verified ? "" : ` <span class="pill">unverified</span>`}</div><div class="id">${esc(r.id)}</div>${(r.also_recorded || []).map(a => `<div class="sm">Also saved as <a href="#games/id:${encodeURIComponent(a.id)}">${esc(a.id)}</a>${a.total_min ? ` · ${hrs(a.total_min)}` : ""}</div>`).join("")}</div></div></td>
      <td>${r.sources.map(srcTag).join("")}</td>
      <td><div class="exes" title="${esc(r.exes.join(", "))}">${r.exes.length ? esc(r.exes.join(", ")) + (r.exe_count > r.exes.length ? ` +${r.exe_count - r.exes.length}` : "") : `<span class="dimt">-</span>`}</div>${r.folders || r.fixups ? `<div class="sm">${r.folders ? `${r.folders} folder hint${r.folders === 1 ? "" : "s"}` : ""}${r.folders && r.fixups ? " · " : ""}${r.fixups ? `${r.fixups} fix-up${r.fixups === 1 ? "" : "s"}` : ""}</div>` : ""}</td>
      <td>${stTag(r)}${r.removed_from_list ? `<div class="sm">off Games list ${day(r.removed_from_list)}</div>` : ""}</td>
      <td class="nr">${r.players ? num(r.players) : `<span class="dimt">0</span>`}</td><td class="nr ${r.total_min ? "hh" : ""}">${r.total_min ? hrs(r.total_min) : `<span class="dimt">-</span>`}</td>
      <td class="sm" title="${esc(AG_ADDED[r.added_kind] || "")}">${r.added_kind === "before_tracking" ? "before tracking" : r.added ? day(r.added) : "-"}</td>
      <td><div class="sm">${r.updated ? when(r.updated) : "-"}</div>${r.by ? `<div class="who">${esc(r.by)}</div>` : ""}</td></tr>`).join("")}</tbody></table>
    ${pagerAll(d)}`;
  saveSel();
  const o = AG.open && box.querySelector(`tr.r[data-id="${CSS.escape(AG.open)}"]`);
  if (o) await toggleAll(o, true);
}
function pagerAll(d) {
  const pages = Math.ceil(d.total / d.page_size), cur = Math.floor(d.offset / d.page_size);
  if (pages <= 1) return `<div class="pg"><span>${num(d.total)} game${d.total === 1 ? "" : "s"}</span></div>`;
  const want = new Set([0, pages - 1, cur - 2, cur - 1, cur, cur + 1, cur + 2].filter(p => p >= 0 && p < pages));
  let last = -1, out = "";
  [...want].sort((a, b) => a - b).forEach(p => { if (p - last > 1) out += `<span>…</span>`; out += `<button data-agoff="${p * d.page_size}"${p === cur ? ` class="on" aria-current="page"` : ""}>${p + 1}</button>`; last = p; });
  return `<div class="pg"><button data-agoff="${Math.max(0, d.offset - d.page_size)}"${cur ? "" : " disabled"}>‹ Prev</button>${out}<button data-agoff="${d.offset + d.page_size}"${cur < pages - 1 ? "" : " disabled"}>Next ›</button>
    <span class="sp">${num(d.offset + 1)}–${num(Math.min(d.total, d.offset + d.page_size))} of ${num(d.total)}</span></div>`;
}
const HIST = {
  "game.steam_sync": "Steam details updated", "game.edit": "Edited", "game.approve": "Approved", "game.reject": "Rejected", "game.undo_decision": "Decision undone",
  "game.icon_upload": "Icon uploaded", "game.icon_remove": "Icon removed", "game.media": "Media changed", "game.art": "Cover art changed", "game.delete": "Deleted", "unknown_game.map": "Unknown id mapped", "game.merge": "Merged", "game.retire": "Retired", "game.unretire": "Retire undone", "game.status": "Status set",
  new: "Added to the Games list", back: "Back on the Games list", renamed: "Renamed on the Games list", exes: "Exes changed on the Games list", removed: "Removed from the Games list",
  first_seen: "First seen on the Games list", on_list_when_tracking_began: "On the Games list when tracking began", first_on_fraglink: "First played on Fraglink", status_checked: "Status checked",
};
function histLine(e) {
  const lbl = HIST[e.action] || String(e.action || "").replace(/^game\.|^detection\.|^unknown_game\./, "").replace(/_/g, " ");
  const a = e.after || {}, b = e.before || {};
  let det = "";
  if (e.action === "renamed") det = `${esc(b.name || "")} → ${esc(a.name || "")}`;
  else if (e.action === "exes") det = `<span class="mono">${esc((a.exes || []).join(", "))}</span>`;
  else if (a.name && b.name && a.name !== b.name) det = `${esc(b.name)} → ${esc(a.name)}`;
  else if (a.into) det = `into <span class="mono">${esc(a.into)}</span>`;
  else if (a.to) det = `to <span class="mono">${esc(a.to)}</span>`;
  else if (a.pattern) det = `<span class="mono">${esc(a.pattern)}</span>`;
  else if (a.steam_appid) det = `Steam ${esc(String(a.steam_appid))}`;
  const by = e.kind === "audit" ? (e.by ? `<em${e.by === "Steam sync" ? ` class="sys"` : ""}>${esc(e.by)}</em>` : "") : e.by ? `<em class="sys">${esc(e.by)}</em>` : "";
  return `<div><time>${when(e.at)}</time><span><b>${esc(lbl)}</b>${det ? " · " + det : ""}${by ? " · " + by : ""}${e.list_updated ? ` <span class="dimt">(list ${esc(e.list_updated)})</span>` : ""}</span></div>`;
}
async function toggleAll(tr, keep) {
  const next = tr.nextElementSibling;
  if (next && next.classList.contains("exp")) { if (keep) return; next.remove(); tr.classList.remove("open"); AG.open = ""; return; }
  $$("#aglist tr.exp").forEach(x => x.remove()); $$("#aglist tr.open").forEach(x => x.classList.remove("open"));
  const id = tr.dataset.id; AG.open = id; tr.classList.add("open");
  const ex = document.createElement("tr"); ex.className = "exp";
  ex.innerHTML = `<td colspan="9"><div class="dimt">Loading…</div></td>`; tr.after(ex);
  const [g, h] = await Promise.all([api("/admin/games/" + encodeURIComponent(id)).catch(() => null), api(`/admin/games/${encodeURIComponent(id)}/history`).catch(() => ({ events: [] }))]);
  if (!ex.isConnected) return;
  const rows = [], d = (g && g.detection) || {}, o = (g && g.override) || {}, b = d.builtin;
  if (b) rows.push(["Built-in", [...(b.exes || []), ...(b.path_exes || [])].join(", ") + ((b.path_hints || []).length ? ` in ${b.path_hints.join(", ")}` : ""), "ships with the app"]);
  if (d.in_games_list) rows.push(["Games list", (d.list_patterns || []).map(p => p.exes.join(", ") + (p.paths.length ? " in " + p.paths.join(", ") : "")).join("; "), "downloaded daily"]);
  ((g && g.steam_appids) || []).forEach(a => rows.push(["Steam", String(a), "Steam import"]));
  if ((o.extra_exes || []).length) rows.push(["Extra exes", o.extra_exes.join(", "), d.edited ? when(d.edited.at) + (d.edited.by ? " · " + d.edited.by : "") : "staff"]);
  if ((o.path_hints || []).length) rows.push(["Folder hints", o.path_hints.join(", "), d.edited ? when(d.edited.at) : "staff"]);
  (d.fixups || []).forEach(x => rows.push(["Fix-up", x.pattern + (x.note ? ` (${x.note})` : ""), when(x.updated_at) + (x.updated_by ? " · " + x.updated_by : "")]));
  ((g && g.merged_from) || []).forEach(m => rows.push(["Merged in", m, ""]));
  const r = (g && g.retired) || null;
  const names = [["Shown as", g ? (g.public_name || g.name || id) : id], ["Official", g ? g.name || "-" : "-"], ["Id", `<span class="mono">${esc(id)}</span>`, 1]];
  if ((o.aliases || []).length) names.push(["Also called", o.aliases.join(", ")]);
  const links = ((g && g.steam_appids) || []).map(a => `<a href="https://store.steampowered.com/app/${a}" target="_blank" rel="noopener">Steam store ${a}</a>`);
  if (r && r.successor) links.push(`<a href="#games/${encodeURIComponent(r.successor)}">${r.merged ? "Merged into" : "New entry"} ${esc(r.successor)}</a>`);
  const ev = h.events || [];
  ex.innerHTML = `<td colspan="9"><div class="xg">
    <div class="xb"><h4>How it's found <span>${rows.length} rule${rows.length === 1 ? "" : "s"}</span></h4>${rows.length ? `<table>${rows.map(x => `<tr><td style="width:96px">${esc(x[0])}</td><td class="mono">${esc(x[1])}</td><td class="rt">${esc(x[2])}</td></tr>`).join("")}</table>` : `<div class="xkv"><span class="dimt">No rules: only reported by the app as unlisted.</span></div>`}</div>
    <div class="xb"><h4>Names &amp; links</h4><div class="xkv">${names.map(n => `<b>${esc(n[0])}</b><span>${n[2] ? n[1] : esc(n[1])}</span>`).join("")}
      <b>Players</b><span>${g ? `${num(g.players)} · ${hrs(g.total_min)} (tracked ${hrs(g.tracked_min)} · imported ${hrs(g.imported_min)})` : "-"}</span>
      <b>Links</b><span>${links.join(" · ") || `<span class="dimt">none</span>`}</span></div></div>
    <div class="xb wide"><h4>History <span>${ev.length} event${ev.length === 1 ? "" : "s"} · staff, Steam sync and Games list</span></h4>${ev.length ? `<div class="lg">${ev.map(histLine).join("")}</div>` : `<div class="xkv"><span class="dimt">Nothing recorded yet.</span></div>`}</div></div>
    <div class="xact"><span class="note">Edits save here and land in History. Hours recorded under another id for this same game stay on that id.</span><a class="btn sm" href="#detection">Detection rules</a></div>
    <section class="panel inline-edit" id="gedit"><div class="body empty">Loading editor…</div></section></td>`;
  editGame(id).catch(e => { const box = $("#gedit"); if (box) box.innerHTML = `<h2>Edit game</h2><div class="alert err">${esc(e.message)}</div>`; });
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
    <table><thead><tr><th></th>${th("name", "Name")}<th>Id</th><th>Source</th>${th("players", "Players", "num")}${th("hours", "Hours", "num")}${th("recent", "Last played", "num")}${th("updated", "Changed", "num")}</tr></thead><tbody>
    ${d.games.map(g => {
      const o = g.override || {};
      const flags = (o.name ? `<span class="pill warn">renamed</span>` : "") + (o.hidden ? `<span class="pill bad">hidden</span>` : "") +
        (o.has_icon ? `<span class="pill">icon</span>` : "") + ((o.extra_exes || []).length ? `<span class="pill">+${o.extra_exes.length} exe</span>` : "") +
        (g.retired ? `<span class="pill bad">${g.retired.partial ? "exe retired" : "retired"}</span>` : "") +
        (g.status && g.status.lifecycle !== "live" ? `<span class="pill ${g.status.lifecycle === "ended" ? "bad" : "warn"}">${esc(LIFE[g.status.lifecycle] || g.status.lifecycle)}</span>` : "");
      const bd = [["tracked", g.tracked_min], ["imported", g.imported_min]].filter(x => x[1] > 0).map(([k, m]) => `${k} ${hrs(m)}`).join(" · ");
      return `<tr class="click" data-id="${esc(g.id)}"><td class="ic"><img src="${esc(g.icon)}" width="20" height="20" alt="" data-hide-broken></td>
        <td>${esc(g.public_name || g.name)}${flags}</td><td class="mono dimt">${esc(g.id)}</td><td>${srcChips(g)}</td>
        <td class="num">${g.players}</td><td class="num" title="${esc(bd || "no hours")}">${hrs(g.total_min)}${bd ? `<div class="dimt" style="font-size:10.5px;white-space:nowrap">${esc(bd)}</div>` : ""}</td><td class="num dimt">${g.last_played_at ? day(g.last_played_at) : "-"}</td><td class="num dimt">${g.updated_at ? day(g.updated_at) : "-"}</td></tr>`;
    }).join("")}</tbody></table>
    ${d.total > d.limit ? `<div class="pager"><button class="btn sm" data-goff="${Math.max(0, GS.offset - d.limit)}" ${GS.offset ? "" : "disabled"}>‹ Prev</button><span>${from}–${to} of ${d.total.toLocaleString("en")}</span><button class="btn sm" data-goff="${GS.offset + d.limit}" ${to < d.total ? "" : "disabled"}>Next ›</button></div>` : ""}`;
  box.onclick = e => {
    const s_ = e.target.closest("th[data-sort]"); if (s_) { GS.sort = s_.dataset.sort; GS.offset = 0; listGames(q); return; }
    const pg = e.target.closest("[data-goff]"); if (pg) { GS.offset = +pg.dataset.goff; listGames(q).then(() => box.scrollIntoView({ block: "start" })); return; }
    const tr = e.target.closest("tr[data-id]"); if (tr) editGame(tr.dataset.id);
  };
}
/* Refresh the list this editor belongs to. On All games the open row is the editor. */
async function relist(openId) {
  const gs = $("#gs");
  if (gs) {
    listGames(gs.q.value.trim());
    if (openId) await editGame(openId);
    return;
  }
  if ($("#aglist")) {
    AG.open = openId || "";
    await listAll();
  }
}
const LIFE = { live: "Live", beta: "Beta", test: "Test", early_access: "Early Access", ended: "Ended" };
function statusHtml(g) {
  const s = g.status || { lifecycle: "live" };
  return `<form method="post" action="/api/v1/form-fallback" class="f" id="gst"><label>Status</label>
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
    ${exes.length ? `<div>Programs to detach:</div>${exes.map(e => `<div class="row"><label class="check" style="margin:0"><input type="checkbox" name="rx" value="${esc(e)}" checked> <span class="mono">${esc(e)}</span></label>${exes.length > 1 ? `<button class="btn sm" type="button" data-rone="${esc(e)}">Retire only this exe…</button>` : ""}</div>`).join("")}` : `<div class="dimt">No known program names; add an extra exe first.</div>`}
    <div class="row"><button class="btn danger" id="gret" type="button"${exes.length ? "" : " disabled"}>Retire…</button></div>
    <small>All programs: the entry is frozen (name, icon and hours stay) and new play starts a fresh entry in Game approvals. Some programs: only those move to fresh entries; the game stays live.</small>
    <div class="alert" id="rmsg" hidden></div></div>`;
}
function detHtml(g) {
  const d = g.detection || {}, fx = d.fixups || [], b = d.builtin, o = g.override || {};
  const KIND = { steam: "Steam app id", id: "Game id", path: "Folder + exe" };
  const rows = [];
  if (b) rows.push(`<tr><td>Built-in signature</td><td class="mono">${esc([...(b.exes || []), ...(b.path_exes || [])].join(", "))}${(b.path_hints || []).length ? ` <span class="dimt">in ${esc(b.path_hints.join(", "))}</span>` : ""}</td><td class="dimt">ships with the app</td></tr>`);
  if (d.in_games_list) rows.push(`<tr><td>Games list</td><td class="mono">${esc((d.list_patterns || []).map(p => p.exes.join(", ") + (p.paths.length ? " in " + p.paths.join(", ") : "")).join("; "))}</td><td class="dimt">${(() => { const j = (d.jobs || []).find(j => /list/.test(j.job)); return j ? "list refreshed " + when(j.last_ok_at || j.last_run_at) : "downloaded list"; })()}</td></tr>`);
  (g.steam_appids || []).forEach(a => rows.push(`<tr><td>Steam</td><td class="mono">${a}</td><td class="dimt">${(() => { const j = (d.jobs || []).find(j => /steam/.test(j.job)); return j ? "Steam sync " + when(j.last_ok_at || j.last_run_at) : ""; })()}</td></tr>`));
  if ((o.extra_exes || []).length) rows.push(`<tr><td>Staff: extra exes</td><td class="mono">${esc(o.extra_exes.join(", "))}</td><td class="dimt">${d.edited ? when(d.edited.at) + (d.edited.by ? " by " + esc(d.edited.by) : "") : ""}</td></tr>`);
  if ((o.path_hints || []).length) rows.push(`<tr><td>Staff: folder hints</td><td class="mono">${esc(o.path_hints.join(", "))}</td><td class="dimt">${d.edited ? when(d.edited.at) + (d.edited.by ? " by " + esc(d.edited.by) : "") : ""}</td></tr>`);
  fx.forEach(x => rows.push(`<tr><td>Fix-up: ${esc(KIND[x.kind] || x.kind)}</td><td class="mono">${esc(x.pattern)}${x.note ? ` <span class="dimt">${esc(x.note)}</span>` : ""}</td><td class="dimt">${when(x.updated_at)}${x.updated_by ? " by " + esc(x.updated_by) : ""}</td></tr>`));
  (g.merged_from || []).forEach(m => rows.push(`<tr><td>Merged in</td><td class="mono">${esc(m)}</td><td></td></tr>`));
  return `<div class="f"><label>Where detection comes from</label>
    ${rows.length ? `<table class="tbl"><thead><tr><th>Source</th><th>Rule</th><th>Last updated</th></tr></thead><tbody>${rows.join("")}</tbody></table>` : `<div class="dimt">No rules: only reported by the app as unlisted.</div>`}
    <div class="row"><button class="btn sm" type="button" id="steamchk">Check Steam link</button><a href="#detection" class="dimt">Detection fix-ups ›</a></div><div id="steamout"></div><div class="alert" id="steammsg" hidden></div></div>`;
}
async function editGame(id) {
  const box = $("#gedit");
  const g = await api("/admin/games/" + encodeURIComponent(id));
  const o = g.override || {};
  const bust = "?t=" + Date.now();
  box.innerHTML = `<h2>Edit game</h2><div class="body"><form method="post" action="/api/v1/form-fallback" class="f" id="gf">
    <div><b>${esc(g.public_name || g.name || g.id)}</b> <span class="mono dimt">${esc(g.id)}</span><div class="dimt">Sources: ${srcChips(g)}${g.updated_at ? ` · last changed ${when(g.updated_at)}` : ""} · ${g.players} players · ${hrs(g.total_min)} (tracked ${hrs(g.tracked_min)} · imported ${hrs(g.imported_min)})</div></div>
    <label>Display name override<input type="text" name="name" maxlength="80" value="${esc(o.name || "")}" placeholder="${esc(g.name)}"><small>Leave empty to use the list name.</small></label>
    <label class="check"><input type="checkbox" name="hidden" ${o.hidden ? "checked" : ""}> Hide from public (shown as "Playing a game")</label>
    <label>Extra exe names (one per line)<textarea class="in" name="exes" placeholder="GameClient-Win64.exe">${esc((o.extra_exes || []).join("\n"))}</textarea><small>Apps detect these as this game (picked up within a day or on restart).</small></label>
    <label>Other names / aliases (one per line)<textarea class="in" name="aliases" placeholder="WoW">${esc((o.aliases || []).join("\n"))}</textarea><small>Found by Admin search. For detection fixes use extra exe names, folder hints or Detection fix-ups.</small></label>
    <label>Folder hints (one per line)<textarea class="in" name="hints" placeholder="World of Warcraft/_retail_">${esc((o.path_hints || []).map(h => h.replace(/^\/|\/$/g, "")).join("\n"))}</textarea><small>This game's programs (${esc((g.exes || []).join(", ") || "none known")}) only count as this game inside a folder containing one of these. Use it when two games share an exe name. Empty = anywhere.</small></label>
    <div class="row"><button class="btn dl">Save</button><span class="dimt">Edits stay in place when the games list refreshes.</span></div>
    <div class="alert" id="gmsg" hidden></div></form>
    <hr style="border:0;border-top:1px solid var(--line);margin:14px 0">${detHtml(g)}
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
      relist(id);
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
    act(ret, async () => { await api(gid + "/retire", { method: "POST", body: { exes: pick } }); relist(id); }).catch(x => msg($("#rmsg"), x.message));
  });
  if (unret) unret.addEventListener("click", () => act(unret, async () => { await api(gid + "/unretire", { method: "POST" }); relist(id); }).catch(x => msg($("#rmsg"), x.message)));
  $("#gf").addEventListener("submit", async e => {
    e.preventDefault(); const f = e.target;
    try {
      await api("/admin/games/" + encodeURIComponent(id), { method: "POST", body: { name: f.name.value.trim() || null, hidden: f.hidden.checked, extra_exes: f.exes.value.split(/\n|,/).map(s => s.trim()).filter(Boolean), aliases: f.aliases.value.split("\n").map(s => s.trim()).filter(Boolean), path_hints: f.hints.value.split("\n").map(s => s.trim()).filter(Boolean) } });
      await relist(id); const sm = $("#gmsg"); if (sm) msg(sm, "Saved.", true);
    } catch (x) { msg($("#gmsg"), x.message); }
  });
  renderMedia(id, g);
  wireMerge(id);
  box.querySelectorAll("[data-rone]").forEach(b => b.addEventListener("click", () => {
    const e = b.dataset.rone;
    if (!confirm(`Detach only ${e} from ${g.public_name || g.name || id}?\n\nIt gets a new entry (in Game approvals); the game stays live with its other programs.`)) return;
    act(b, async () => { await api(gid + "/retire", { method: "POST", body: { exes: [e] } }); relist(id); }).catch(x => msg($("#rmsg"), x.message));
  }));
  const sc = $("#steamchk");
  if (sc) sc.addEventListener("click", () => act(sc, async () => {
    const r = await api(gid + "/steam_check");
    $("#steamout").innerHTML = r.checks.length ? `<table class="tbl"><tbody>${r.checks.map(c => `<tr><td class="mono"><a href="${esc(c.url)}" target="_blank" rel="noopener">${c.appid}</a></td><td>${!c.reachable ? `<span class="pill">Steam didn't answer</span>` : c.found ? `<span class="pill ok">OK</span> ${esc(c.name || "")} <span class="dimt">${esc(c.type || "")}</span>${c.name && (g.public_name || g.name) && c.name.toLowerCase() !== (g.public_name || g.name).toLowerCase() ? ` <span class="pill bad">name differs</span>` : ""}` : `<span class="pill bad">not on the Steam store</span>`}</td></tr>`).join("")}</tbody></table><div class="dimt">Checked ${when(r.checked_at)}</div>` : `<div class="dimt">No Steam app id linked. Set one under Pictures or add a Steam fix-up in Detection.</div>`;
  }).catch(x => msg($("#steammsg"), x.message)));
  const gd = $("#gdelete");
  if (gd) gd.addEventListener("click", () => {
    const typed = prompt(`Delete "${g.public_name || g.name || id}"${g.sessions ? ` and ${g.sessions} session(s) from ${g.players} player(s)` : ""}? This can't be undone.\n\nType the game id to confirm:\n${id}`);
    if (typed === null) return;
    if (typed.trim() !== id) return msg($("#dmsg"), "The id didn't match; nothing was deleted.");
    act(gd, async () => {
      await api("/admin/games/" + encodeURIComponent(id) + "/delete", { method: "POST", body: { confirm: typed.trim() } });
      if ($("#gs")) { $("#gedit").innerHTML = `<h2>Edit game</h2><div class="body empty">Deleted ${esc(id)}.</div>`; listGames($("#gs").q.value.trim()); }
      else { AG.open = ""; listAll(); }
    }).catch(x => msg($("#dmsg"), x.message));
  });
}

/* ---------- 3. users ---------- */
async function users(q) {
  q = q || "";
  view.innerHTML = `<section class="panel"><h2>Users</h2>
    <form method="post" action="/api/v1/form-fallback" class="tools" id="us"><input class="in grow" name="q" value="${esc(q)}" placeholder="Username, display name or email" autocomplete="off"></form>
    <div id="ulist"></div></section>`;
  $("#us").addEventListener("submit", e => { e.preventDefault(); const v = e.target.q.value.trim(); history.replaceState(null, "", "#users/" + encodeURIComponent(v)); listUsers(v); });
  liveSearch($("#us"));
  listUsers(q);
}
async function listUsers(q, off) {
  off = off || 0;
  const d = await api("/admin/users?" + new URLSearchParams({ q, offset: off }));
  const pages = Math.ceil(d.total / d.page_size);
  const pager = pages > 1 ? `<div class="pg"><button data-uoff="${Math.max(0, off - d.page_size)}"${off ? "" : " disabled"}>‹ Prev</button><span class="sp">${num(off + 1)}–${num(Math.min(d.total, off + d.page_size))} of ${num(d.total)}</span><button data-uoff="${off + d.page_size}"${off + d.page_size < d.total ? "" : " disabled"}>Next ›</button></div>` : "";
  $("#ulist").innerHTML = d.users.length ? `<div class="note">${q ? `${num(d.total)} match${d.total === 1 ? "" : "es"}` : `Newest accounts · ${num(d.total)} in all`}</div><table><thead><tr><th>Username</th><th>Display name</th><th>Email</th><th>Age</th><th>Joined</th><th>Status</th></tr></thead><tbody>
    ${d.users.map(u => `<tr class="click" data-id="${esc(u.id)}"><td><a href="#user/${esc(u.id)}">${esc(u.handle)}</a>${rolePill(u.role)}${u.is_test ? `<span class="pill" title="Test account: hidden from everyone else">test</span>` : ""}${u.banned ? `<span class="pill bad">banned</span>` : ""}${u.age_review_at ? `<span class="pill bad" title="Entered a birthday under 13. Can't sign in until reviewed.">age check</span>` : ""}${u.hours_flag_at ? `<span class="pill bad" title="Unusual tracked hours. Look at Sessions; nothing was removed.">hours</span>` : ""}</td>
      <td>${esc(u.display_name)}</td><td class="muted">${esc(u.email || "-")}</td><td>${u.is_minor ? `<span class="pill" title="Under 18: private by default; can go public">13–17</span>` : `<span class="dimt">18+</span>`}</td><td>${day(u.created_at)}</td>
      <td><span class="status"><i class="dot ${esc(u.status === "invisible" ? "offline" : u.status)}"></i>${esc(u.status)}</span>${u.last_seen ? ` <span class="dimt">${when(u.last_seen)}</span>` : ""}</td></tr>`).join("")}</tbody></table>`
    + pager : `<div class="body empty">No accounts match.</div>`;
  $("#ulist").onclick = e => { const pb = e.target.closest("[data-uoff]"); if (pb) { listUsers(q, +pb.dataset.uoff); return; } const tr = e.target.closest("tr[data-id]"); if (tr && !e.target.closest("a")) location.hash = "user/" + tr.dataset.id; };
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
    <section class="panel"><h2>Recent sessions <span class="more"><a href="#hourscheck/${esc(u.id)}">Hours checker</a></span></h2>${u.sessions.length ? `<table><thead><tr><th>Game</th><th>Started</th><th class="num">Length</th><th>Source</th><th></th></tr></thead><tbody>
      ${u.sessions.map(s => `<tr data-sid="${esc(s.id)}"><td>${esc(s.name)}</td><td>${when(s.started_at)}</td><td class="num">${hrs(s.duration_s / 60)}${s.duration_s > 18 * 3600 ? `<span class="pill bad">long</span>` : ""}</td><td class="muted">${esc(s.source)}</td><td class="num"><button class="btn sm danger" data-delsess>Delete</button></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No sessions.</div>`}</section>
  </div><div class="col">
    <section class="panel"><h2>Actions</h2><div class="body f">
      <div><b>Email</b> ${u.email ? (u.email_verified ? `<span class="pill ok">confirmed</span>` : `<span class="pill bad">not confirmed</span>`) : `<span class="dimt">none</span>`}</div>
      ${u.email ? `<div class="row">${u.email_verified ? "" : `<button class="btn sm" id="vresend"${u.mail_live ? "" : " disabled"}>Resend confirmation email</button>`}${can.reset_password ? `<button class="btn sm" id="vset" data-v="${u.email_verified ? "0" : "1"}">${u.email_verified ? "Mark not confirmed" : "Mark confirmed"}</button>` : ""}</div>` : ""}
      ${can.reset_password && u.email ? `<div class="row"><button class="btn sm" id="resetlink"${u.mail_live ? "" : " disabled"}>Email a password reset link</button></div><small>Sends a 1-hour link to their address. Nothing changes until they use it.${u.mail_live ? "" : " Email is off on this server."}</small>` : ""}
      ${can.reset_password ? `<label class="check"><input type="checkbox" id="istest" ${u.is_test ? "checked" : ""}> Hidden test account</label><small>Hidden from search, profiles, lists, polls and totals. The account still works.</small>` : ""}
      <div class="alert" id="acctmsg" hidden></div><hr style="border:0;border-top:1px solid var(--line);margin:6px 0">
      ${u.hours_flag_at ? `<div class="row"><button class="btn" id="hoursclear">Clear hours flag</button></div><small>Only removes the flag. To fix hours, use the session tools.</small><hr style="border:0;border-top:1px solid var(--line);margin:6px 0">` : ""}
      ${u.age_review_at ? `<div class="row"><button class="btn" id="ageclear">Clear age check</button></div><small>Lets them sign in again. If they really are under 13, ban or delete the account instead. Nothing is deleted automatically.</small><hr style="border:0;border-top:1px solid var(--line);margin:6px 0">` : ""}
      ${u.banned_at ? `<button class="btn" id="unban">Unban</button>` : `<label>Ban reason (optional, kept in the audit log)<input type="text" id="reason" maxlength="200"></label><div class="row"><button class="btn danger" id="ban">Ban account</button></div><small>Banned accounts are signed out everywhere, can't sign in, and their profile is hidden.</small>`}
      ${can.reset_password ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div class="row"><button class="btn" id="resetpw">Reset password</button></div><small>Makes a temporary password (shown once) and signs the account out everywhere. Send it to the user yourself. Prefer the reset link above.</small><div id="pwout"></div>` : ""}
      ${can.reset_password ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><label>Date of birth (YYYY-MM-DD)<input type="date" id="newdob" min="1900-01-01" max="2099-12-31" value="${esc(u.birth_date || "")}"></label><div class="row"><button class="btn" id="chdob">Save date of birth</button></div><small>Only staff can change this. Under 13 is refused and not stored. 13–17 forces a private profile; turning 18 clears the under-18 flag but leaves the profile private.</small>` : ""}
      ${can.change_handle ? `<hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><label>Username<input type="text" id="newhandle" maxlength="20" value="${esc(u.handle)}"></label><div class="row"><button class="btn" id="chhandle">Change username</button></div><small>Users can't change their username themselves. The old one keeps redirecting to them and can't be taken by anyone else. Logged in the audit log.</small>` : ""}
      <hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div class="row"><button class="btn" id="rmav">Remove picture</button>${can.mfa_reset ? ` <button class="btn" id="mfareset">Reset two-step sign-in</button>` : ""}</div><small>Removing a picture resets it to the default and blocks that exact image. Resetting two-step sign-in lets them sign in with just their password (check it's really them first).</small>
      <hr style="border:0;border-top:1px solid var(--line);margin:6px 0"><div><b>WoW hours</b></div><div class="row"><button class="btn" data-wowrc="wow" data-gname="WoW">Recount WoW</button><button class="btn" data-wowrc="wow_forever" data-gname="WoW Forever">Recount WoW Forever</button></div><small>Recounts from their characters' /played only: drops the menu-time extra carried on top. Deleted characters keep their hours. Logged in the audit log.</small>
      <div class="alert" id="amsg" hidden></div></div></section>
    ${can.staff && u.role !== "owner" && u.handle !== who.handle ? `<section class="panel"><h2>Staff role</h2><div class="body f"><div class="row"><select class="in" id="newrole"><option value="none"${u.role ? "" : " selected"}>No staff role</option><option value="moderator"${u.role === "moderator" ? " selected" : ""}>Moderator</option>${can.manage_admins || u.role === "admin" ? `<option value="admin"${u.role === "admin" ? " selected" : ""}${can.manage_admins ? "" : " disabled"}>Admin</option>` : ""}</select><button class="btn" id="setrole">Save role</button></div><small>Moderators handle approvals, users and hours. Only the owner adds or removes admins. Logged in the audit log.</small></div></section>` : ""}
    ${can.user_delete && !u.role && u.handle !== who.handle ? `<section class="panel danger-zone"><h2>Delete account</h2><div class="body f"><p class="note">Deletes the account and everything in it for good: hours, sessions, friends, pictures and sign-ins. It can't be undone. Ban instead if you might need it back.</p>
      <label>Type <b class="mono">${esc(u.handle)}</b> to confirm<input type="text" id="delconfirm" autocomplete="off" spellcheck="false"></label><div class="row"><button class="btn danger" id="deluser" disabled>Delete account for good</button></div><div class="alert" id="delmsg" hidden></div></div></section>` : ""}
    <section class="panel"><h2>Signed-in devices</h2>${u.devices.length ? u.devices.map(d => `<div class="fr"><div class="who"><div class="nm">${esc(d.name || (d.kind === "web" ? "Web browser" : "Fraglink desktop"))} <small>${esc(d.kind)}</small></div><div class="gm">signed in ${day(d.created_at)} · last used ${when(d.last_used_at)}</div></div></div>`).join("") : `<div class="body empty">None.</div>`}</section>
  </div></div>`;
  const amsg = $("#amsg");
  const sr = $("#setrole");
  if (sr) sr.addEventListener("click", () => { const r = $("#newrole").value; if ((r === "none" ? !u.role : r === u.role) || !confirm(r === "none" ? `Remove ${u.handle}'s staff role?` : `Make ${u.handle} ${r === "admin" ? "an admin" : "a moderator"}?`)) return; act(sr, async () => { await api("/admin/staff", { method: "POST", body: { handle: u.handle, role: r } }); userDetail(id); }); });
  const dc = $("#delconfirm"), du = $("#deluser");
  if (dc) {
    dc.addEventListener("input", () => { du.disabled = dc.value.trim().toLowerCase() !== u.handle.toLowerCase(); });
    du.addEventListener("click", () => { if (!confirm(`Delete ${u.handle} for good? This can't be undone.`)) return; act(du, async () => { try { await api(`/admin/users/${encodeURIComponent(u.id)}/delete`, { method: "POST", body: { confirm: dc.value.trim() } }); location.hash = "users"; } catch (x) { msg($("#delmsg"), x.message); } }); });
  }
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
  const am = $("#acctmsg"), uid = `/admin/users/${encodeURIComponent(u.id)}`;
  const vr = $("#vresend"); if (vr) vr.addEventListener("click", () => act(vr, async () => { await api(uid + "/verify/resend", { method: "POST" }); msg(am, "Confirmation email sent to " + u.email + ".", true); }).catch(x => msg(am, x.message)));
  const vs = $("#vset"); if (vs) vs.addEventListener("click", () => { const v = vs.dataset.v === "1"; if (confirm(`${v ? "Mark" : "Unmark"} ${u.handle}'s email as confirmed?`)) act(vs, async () => { await api(uid + "/verify/set", { method: "POST", body: { value: v } }); userDetail(u.id); }).catch(x => msg(am, x.message)); });
  const rl = $("#resetlink"); if (rl) rl.addEventListener("click", () => { if (confirm(`Email a password reset link to ${u.email}?`)) act(rl, async () => { await api(uid + "/reset_link", { method: "POST" }); msg(am, "Reset link sent to " + u.email + ".", true); }).catch(x => msg(am, x.message)); });
  const it = $("#istest"); if (it) it.addEventListener("change", async () => { const v = it.checked; if (!confirm(`${v ? "Hide" : "Unhide"} ${u.handle} as a test account?`)) { it.checked = !v; return; } try { await api(uid + "/test", { method: "POST", body: { value: v } }); msg(am, v ? "Hidden as a test account." : "No longer a test account.", true); } catch (x) { it.checked = !v; msg(am, x.message); } });
  $$("[data-wowrc]").forEach(b => b.addEventListener("click", () => { const g = u.games.find(x => x.game_id === b.dataset.wowrc); if (confirm(`Recount ${u.handle}'s ${b.dataset.gname} hours from /played only?\nCurrent total: ${g ? hrs(g.display_min) : "none"}. The carried menu-time extra is dropped; deleted characters keep their hours. Logged in the audit log.`)) act(b, async () => { const r = await api(`/admin/users/${id}/wow_recount`, { method: "POST", body: { game_id: b.dataset.wowrc } }); msg(amsg, `Recounted: ${hrs(r.total_min)}`, true); }); }));
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
const FLAG = { day_over_20h: "Over 20 h in a day", session_over_18h: "Session over 18 h", overlapping_sessions: "Overlapping sessions", faster_than_real_time: "Faster than real time",
  wow_checksum: "WoW: entry edited (bad checksum)", wow_battletag: "WoW: BattleTag doesn't match link", wow_decrease: "WoW: /played went down",
  wow_impossible: "WoW: /played grew faster than real time", wow_bnet_mismatch: "WoW: level doesn't match Battle.net", wow_clock: "WoW: timestamp in the future", wow_old_addon: "WoW: retired addon version" };
async function hoursReview(days) {
  days = Number(days) || 30;
  const show = (location.hash.split("/")[2]) || "open";
  const d = await api("/admin/hours/flags?" + new URLSearchParams({ days, show }));
  view.innerHTML = `<section class="panel"><h2>Hours review</h2>
    <div class="tools"><span class="muted">Suspicious totals in the last</span><select class="in" id="hd">${[7, 30, 90, 365].map(n => `<option ${n === days ? "selected" : ""}>${n}</option>`).join("")}</select><span class="muted">days. Days are UTC.</span>
      <a class="btn sm${show === "reviewed" ? " dl" : ""}" href="#hours/${days}/reviewed">Reviewed (${num(d.reviewed || 0)})</a>
      <a class="btn sm${show === "open" ? " dl" : ""}" href="#hours/${days}/open">Open (${num(d.open || 0)})</a></div>
    <p class="note">Mark as reviewed hides a flag only while the hours it was judging are unchanged. It does not delete sessions or change totals. A later session brings the flag back.</p>
    ${d.flags.length ? `<table><thead><tr><th>Flag</th><th>User</th><th>Game</th><th class="num">Hours</th><th>When</th><th>Detail</th><th></th></tr></thead><tbody>
    ${d.flags.map(f => `<tr data-key="${esc(f.flag_key)}"><td><span class="pill warn">${esc(FLAG[f.kind] || f.kind)}</span></td><td><a href="#user/${esc(f.user_id)}">${esc(f.handle)}</a></td>
      <td>${esc(f.game_name || "-")}</td><td class="num">${hrs(f.minutes)}</td><td>${f.kind === "day_over_20h" ? esc(f.when) : when(f.when)}</td><td class="muted">${esc(f.detail)}${f.review && f.review.matches ? `<div class="dimt">Reviewed ${when(f.review.at)}${f.review.by ? " by " + esc(f.review.by) : ""}${f.review.reason ? " · " + esc(f.review.reason) : ""}</div>` : ""}</td>
      <td class="num"><div class="acts">${f.review && f.review.matches ? `<button class="btn sm" data-reopen>Open again</button>` : `<button class="btn sm" data-reviewed>Mark as reviewed</button>`}${f.session_id ? ` <button class="btn sm danger" data-sid="${esc(f.session_id)}" data-uid="${esc(f.user_id)}">Delete session</button>` : ` <a class="btn sm" href="#user/${esc(f.user_id)}">Account</a>`}</div></td></tr>`).join("")}</tbody></table>`
    : `<div class="body empty">${show === "reviewed" ? "Nothing is marked reviewed for the current evidence." : "Nothing suspicious."}</div>`}</section>`;
  $("#hd").addEventListener("change", e => { location.hash = "hours/" + e.target.value + "/" + show; });
  view.onclick = e => {
    const b = e.target.closest("button"); if (!b) return;
    if (b.matches("[data-sid]")) { if (confirm("Delete this session?")) act(b, async () => { await api(`/admin/users/${b.dataset.uid}/sessions/${b.dataset.sid}`, { method: "DELETE" }); hoursReview(days); }); return; }
    const key = b.closest("tr") && b.closest("tr").dataset.key;
    if (b.matches("[data-reviewed]")) {
      const reason = prompt("Optional note (saved with the review). Leave blank to skip.");
      if (reason == null) return;
      act(b, async () => { await api("/admin/hours/flags/review", { method: "POST", body: { flag_key: key, days, reason } }); hoursReview(days); });
    } else if (b.matches("[data-reopen]")) {
      act(b, async () => { await api("/admin/hours/flags/reopen", { method: "POST", body: { flag_key: key, days } }); hoursReview(days); });
    }
  };
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
  const [d, al, an] = await Promise.all([api("/admin/settings"), edit ? api("/admin/alerts").catch(() => null) : null, can.announcements ? api("/admin/announcements").catch(() => null) : null]);
  const row = (s, kind) => `<tr><td style="width:56px"><label class="switch"><input type="checkbox" data-${kind}="${esc(s.key)}" ${s.enabled ? "checked" : ""} ${edit ? "" : "disabled"}><i></i></label></td>
    <td><b>${esc(s.label)}</b>${s.key === "maintenance" && s.enabled ? ` <span class="pill bad">ON</span>` : ""}${s.default !== undefined && s.enabled !== s.default ? ` <span class="pill">changed</span>` : ""}<div class="dimt">${esc(s.help)}</div></td>
    <td class="dimt" style="white-space:nowrap">${s.updated_at ? `${when(s.updated_at)}${s.updated_by ? " by " + esc(s.updated_by) : ""}` : "default"}</td></tr>`;
  const alerts = al ? `<section class="panel"><h2>Email alerts</h2><div class="body">
    <p class="note">Staff alerts by email, using the site's sending address. To keep within Cloudflare's 200 emails a day, alerts are batched (see each one) and capped at ${al.cap} a day. Sent in the last 24 hours: <b>${al.sent_24h}</b> of ${al.cap}.${al.mail_live ? "" : ` <span class="pill bad">Email sending is off on the server</span>`}</p>
    <form method="post" action="/api/v1/form-fallback" class="f" id="al-to" style="max-width:560px"><label>Send alerts to<div class="row"><input class="in" type="email" name="to" value="${esc(al.recipient)}" required maxlength="200" style="flex:1"><button class="btn">Save</button></div></label>
      <div class="dimt">Default: ${esc(al.default_recipient)}</div><div class="alert" id="al-msg" hidden></div></form>
    <table class="settings"><tbody>${al.alerts.map(a => row(a, "alert")).join("")}</tbody></table>
    <div class="row" style="margin-top:12px"><button class="btn" id="al-test">Send a test alert</button><span class="dimt">Sends today's summary marked [Test] to the address above. Counts toward the daily cap.</span></div>
    ${al.recent.length ? `<h3 style="margin:16px 0 6px">Recent alerts</h3><table class="t"><tbody>${al.recent.map(r => `<tr><td class="dimt" style="white-space:nowrap">${when(r.at)}</td><td>${esc(r.subject)}</td><td>${r.ok ? `<span class="pill ok">sent</span>` : `<span class="pill warn" title="${esc(r.error || "")}">not sent</span>`}</td></tr>`).join("")}</tbody></table>` : ""}
    </div></section>` : "";
  view.innerHTML = `<section class="panel"><h2>Site settings</h2><div class="body">
    <p class="note">Switches apply to the website and the desktop app within about 30 seconds, and the server enforces them (hiding a button is not the only effect). Every change is written to the audit log.${edit ? "" : " Only admins can change them."} <a href="#suggested">Suggested Merge</a> compares the catalogue with launcher ids we actually have.</p>
    <table class="settings"><tbody>${d.settings.map(s => row(s, "key")).join("")}</tbody></table></div></section>
    <section class="panel" id="hc"><h2>Home page user counts</h2><div class="body"><p class="note">How the registered and online player counts show at the top of the home page.</p>
    ${(d.home_counts_options || []).map(o => `<label style="display:block;margin:4px 0"><input type="radio" name="home_counts" value="${esc(o.key)}" ${o.key === d.home_counts ? "checked" : ""} ${edit ? "" : "disabled"}> <b>${esc(o.key.toUpperCase())}</b> · ${esc(o.label)}</label>`).join("")}</div></section>
    ${notice(an)}
    <section class="panel" id="links"><h2>Download page links</h2><div class="body"><p class="note">Change where the Download page points without a deploy. Live within about 30 seconds. Leave a box empty to use the built-in link.</p>
    ${(d.links || []).map(l => `<form method="post" action="/api/v1/form-fallback" class="f" data-link="${esc(l.key)}" style="max-width:640px;margin-bottom:12px"><label>${esc(l.label)}<div class="row"><input class="in" type="url" name="v" value="${esc(l.value)}" maxlength="200" placeholder="https://… (empty: built-in)" style="flex:1" ${edit ? "" : "disabled"}>${edit ? `<button class="btn">Save</button>` : ""}</div></label><div class="dimt">${esc(l.help)}${l.updated_at && l.value !== undefined ? ` · changed ${when(l.updated_at)}${l.updated_by ? " by " + esc(l.updated_by) : ""}` : ""}</div><div class="alert" hidden></div></form>`).join("")}</div></section>${alerts}`;
  $$("form[data-link]").forEach(f => f.addEventListener("submit", async e => {
    e.preventDefault();
    try { await api("/admin/settings", { method: "POST", body: { key: f.dataset.link, value: f.v.value.trim() } }); msg($(".alert", f), "Saved. Live within about 30 seconds.", true); }
    catch (x) { msg($(".alert", f), x.message); }
  }));
  wireNotice(siteSettings);
  view.onchange = async e => {
    const a = e.target.closest("input[data-alert]");
    if (a) {
      const on = a.checked; a.disabled = true;
      try { await api("/admin/alerts", { method: "POST", body: { key: a.dataset.alert, enabled: on } }); siteSettings(); }
      catch (x) { a.checked = !on; a.disabled = false; alert(x.message); }
      return;
    }
    const hc = e.target.closest("input[name=home_counts]");
    if (hc) {
      try { await api("/admin/settings", { method: "POST", body: { key: "home_counts", value: hc.value } }); siteSettings(); }
      catch (x) { alert(x.message); siteSettings(); }
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

/* Site notice: the one-line banner (same as Announcements), from Site settings. */
function notice(an) {
  if (!an) return "";
  const a = an.active;
  return `<section class="panel" id="notice"><h2>Site notice <span class="more"><a href="#announcements">Full options</a></span></h2><div class="body">
    ${a ? `<p class="muted">Showing now (${esc(a.style)}${a.ends_at ? ", ends " + when(a.ends_at) : ", no end time"}):</p><div class="announce ${esc(a.style)}" style="border:1px solid;border-radius:3px"><div class="wrap" style="padding:6px 10px"><span>${esc(a.message)}</span></div></div><div class="row" style="margin-top:8px"><button class="btn sm danger" id="nclr">Clear notice</button></div>` : `<p class="empty" style="padding:0 0 8px">No notice is showing.</p>`}
    <form method="post" action="/api/v1/form-fallback" class="f" id="nf" style="max-width:640px;margin-top:10px"><label>Banner text<input type="text" name="message" maxlength="300" required placeholder="Servers restart at 10 pm AEDT for an update."></label>
      <div class="row"><select class="in" name="style"><option value="info">Info (blue)</option><option value="warning">Warning (amber)</option></select><label style="margin:0">Ends (optional) <input class="in" type="datetime-local" name="ends"></label><button class="btn dl">Show notice</button></div>
      <div class="dimt">Shows at the top of every website page and in the app within a few minutes. Replaces the current one.</div><div class="alert" id="nmsg" hidden></div></form></div></section>`;
}
function wireNotice(again) {
  const c = $("#nclr"); if (c) c.addEventListener("click", () => { if (confirm("Clear the site notice?")) act(c, async () => { await api("/admin/announcements/clear", { method: "POST" }); again(); }); });
  const f = $("#nf"); if (f) f.addEventListener("submit", async e => {
    e.preventDefault();
    try { await api("/admin/announcements", { method: "POST", body: { message: f.message.value.trim(), style: f.style.value, link: null, ends_at: f.ends.value ? new Date(f.ends.value).toISOString() : null } }); again(); }
    catch (x) { msg($("#nmsg"), x.message); }
  });
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
    <form method="post" action="/api/v1/form-fallback" class="f" id="af"><label>Message<input type="text" name="message" maxlength="300" required placeholder="Servers restart at 10 pm AEDT for an update."></label>
      <div class="row"><label>Style<select class="in" name="style"><option value="info">Info (blue)</option><option value="warning">Warning (amber)</option></select></label>
      <label>Ends (optional)<input class="in" type="datetime-local" name="ends"></label></div>
      <label>Cover image (optional)<input type="text" name="image" placeholder="https://… or upload below"></label>
      <div class="row" style="align-items:center"><input type="file" id="nf-file" accept="image/jpeg,image/png,image/webp,image/gif"><button type="button" class="btn sm" id="nf-up">Upload</button><button type="button" class="btn sm danger" id="nf-rm" hidden>Remove image</button></div>
      <p class="dimt">JPG, PNG, WebP or GIF up to 5 MB. Shows beside the post in lists and full width at the top of the post.</p>
      <img id="nf-prev" alt="" hidden style="max-width:100%;max-height:180px;border-radius:4px;object-fit:contain;align-self:flex-start">
      <div class="row"><label>Button link (optional)<input type="text" name="link" placeholder="https://discord.gg/… or /download"></label>
        <label>Button label<input type="text" name="link_label" maxlength="40" placeholder="Read more"></label></div>
      <p class="dimt">Adds a button under the post. Links typed in the post itself are already clickable.</p>
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

// Only server-rendered, sanitized HTML enters previews. No Markdown parser or
// raw textarea-to-innerHTML path exists in the browser.
const MD = (() => {
  const tools = [['bold','Bold'],['italic','Italic'],['link','Link'],['bullet','Bullet list'],['code','Code'],['break','Line break'],['paragraph','Paragraph']];
  const toolbar = name => `<div class="md-toolbar" role="toolbar" aria-label="Format ${esc(name)}" data-md-for="${esc(name)}">${tools.map(([action,label])=>`<button class="btn sm" type="button" data-md="${action}">${label}</button>`).join('')}</div>`;
  const edit = (value, start, end, action, url) => {
    let text = value.slice(start,end), insert, inner = 0;
    if (action === 'bullet') {
      start = value.lastIndexOf('\n',start-1)+1;
      const edge = end>start && value[end-1]==='\n' ? end-1 : end;
      const nl = value.indexOf('\n',edge); end = nl<0 ? value.length : nl;
      text = value.slice(start,end);
      insert = text.trim() ? text.split('\n').map(line=>line.trim() && !/^\s*[-*+]\s/.test(line) ? '- '+line : line).join('\n') : '- ';
    } else if (action === 'break') insert='\n';
    else if (action === 'paragraph') insert='\n\n';
    else if (action === 'link') {
      if (!url || !/^(https?:\/\/|\/(?!\/)|#)/i.test(url) || /[\s<>"']/.test(url)) return null;
      text = text || 'link text'; insert='['+text.replace(/\]/g,'\\]')+']('+url.replace(/[()]/g,'\\$&')+')'; inner=1;
    } else {
      const mark = {bold:'**',italic:'*',code:'`'}[action]; if (!mark) return null;
      text = text || {bold:'bold text',italic:'italic text',code:'code'}[action]; insert=mark+text+mark;inner=mark.length;
    }
    return {value:value.slice(0,start)+insert+value.slice(end),start:start+inner,end:inner ? start+inner+text.length : start+insert.length};
  };
  const bind = form => form.addEventListener('click',e=>{
    const b=e.target.closest('[data-md]'); if (!b || !form.contains(b)) return;
    const box=b.closest('[data-md-for]'),ta=form.elements.namedItem(box.dataset.mdFor); if (!ta) return;
    const url=b.dataset.md==='link' ? prompt('Link URL','https://') : undefined;
    if (url===null) return;
    const r=edit(ta.value,ta.selectionStart,ta.selectionEnd,b.dataset.md,url && url.trim());
    if (!r) { if (b.dataset.md==='link') alert('Use an https://, http://, / or # link.'); return; }
    ta.value=r.value;ta.focus();ta.setSelectionRange(r.start,r.end);ta.dispatchEvent(new Event('input',{bubbles:true}));
  });
  const preview = (payload,draw,fail) => {
    let seq=0,timer;
    return () => { const ticket=++seq;clearTimeout(timer);timer=setTimeout(async()=>{
      try { const result=await api('/admin/markdown/preview',{method:'POST',body:payload()});if(ticket===seq) draw(result); }
      catch (_) { if(ticket===seq) fail(); }
    },180); };
  };
  const blocks = value => value.trim() ? [value.trim()] : [];
  const legacy = items => items.map(s=>'- '+String(s).replace(/[\\`*_\[\]<>#!~.+\-()={}]/g,'\\$&').replace(/\n/g,'\n  ')).join('\n');
  return {toolbar,edit,bind,preview,blocks,legacy};
})();

/* ---------- 6b. news ---------- */
/* Posts for the home page News box, /news and /news/<slug>. Separate from the one-line
   announcement banner; "Also show as banner" publishes a banner that links to the post. */
async function news(arg) {
  const PER = 20; let pg = 1, q = "";
  view.innerHTML = `<div class="split"><section class="panel"><h2 id="nf-h">New post</h2><div class="body">
    <p class="note">News posts show in the <b>News</b> box on the home page (latest 4, pinned first), on <a href="/news" target="_blank">/news</a>, and each has its own page. For a short alert on every page and in the app, use <a href="#announcements">Announcements</a>, or tick “Also show as banner” below.</p>
    <form method="post" action="/api/v1/form-fallback" class="f" id="nf"><input type="hidden" name="id">
      <label>Title<input type="text" name="title" maxlength="140" required placeholder="Fraglink 0.3.0 beta 1 is out"></label>
      <label for="nf-body">Post</label>${MD.toolbar("body")}<textarea class="in" id="nf-body" name="body" rows="10" maxlength="20000" placeholder="Markdown: **bold**, *italic*, links, lists and code. Blank line = new paragraph."></textarea>
      <section class="md-preview"><h3>Preview</h3><div class="md-content txt" id="nf-mdprev" aria-live="polite"></div></section>
      <label>Cover image (optional)<input type="text" name="image" placeholder="https://… or upload below"></label>
      <div class="row" style="align-items:center"><input type="file" id="nf-file" accept="image/jpeg,image/png,image/webp,image/gif"><button type="button" class="btn sm" id="nf-up">Upload</button><button type="button" class="btn sm danger" id="nf-rm" hidden>Remove image</button></div>
      <p class="dimt">JPG, PNG, WebP or GIF up to 5 MB. Shows beside the post in lists and full width at the top of the post.</p>
      <img id="nf-prev" alt="" hidden style="max-width:100%;max-height:180px;border-radius:4px;object-fit:contain;align-self:flex-start">
      <div class="row"><label>Button link (optional)<input type="text" name="link" placeholder="https://discord.gg/… or /download"></label>
        <label>Button label<input type="text" name="link_label" maxlength="40" placeholder="Read more"></label></div>
      <p class="dimt">Adds a button under the post. Links typed in the post itself are already clickable.</p>
      <div class="row"><label>Date<input class="in" type="datetime-local" name="at"></label>
        <label style="display:flex;flex-direction:row;align-items:center;gap:6px;align-self:flex-end;padding-bottom:8px"><input type="checkbox" name="pinned"> Pin to top</label>
        <label style="display:flex;flex-direction:row;align-items:center;gap:6px;align-self:flex-end;padding-bottom:8px" id="nf-ban"><input type="checkbox" name="banner"> Also show as banner</label></div>
      <p class="dimt">Leave the date empty to publish now. A future date keeps the post hidden until then.</p>
      <div class="row"><button class="btn dl" id="nf-go">Publish</button><button type="button" class="btn" id="nf-cancel" hidden>Cancel edit</button></div>
      <div class="alert" id="nfmsg" hidden></div></form></div></section>
    <section class="panel"><h2>Posts <span class="dimt" id="n-count"></span></h2><div class="body" style="padding-bottom:4px"><input class="in" type="search" id="n-q" placeholder="Search posts" style="width:100%"></div><div id="n-list"></div></section></div>`;
  const f = $("#nf"), local = iso => { const d = new Date(iso); d.setMinutes(d.getMinutes() - d.getTimezoneOffset()); return d.toISOString().slice(0, 16); };
  let items = [], newsFormat = "markdown";
  const previewBody = MD.preview(()=>({kind:"news",format:newsFormat,body:f.body.value}),d=>{if(f.isConnected) $("#nf-mdprev").innerHTML=d.html;},()=>{if(f.isConnected) $("#nf-mdprev").textContent="Couldn't load the preview.";});
  MD.bind(f); f.body.addEventListener("input",()=>{newsFormat="markdown";previewBody();}); previewBody();
  const reset = () => { f.reset(); newsFormat="markdown";previewBody(); f.id.value = ""; $("#nf-h").textContent = "New post"; $("#nf-go").textContent = "Publish"; $("#nf-cancel").hidden = true; $("#nf-ban").hidden = false; prev(); };
  const fill = p => {
    newsFormat=p.format || null;
    f.id.value = p.id; f.title.value = p.title; f.body.value = p.body || ""; f.link.value = p.link || ""; f.link_label.value = p.link_label || ""; f.image.value = p.image || ""; prev(); previewBody(); f.at.value = local(p.at); f.pinned.checked = p.pinned;
    $("#nf-h").textContent = "Edit post"; $("#nf-go").textContent = "Save changes"; $("#nf-cancel").hidden = false; $("#nf-ban").hidden = true; f.title.focus(); window.scrollTo(0, 0);
  };
  function prev() { const u = f.image.value.trim(), im = $("#nf-prev"); im.hidden = !u; $("#nf-rm").hidden = !u; if (u) im.src = u; else im.removeAttribute("src"); }
  f.image.addEventListener("input", prev);
  $("#nf-prev").addEventListener("error", () => { if (f.image.value.trim()) msg($("#nfmsg"), "That image couldn't be loaded."); });
  $("#nf-rm").addEventListener("click", () => { f.image.value = ""; $("#nf-file").value = ""; prev(); });
  $("#nf-up").addEventListener("click", e => act(e.target, async () => {
    const file = $("#nf-file").files[0]; if (!file) throw new Error("Pick an image first.");
    if (file.size > 5 * 1024 * 1024) throw new Error("That image is over 5 MB.");
    const r = await fetch(window.FL.API + "/admin/news-image", { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": file.type || "application/octet-stream" }, body: file });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(j.message || "Upload failed (HTTP " + r.status + ").");
    f.image.value = j.url; prev();
  }).catch(x => msg($("#nfmsg"), x.message)));
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
    const body = { title: f.title.value.trim(), body: f.body.value, format: newsFormat, link: f.link.value.trim(), link_label: f.link_label.value.trim(), image: f.image.value.trim(), pinned: f.pinned.checked };
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
    <form method="post" action="/api/v1/form-fallback" class="tools" id="sf"><input class="in grow" name="handle" placeholder="Username" required><select class="in" name="role">${roleOpts.map(r => `<option>${r}</option>`).join("")}</select><button class="btn">Add to staff</button></form>
    <div class="note">Owner: everything, and the only one who adds or removes admins. Admin: everything except managing admins. Moderator: game approvals, user ban/unban and hours review.</div>
    <div class="alert" id="smsg" hidden style="margin:0 12px 12px"></div></section>
    ${owner ? `<section class="panel"><h2>Transfer ownership</h2><div class="body"><form method="post" action="/api/v1/form-fallback" class="f" id="tf"><label>New owner's username<input type="text" name="handle" required></label>
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

function previewHtml(p, o) {
  const mins = list => (list || []).length ? list.map(n => hrs(n)).join(", ") : "none";
  const moveTitle = p.effect === "merge" ? "Would move" : "Stays on this id";
  const stayTitle = p.effect === "merge" ? "Stays" : "Catalogue game, unchanged";
  const box = (title, g, rows) => `<div class="sm-card"><div class="k">${title}</div><div class="nm">${esc((g && g.name) || (g && g.id) || "")}</div><div class="id">${esc((g && g.id) || "")}</div>
    <div class="sm">${g && g.in_database ? "Saved row" : "No database row"}${g && g.on_list ? " · on the catalogue" : ""}</div>
    <dl><dt>Sessions</dt><dd>${num(rows.sessions)} · ${num(rows.session_seconds)} seconds</dd>
    <dt>Imported rows</dt><dd>${mins(rows.imported_minutes)}</dd>
    <dt>Estimated, stored</dt><dd>${rows.estimated_rows ? hrs(rows.estimated_minutes_stored) : "none"}</dd>
    <dt>Steam snapshots</dt><dd>${mins(rows.steam_minutes)}</dd></dl></div>`;
  const shown = p.projection && p.projection.status === "known" ? hrs(p.projection.display_minutes) : "not calculated";
  return `<div class="sm-prev" role="status">
    <div class="sm-cols">${box(moveTitle, p.moves, p.moving_rows)}${box(stayTitle, p.stays, p.staying_rows)}</div>
    <p>${esc(p.change || "")}</p>
    ${p.effect === "merge" ? `<p><b>Shown total if merged:</b> ${shown}. ${esc((p.projection && p.projection.rule) || "")}</p>` : ""}
    ${((p.projection && p.projection.notes) || []).map(n => `<p>${esc(n)}</p>`).join("")}
    ${(p.warnings || []).map(w => `<div class="alert">${esc(w)}</div>`).join("")}
    ${(p.blocked || []).map(w => `<div class="alert err">${esc(w)}</div>`).join("")}
    <p class="dimt">${num(p.aliases)} detection fix-up${p.aliases === 1 ? "" : "s"} on the first id. ${esc(p.alias_note || "")} Preview did not change any rows.</p>
    ${p.can_merge ? `<form class="sm-confirm" data-confirm>
      <label>Type <span class="mono">${esc(o.source)}</span> to merge into ${esc((p.stays && p.stays.name) || o.target)}
      <input class="in" name="confirm" autocomplete="off" spellcheck="false" aria-label="Type the source id to confirm the merge"></label>
      <button class="btn danger" type="submit" disabled>Merge</button></form>` : ""}
    ${p.effect === "link" && o.link ? `<p><button class="btn" type="button" data-linkdo>Link, don't move hours</button></p>` : ""}
  </div>`;
}

// Edit game > Merge into…: read-only preview, then a typed confirm only when a merge is safe.
function wireMerge(id) {
  const q = $("#mrgq"); if (!q) return;
  const gid = "/admin/games/" + encodeURIComponent(id), out = $("#mrgout"), m = $("#mrgmsg");
  let tm;
  q.addEventListener("input", () => { clearTimeout(tm); tm = setTimeout(async () => {
    const v = q.value.trim(); if (v.length < 2) return;
    const d = await api("/admin/games?q=" + encodeURIComponent(v)).catch(() => ({ games: [] }));
    $("#mrg-games").innerHTML = d.games.filter(x => x.id !== id).map(x => `<option value="${esc(x.id)}">${esc(x.public_name || x.name)}</option>`).join("");
  }, 250); });
  $("#mrgp").addEventListener("click", e => act(e.target, async () => {
    const into = q.value.trim(); if (!into) throw new Error("Pick the game to merge into.");
    const p = await api(gid + "/merge?into=" + encodeURIComponent(into));
    out.innerHTML = previewHtml(p, { source: id, target: into, link: false });
    const form = $("[data-confirm]", out);
    if (form) {
      const input = $("input", form), go = $("button", form);
      input.addEventListener("input", () => { go.disabled = input.value.trim() !== id; });
      form.addEventListener("submit", ev => {
        ev.preventDefault();
        if (input.value.trim() !== id) return msg(m, "The id didn't match; nothing was merged.");
        act(go, async () => { await api(gid + "/merge", { method: "POST", body: { into, confirm: input.value.trim() } }); relist(into); }).catch(x => msg(m, x.message));
      });
    }
  }).catch(x => msg(m, x.message)));
}

// Edit game > Pictures: pick where the icon and the banner art come from.
async function renderMedia(id, g) {
  const box = $("#media"), gid = "/admin/games/" + encodeURIComponent(id), t = "?t=" + Date.now();
  let m;
  try { m = await api(gid + "/media"); } catch (x) { box.innerHTML = `<div class="alert">${esc(x.message)}</div>`; return; }
  const own = { icon: g.icon64 + t, art: `/api/v1/game-art/${encodeURIComponent(id)}.jpg${t}` };
  const ph = k => `<span class="mtile-none ${k}">none</span>`;
  const img = (k, src) => src ? `<img class="${k}" src="${esc(src)}" alt="" data-broken-swap="${k}">` : ph(k);
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
    renderMedia(id, g); const gs = $("#gs"); if (gs) listGames(gs.q.value.trim()); else if ($("#aglist")) listAll();
  }).catch(x => msg($("#mmsg"), x.message)));
  box.querySelectorAll(".mpick").forEach(p => {
    const k = p.dataset.k, out = $("[data-msg]", p);
    const pick = async () => { await api(gid + "/media", { method: "POST", body: { [k]: "custom" } }); renderMedia(id, g); };
    $("[data-link]", p).addEventListener("click", e => act(e.target, async () => {
      await api(gid + (k === "icon" ? "/icon/url" : "/art/url"), { method: "POST", body: { url: $("[data-url]", p).value.trim() } }); await pick();
    }).catch(x => msg(out, x.message)));
    $("[data-up]", p).addEventListener("click", e => act(e.target, async () => {
      const file = $("[data-file]", p).files[0]; if (!file) throw new Error("Pick an image first.");
      const r = await fetch(window.FL.API + gid + (k === "icon" ? "/icon" : "/art"), { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": file.type || "application/octet-stream" }, body: file });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.message || "Upload failed (HTTP " + r.status + ").");
      await pick();
    }).catch(x => msg(out, x.message)));
  });
}

/* ---------- polls (home page Quick poll) ---------- */
async function polls() {
  const d = await api("/admin/polls");
  const bars = p => p.options.map(o => `<div style="margin:3px 0"><b>${esc(o.label)}</b> · ${o.votes} (${o.pct}%)<div class="bar" style="height:6px;background:#1c2a3a"><i style="display:block;height:6px;width:${o.pct}%;background:#e08a20"></i></div></div>`).join("");
  const st = p => p.active ? (p.closed ? `<span class="role">Active · closed</span>` : `<span class="role">Active</span>`) : (p.closed ? "Closed" : "Not shown");
  view.innerHTML = `<section class="panel"><h2>New poll</h2><div class="body"><form id="pn">
    <p class="note">The active poll shows in the <b>Quick poll</b> box on the home page. One vote per account (players can change it); test accounts aren't counted. Closing keeps it on the home page with final results; making another poll active replaces it. Past polls are kept.</p>
    <p><input name="q" maxlength="140" placeholder="Question" style="width:100%" required></p>
    ${[1,2,3,4,5,6].map(i => `<p style="margin:4px 0"><input name="o" maxlength="60" placeholder="Answer ${i}${i > 2 ? " (optional)" : ""}" ${i <= 2 ? "required" : ""} style="width:60%"></p>`).join("")}
    <label><input type="checkbox" name="act" checked> Make it the active poll now</label>
    <p><button class="btn dl">Create poll</button></p></form></div></section>
    <section class="panel"><h2>All polls (${d.polls.length})</h2><div class="body">${d.polls.length ? d.polls.map(p => `<div class="fr" style="display:block;padding:10px 0;border-bottom:1px solid #202c3a" data-id="${p.id}">
      <div><b>${esc(p.question)}</b> · ${st(p)} · ${p.total} vote${p.total === 1 ? "" : "s"} · <span class="dimt">${new Date(p.created_at).toLocaleDateString()}</span></div>${bars(p)}
      <div style="margin-top:6px">${p.active && !p.closed ? "" : `<button class="btn sm" data-act="activate">${p.active ? "Reopen" : "Make active"}</button> `}${p.closed ? "" : `<button class="btn sm" data-act="close">Close voting</button>`} <button class="btn sm danger" data-act="delete">Delete</button></div></div>`).join("") : `<div class="empty">No polls yet.</div>`}</div></section>`;
  $("#pn").onsubmit = async e => {
    e.preventDefault(); const f = e.target;
    try { await api("/admin/polls", { method: "POST", body: { question: f.q.value, options: [...f.querySelectorAll("[name=o]")].map(i => i.value), activate: f.act.checked } }); polls(); }
    catch (x) { alert(x.message); }
  };
  view.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    const id = b.closest("[data-id]").dataset.id, act = b.dataset.act;
    if (act === "close" && !confirm("Close voting on this poll? Results stay visible.")) return;
    if (act === "delete" && !confirm("Delete this poll and all its votes? This can't be undone.")) return;
    try { await api(`/admin/polls/${id}/${act}`, { method: "POST" }); polls(); } catch (x) { alert(x.message); }
  };
}

async function incidents() {
  const d = await api("/admin/incidents");
  const when = t => new Date(t).toLocaleString();
  view.innerHTML = `<section class="panel"><h2>New incident</h2><div class="body"><form id="inn">
    <p class="note">Shows under <b>Past incidents</b> on the public <a href="/status">Status</a> page. Write it for players: what broke and what they should do.</p>
    <p><input name="t" maxlength="140" placeholder="Title, e.g. Sign-in failing for some players" style="width:100%" required></p>
    <p><textarea name="b" maxlength="2000" rows="3" placeholder="Details (optional)" style="width:100%"></textarea></p>
    <p><select name="s"><option value="minor">Minor</option><option value="major">Major</option><option value="maintenance">Maintenance</option></select>
    <label><input type="checkbox" name="r"> Already resolved</label></p>
    <p><button class="btn dl">Add incident</button></p></form></div></section>
    <section class="panel"><h2>Incidents (${d.incidents.length})</h2><div class="body">${d.incidents.length ? d.incidents.map(i => `<div class="fr" style="display:block;padding:8px 0;border-bottom:1px solid #202c3a" data-id="${i.id}">
      <b>${esc(i.title)}</b> · ${esc(i.severity)} · <span class="dimt">${when(i.started_at)}${i.resolved_at ? " – " + when(i.resolved_at) : " · ongoing"}</span>${i.body ? `<div>${esc(i.body)}</div>` : ""}
      <div style="margin-top:5px">${i.resolved_at ? "" : `<button class="btn sm" data-act="resolve">Mark resolved</button> `}<button class="btn sm red" data-act="delete">Delete</button></div></div>`).join("") : `<div class="empty">No incidents yet.</div>`}</div></section>`;
  $("#inn").onsubmit = async e => {
    e.preventDefault(); const f = e.target, now = new Date().toISOString();
    try { await api("/admin/incidents", { method: "POST", body: { title: f.t.value, body: f.b.value, severity: f.s.value, started_at: now, resolved_at: f.r.checked ? now : null } }); incidents(); }
    catch (x) { alert(x.message); }
  };
  view.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    const id = b.closest("[data-id]").dataset.id, act = b.dataset.act;
    if (act === "delete" && !confirm("Delete this incident from the Status page?")) return;
    try { await api(`/admin/incidents/${id}/${act}`, { method: "POST" }); incidents(); } catch (x) { alert(x.message); }
  };
}

/* ---------- Changelog: write, preview and publish /changelog entries ---------- */
const CL = { offset: 0, q: "" };
const clLines = v => v.split("\n").map(l => l.replace(/^\s*[-*•]\s*/, "").trim()).filter(Boolean);
const clLabel = d => { const t = new Date(d + "T00:00:00"); return isNaN(t) ? d : t.toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" }); };
function clPreview(e) {
  const list = (h, items, html) => items.length ? `<h4>${h}</h4><div class="md-content">${typeof html === "string" ? html : `<ul>${items.map(i => `<li>${esc(i)}</li>`).join("")}</ul>`}</div>` : "";
  return `<section class="panel"><h2><span class="cl-tag ${esc(e.area)}">${esc(e.area)}</span>${esc(clLabel(e.date))}</h2><div class="body">${e.version ? `<h3>${esc(e.version)}</h3>` : ""}${list("New", e.new, e.new_html)}${list("Changes", e.changes, e.changes_html)}${list("Fixes", e.fixes, e.fixes_html)}${e.new.length + e.changes.length + e.fixes.length ? "" : `<p class="dimt">Add at least one line.</p>`}</div></section>`;
}
function clText(e) {
  const part = (h, items) => items.length ? `**${h}**\n${e.format === "markdown" ? items.join("\n\n") : items.map(i => "- " + i).join("\n")}\n` : "";
  return `**Fraglink ${e.area === "App" ? "app" : "website"} · ${clLabel(e.date)}${e.version ? " · " + e.version : ""}**\n${part("New", e.new)}${part("Changes", e.changes)}${part("Fixes", e.fixes)}`.trim();
}
async function changelog(arg) {
  const today = new Date().toLocaleDateString("en-CA");
  view.innerHTML = `<div class="split cl-ed"><section class="panel"><h2 id="clh">New entry</h2><form method="post" action="/api/v1/form-fallback" class="f body" id="clf">
      <input type="hidden" name="eid" value="">
      <div class="row"><label style="margin:0">Area<select class="in" name="area" style="min-width:110px"><option>Website</option><option>App</option></select></label><label style="margin:0">Date<input class="in" type="date" name="date" value="${today}" required></label><label style="margin:0;flex:1">Version (app only)<input class="in" type="text" name="version" maxlength="40" placeholder="0.3.0-beta.11"></label></div>
      <label for="cl-new">New</label>${MD.toolbar("new")}<textarea id="cl-new" name="new" rows="4" maxlength="20000" placeholder="- One change\n- Another change"></textarea>
      <label for="cl-changes">Changes</label>${MD.toolbar("changes")}<textarea id="cl-changes" name="changes" rows="4" maxlength="20000"></textarea>
      <label for="cl-fixes">Fixes</label>${MD.toolbar("fixes")}<textarea id="cl-fixes" name="fixes" rows="4" maxlength="20000"></textarea>
      <div class="dimt">Markdown: **bold**, *italic*, links, lists and code. A blank line starts a paragraph. Saved as a draft until you publish it.</div>
      <div class="row"><button class="btn">Save draft</button><button type="button" class="btn dl" id="clpub">Save and publish</button><button type="button" class="btn" id="clcopy">Copy for Discord</button><button type="button" class="btn" id="clnew" hidden>New entry</button></div>
      <div class="alert" id="clmsg" hidden></div></form></section>
    <section class="panel"><h2>Preview <span class="more">as it shows on /changelog</span></h2><div class="body changelog" id="clprev"></div></section></div>
    <section class="panel" style="margin-top:12px"><h2>Entries <span class="dimt" id="clcount" style="text-transform:none;letter-spacing:0;font-weight:400"></span></h2>
      <div class="ag-tb"><input class="in" type="search" id="clq" placeholder="Search the entries" value="${esc(CL.q)}"><span class="dimt" style="margin-left:auto">Only entries written here. The shipped ones stay in the site files.</span></div><div id="cllist"><div class="body empty">Loading…</div></div></section>`;
  const f = $("#clf");
  let clFormat = "markdown", legacyEntry = null;
  const cur = () => ({ area: f.area.value, date: f.date.value, version: f.version.value.trim(), format:clFormat, new: clFormat ? MD.blocks(f.new.value) : legacyEntry.new, changes: clFormat ? MD.blocks(f.changes.value) : legacyEntry.changes, fixes: clFormat ? MD.blocks(f.fixes.value) : legacyEntry.fixes });
  const prev = MD.preview(()=>({kind:"changelog",...cur()}),d=>{if(f.isConnected) $("#clprev").innerHTML=clPreview({...cur(),...d});},()=>{if(f.isConnected) $("#clprev").textContent="Couldn't load the preview.";});
  MD.bind(f); f.addEventListener("input",e=>{if(["new","changes","fixes"].includes(e.target.name)) clFormat="markdown";prev();}); prev();
  const fill = e => {
    clFormat = e ? e.format || null : "markdown"; legacyEntry = e;
    f.eid.value = e ? e.id : ""; f.area.value = e ? e.area : "Website"; f.date.value = e ? e.date : today; f.version.value = e ? e.version || "" : "";
    const text = items => e.format === "markdown" ? items.join("\n\n") : MD.legacy(items);
    f.new.value = e ? text(e.new) : ""; f.changes.value = e ? text(e.changes) : ""; f.fixes.value = e ? text(e.fixes) : "";
    $("#clh").textContent = e ? `Edit entry · ${e.published_at ? "published" : "draft"}` : "New entry"; $("#clnew").hidden = !e; prev();
  };
  const save = async publish => {
    const b = { ...cur() }; if (f.eid.value) b.id = +f.eid.value; if (!b.version) b.version = null;
    try {
      const r = await api("/admin/changelog", { method: "POST", body: b });
      if (publish) await api(`/admin/changelog/${r.entry.id}/publish`, { method: "POST", body: { publish: true } });
      fill({ ...r.entry, published_at: publish || r.entry.published_at });
      msg($("#clmsg"), publish ? "Published. It's on /changelog within a minute." : "Draft saved.", true); listCl();
    } catch (x) { msg($("#clmsg"), x.message); }
  };
  f.addEventListener("submit", e => { e.preventDefault(); save(false); });
  $("#clpub").onclick = () => { if (confirm("Publish this entry on /changelog?")) save(true); };
  $("#clnew").onclick = () => fill(null);
  $("#clcopy").onclick = async () => { try { await navigator.clipboard.writeText(clText(cur())); msg($("#clmsg"), "Copied. Paste it into Discord yourself.", true); } catch (x) { msg($("#clmsg"), "Couldn't copy: " + x.message); } };
  let t; $("#clq").oninput = e => { clearTimeout(t); t = setTimeout(() => { CL.q = e.target.value.trim(); CL.offset = 0; listCl(); }, 300); };
  let rows = [];
  async function listCl() {
    const d = await api("/admin/changelog?" + new URLSearchParams({ offset: CL.offset, q: CL.q }));
    rows = d.rows; $("#clcount").textContent = `· ${num(d.total)}${d.drafts ? ` · ${num(d.drafts)} draft${d.drafts === 1 ? "" : "s"}` : ""}`;
    const n = r => r.format === "markdown" ? [...r.new,...r.changes,...r.fixes].join("\n").split("\n").filter(s=>s.trim()).length : r.new.length + r.changes.length + r.fixes.length;
    $("#cllist").innerHTML = rows.length ? `<table class="gl"><thead><tr><th>Date</th><th>Area</th><th>Version</th><th>First line</th><th class="nr">Lines</th><th>Status</th><th>Last edit</th><th></th></tr></thead><tbody>${rows.map(r => `<tr data-id="${r.id}"><td>${esc(r.label)}</td><td><span class="cl-tag ${esc(r.area)}">${esc(r.area)}</span></td><td class="mono">${esc(r.version || "-")}</td><td><div class="exes">${esc((r.new[0] || r.changes[0] || r.fixes[0] || ""))}</div></td><td class="nr">${n(r)}</td>
      <td>${r.published_at ? `<span class="st ok">Published</span><div class="sm">${when(r.published_at)}</div>` : `<span class="st hid">Draft</span>`}</td><td><div class="sm">${when(r.updated_at)}</div>${r.updated_by ? `<div class="who">${esc(r.updated_by)}</div>` : ""}</td>
      <td class="rt" style="white-space:nowrap"><button class="btn sm" data-cl="edit">Edit</button> <button class="btn sm${r.published_at ? "" : " dl"}" data-cl="${r.published_at ? "unpublish" : "publish"}">${r.published_at ? "Unpublish" : "Publish"}</button> <button class="btn sm danger" data-cl="delete">Delete</button></td></tr>`).join("")}</tbody></table>
      ${d.total > d.page_size ? `<div class="pg"><button data-cloff="${Math.max(0, d.offset - d.page_size)}"${d.offset ? "" : " disabled"}>‹ Prev</button><span class="sp">${num(d.offset + 1)}–${num(Math.min(d.total, d.offset + d.page_size))} of ${num(d.total)}</span><button data-cloff="${d.offset + d.page_size}"${d.offset + d.page_size < d.total ? "" : " disabled"}>Next ›</button></div>` : ""}`
      : `<div class="body empty">${CL.q ? "No entries match." : "No entries written here yet."}</div>`;
  }
  $("#cllist").onclick = async e => {
    const pg = e.target.closest("[data-cloff]"); if (pg) { CL.offset = +pg.dataset.cloff; listCl(); return; }
    const b = e.target.closest("button[data-cl]"); if (!b) return;
    const r = rows.find(x => String(x.id) === b.closest("tr").dataset.id), a = b.dataset.cl;
    if (a === "edit") { fill(r); f.scrollIntoView({ block: "start" }); return; }
    let confirmWord = "";
    if (a === "delete") {
      const typed = prompt(`Delete the ${r.area} entry for ${r.label}? This can't be undone.\n\nType delete to confirm:`);
      if (typed == null) return;
      if (typed.trim().toLowerCase() !== "delete") { alert("Nothing was deleted."); return; }
      confirmWord = typed.trim();
    }
    if (a === "unpublish" && !confirm("Take this entry off /changelog? It stays here as a draft.")) return;
    await act(b, async () => {
      await api(`/admin/changelog/${r.id}/${a === "delete" ? "delete" : "publish"}`, { method: "POST", body: a === "delete" ? { confirm: confirmWord } : { publish: a === "publish" } });
      if (String(f.eid.value) === String(r.id)) fill(a === "delete" ? null : { ...r, published_at: a === "publish" ? new Date().toISOString() : null });
      listCl();
    }).catch(x => alert(x.message));
  };
  await listCl();
  if (arg) { const r = rows.find(x => String(x.id) === arg); if (r) fill(r); }
}

/* ---------- Hours checker: does every view of one player's week agree? ---------- */
async function hoursCheck(arg) {
  if (!arg) {
    view.innerHTML = `<section class="panel"><h2>Hours checker</h2><div class="body"><p class="note">Pick a player to compare their weekly headline, daily bars and per-game totals side by side, find bad sessions, and close a stuck one. Every fix needs a reason and is logged.</p>
      <input class="in" type="search" id="hcq" placeholder="Search handle, name or email" autocomplete="off" style="max-width:420px"></div><div id="hclist"></div></section>`;
    const go = async q => { const d = await api("/admin/users?" + new URLSearchParams({ q })); $("#hclist").innerHTML = d.users.length ? `<table><thead><tr><th>Handle</th><th>Name</th><th>Last seen</th></tr></thead><tbody>${d.users.map(u => `<tr data-id="${esc(u.id)}" style="cursor:pointer"><td><a href="#hourscheck/${esc(u.id)}">${esc(u.handle)}</a> ${rolePill(u.role)}</td><td>${esc(u.display_name)}</td><td class="sm">${when(u.last_seen)}</td></tr>`).join("")}</tbody></table>${d.total > d.users.length ? `<div class="note">Showing ${d.users.length} of ${num(d.total)}. Search to narrow it down.</div>` : ""}` : `<div class="body empty">No accounts match.</div>`; };
    let t; $("#hcq").oninput = e => { clearTimeout(t); t = setTimeout(() => go(e.target.value.trim()), 250); };
    $("#hclist").onclick = e => { const tr = e.target.closest("tr[data-id]"); if (tr && !e.target.closest("a")) location.hash = "hourscheck/" + tr.dataset.id; };
    return go("");
  }
  const d = await api(`/admin/users/${encodeURIComponent(arg)}/hours_check`);
  await loadIcons(d.games.map(g => g.game_id));
  const h1 = s => (Math.round(s / 360) / 10).toLocaleString("en") + " h";
  const FLAG = { long: "over 18 h", future: "in the future", overlap: "overlaps another", duration: "length doesn't match start/end" };
  const max = Math.max(1, ...d.days.map(x => x.seconds));
  const o = d.open_session;
  view.innerHTML = `<p><a href="#hourscheck">← Hours checker</a> · <a href="#user/${esc(d.user.id)}">${esc(d.user.handle)}'s account</a></p>
    <div class="hc-head ${d.mismatches ? "bad" : "ok"}"><b>${d.mismatches ? `${d.mismatches} mismatch${d.mismatches === 1 ? "" : "es"}` : "Everything agrees"}</b><span>${esc(d.user.handle)} · this week ${day(d.from)} – ${day(d.to)} (${esc(d.zone)})${d.problems ? ` · ${d.problems} session${d.problems === 1 ? "" : "s"} flagged` : ""}</span></div>
    <div class="hc-3">
      <section class="panel"><h2>Weekly headline</h2><div class="body"><div class="hc-big">${th(d.headline.tenths)}</div><div class="dimt">What the profile shows · ${num(d.headline.seconds)} s</div><div class="dimt" style="margin-top:6px">Rolling 7 days: ${hrs(d.rolling_7d_minutes)}</div></div></section>
      <section class="panel"><h2>Daily bars <span class="more">${th(d.days.reduce((a, x) => a + x.tenths, 0))}</span></h2><div class="body hc-bars">${d.days.map(x => `<div class="b"><i style="height:${Math.round(x.seconds / max * 100)}%"></i><span>${(x.tenths / 10).toFixed(1)}</span><em>${esc(x.label)}</em></div>`).join("")}</div></section>
      <section class="panel"><h2>Per game <span class="more">${th(d.games.reduce((a, x) => a + x.tenths, 0))}</span></h2><div class="body">${d.games.length ? `<table>${d.games.map(g => `<tr><td><span class="gm">${icon(g.game_id, g.name)} ${esc(g.name)}</span></td><td class="nr">${th(g.tenths)}</td></tr>`).join("")}</table>` : `<div class="empty">No games this week.</div>`}</div></section></div>
    <section class="panel"><h2>Checks</h2><table><thead><tr><th>Compare</th><th class="nr">Headline</th><th class="nr">Other view</th><th>Result</th></tr></thead><tbody>${d.checks.map(c => `<tr><td>${esc(c.label)}</td><td class="nr">${c.unit === "tenths" ? th(c.a) : num(c.a) + " s"}</td><td class="nr">${c.unit === "tenths" ? th(c.b) : num(c.b) + " s"}</td><td>${c.ok ? `<span class="st ok">Match</span>` : `<span class="st ret">Off by ${c.unit === "tenths" ? th(Math.abs(c.a - c.b)) : num(Math.abs(c.a - c.b)) + " s"}</span>`}</td></tr>`).join("")}</tbody></table></section>
    <section class="panel"><h2>Open session</h2><div class="body">${o ? `<dl class="kv"><dt>Game</dt><dd>${esc(o.name)}${o.hidden ? " (hidden)" : ""}</dd><dt>Started</dt><dd>${when(o.started_at)}</dd><dt>Last heartbeat</dt><dd>${o.heartbeat_at ? when(o.heartbeat_at) : "none"}</dd><dt>Length so far</dt><dd>${h1(o.seconds)}${o.counting ? " · counting in the week" : ""}</dd></dl>
      ${o.stuck ? `<div class="alert err">Stuck: no heartbeat for a while. Closing keeps play up to the last heartbeat (${when(o.would_end_at)}).</div>` : ""}
      ${d.may_edit ? `<div class="row"><button class="btn${o.stuck ? " dl" : ""}" data-close="keep">Close and keep time to last heartbeat</button><button class="btn danger" data-close="drop">Close and drop it</button></div>` : ""}` : `<div class="empty" style="padding:0">No open session.</div>`}<div class="alert" id="hcmsg" hidden></div></div></section>
    <section class="panel"><h2>Sessions this week <span class="more">${d.sessions.length}</span></h2>${d.sessions.length ? `<table><thead><tr><th>Game</th><th>Start</th><th>End</th><th class="nr">Length</th><th>Source</th><th>Flags</th><th></th></tr></thead><tbody>${d.sessions.map(s => `<tr data-sid="${s.id}"${s.flags.length ? ` class="hc-flag"` : ""}><td>${esc(s.name)}</td><td class="sm">${when(s.started_at)}</td><td class="sm">${when(s.ended_at)}</td><td class="nr">${h1(s.duration_s)}</td><td class="sm">${esc(s.source)}</td><td>${s.flags.map(f => `<span class="pill bad">${esc(FLAG[f] || f)}</span>`).join(" ")}</td><td class="rt">${d.may_edit ? `<button class="btn sm danger" data-rm>Remove</button>` : ""}</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No sessions this week.</div>`}</section>`;
  const uid = `/admin/users/${encodeURIComponent(d.user.id)}`;
  view.onclick = async e => {
    const c = e.target.closest("[data-close]"), r = e.target.closest("[data-rm]");
    if (!c && !r) return;
    const reason = prompt(c ? "Why close this session? (logged)" : "Why remove this session? (logged)");
    if (reason == null) return;
    if (reason.trim().length < 3) { alert("Add a short reason."); return; }
    await act(c || r, async () => {
      if (c) await api(uid + "/open_session/close", { method: "POST", body: { reason: reason.trim(), keep: c.dataset.close === "keep" } });
      else await api(`${uid}/sessions/${r.closest("tr").dataset.sid}/remove`, { method: "POST", body: { reason: reason.trim() } });
      hoursCheck(arg);
    }).catch(x => msg($("#hcmsg"), x.message));
  };
}

/* ---------- Bulk fixes: the same audited actions, many games at a time ---------- */
async function runAll(items, out, fn) {
  let ok = 0, bad = []; out.hidden = false; out.className = "alert";
  for (let i = 0; i < items.length; i++) {
    out.textContent = `Working… ${i + 1} of ${items.length}`;
    try { await fn(items[i]); ok++; } catch (x) { bad.push(`${items[i].label || items[i]}: ${x.message}`); }
  }
  out.className = "alert " + (bad.length ? "err" : "ok");
  out.textContent = `${ok} done${bad.length ? `, ${bad.length} failed:\n` + bad.slice(0, 20).join("\n") + (bad.length > 20 ? `\n…and ${bad.length - 20} more` : "") : "."}`;
  out.style.whiteSpace = "pre-line";
}
async function bulkFixes() {
  const sug = can.approvals ? await api("/admin/suggestions").catch(() => ({ suggestions: [] })) : { suggestions: [] };
  const s = sug.suggestions || [];
  view.innerHTML = `<section class="panel"><h2>Approve submitted games <span class="more">${s.length} waiting</span></h2>${s.length ? `<div class="body"><div class="row"><label class="check" style="margin:0"><input type="checkbox" id="sgall"> Select all</label><button class="btn dl" id="sgok" disabled>Approve selected</button><a class="btn sm" href="#approvals">One at a time in Approvals</a></div></div>
      <table class="gl"><thead><tr><th class="ck"></th><th>Exe</th><th>Name to approve</th><th class="nr">Reports</th><th class="nr">Players</th><th>First seen</th></tr></thead><tbody>${s.map((x, i) => `<tr data-i="${i}"><td class="ck"><input type="checkbox" data-sg></td><td class="mono">${esc(x.exe)}</td><td><input class="in" type="text" data-nm value="${esc(x.names[0] ? x.names[0].name : "")}" maxlength="120" style="width:100%"></td><td class="nr">${num(x.reports)}</td><td class="nr">${num(x.users)}</td><td class="sm">${when(x.first_seen)}</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">Nothing waiting.</div>`}<div class="body"><div class="alert" id="sgmsg" hidden></div></div></section>
    <section class="panel"><h2>Set status for many games</h2><form method="post" action="/api/v1/form-fallback" class="f body" id="bst">
      <label>Game ids, one per line<textarea name="ids" rows="6" class="mono" placeholder="Tick games in All games, or paste ids here">${esc([...SEL].join("\n"))}</textarea></label>
      <div class="row"><select class="in" name="lifecycle">${Object.entries(LIFE).map(([k, v]) => `<option value="${k}">${v}</option>`).join("")}</select><label style="margin:0">Expected end / launch <input type="date" name="date"></label><label style="margin:0;flex:1">Note<input class="in" type="text" name="note" maxlength="500" style="width:100%"></label></div>
      <label class="check"><input type="checkbox" name="checked" checked> Mark as checked now</label>
      <div class="row"><button class="btn dl">Preview changes</button><span class="dimt" id="bstn"></span><a class="btn sm" href="#games">Pick in Games</a></div><div id="bstprev"></div><div class="alert" id="bstmsg" hidden></div></form></section>
    ${can.games ? `<section class="panel"><h2>Upload icons or covers</h2><div class="body f"><p class="note">Name each file after its game id, e.g. <span class="mono">counter-strike-2.png</span>. Use <span class="mono">__</span> for a <span class="mono">:</span> in the id. PNG, JPG or WebP up to 1 MB each.</p>
      <div class="row"><select class="in" id="upk"><option value="icon">Icons</option><option value="cover">Covers</option></select><input type="file" id="upf" multiple accept="image/png,image/jpeg,image/webp"><button class="btn dl" id="upgo" disabled>Upload all</button></div>
      <div id="uplist"></div><div class="alert" id="upmsg" hidden></div></div></section>` : ""}`;
  const ids = () => [...new Set($("#bst").ids.value.split(/[\s,]+/).map(x => x.trim()).filter(Boolean))];
  const cnt = () => { $("#bstn").textContent = `${ids().length} game${ids().length === 1 ? "" : "s"}`; }; cnt();
  $("#bst").ids.addEventListener("input", () => { cnt(); const p = $("#bstprev"); if (p) p.innerHTML = ""; });
  $("#bst").addEventListener("submit", async e => {
    e.preventDefault(); const f = e.target, list = ids();
    if (!list.length) return;
    if (list.length > 100) { msg($("#bstmsg"), "Preview at most 100 games at a time."); return; }
    const body = { ids: list, lifecycle: f.lifecycle.value, date: f.date.value, note: f.note.value, checked: f.checked.checked };
    let p;
    try { p = await api("/admin/bulk/preview", { method: "POST", body }); }
    catch (x) { msg($("#bstmsg"), x.message); return; }
    const life = LIFE[p.lifecycle] || p.lifecycle;
    const before = r => r.before ? (LIFE[r.before.lifecycle] || r.before.lifecycle) : "Live (not saved yet)";
    const afterBits = [life, p.date ? `date ${p.date}` : "date cleared", p.note ? `note “${p.note}”` : "note cleared", p.checked ? "mark checked" : "don't stamp checked"].join(" · ");
    $("#bstprev").innerHTML = `<table class="gl"><thead><tr><th>Game</th><th>Now</th><th>Will write</th></tr></thead><tbody>${p.rows.map(r => `<tr><td>${r.found ? esc(r.name) : `<span class="pill bad">Not found</span>`} <span class="mono dimt">${esc(r.id)}</span></td><td>${r.found ? esc(before(r)) + (r.before && r.before.note ? ` · ${esc(r.before.note)}` : "") : "-"}</td><td>${r.found ? esc(afterBits) : "skipped"}</td></tr>`).join("")}</tbody></table>
      <p class="note">${p.missing ? `${num(p.missing)} id${p.missing === 1 ? "" : "s"} won't be changed because Fraglink doesn't know them. ` : ""}Nothing is saved until you apply. Each change is written to the audit log.</p>
      <label>Type <b>APPLY</b> to set ${esc(life)} on the ${num(p.rows.length - p.missing)} found game${p.rows.length - p.missing === 1 ? "" : "s"}<input type="text" id="bstok" autocomplete="off" spellcheck="false"></label>
      <div class="row"><button type="button" class="btn dl" id="bstgo" disabled>Apply these changes</button></div>`;
    const ok = $("#bstok"), go = $("#bstgo");
    ok.addEventListener("input", () => { go.disabled = ok.value.trim() !== "APPLY"; });
    go.addEventListener("click", async () => {
      if (ok.value.trim() !== "APPLY") return;
      const found = p.rows.filter(r => r.found);
      await runAll(found.map(r => ({ id: r.id, label: r.name })), $("#bstmsg"), x => api("/admin/games/" + encodeURIComponent(x.id) + "/status", { method: "POST", body: { lifecycle: p.lifecycle, date: p.date || "", note: p.note || "", checked: p.checked } }));
    });
  });
  if (s.length) {
    const boxes = () => $$("input[data-sg]"), upd = () => { const n = boxes().filter(b => b.checked).length; $("#sgok").disabled = !n; $("#sgok").textContent = n ? `Approve ${n}` : "Approve selected"; };
    $("#sgall").onchange = e => { boxes().forEach(b => { b.checked = e.target.checked; }); upd(); };
    view.addEventListener("change", e => { if (e.target.matches("input[data-sg]")) upd(); });
    $("#sgok").onclick = async () => {
      const pick = boxes().filter(b => b.checked).map(b => { const tr = b.closest("tr"); return { exe: s[+tr.dataset.i].exe, name: $("input[data-nm]", tr).value.trim(), label: s[+tr.dataset.i].exe }; });
      if (pick.some(p => !p.name)) { alert("Every selected game needs a name."); return; }
      if (!confirm(`Approve ${pick.length} game${pick.length === 1 ? "" : "s"}?`)) return;
      await runAll(pick, $("#sgmsg"), p => api("/admin/suggestions/approve", { method: "POST", body: { exe: p.exe, name: p.name } }));
      badge("approvals", 0);
    };
  }
  const upf = $("#upf");
  if (upf) {
    let files = [];
    upf.onchange = () => {
      files = [...upf.files].map(file => ({ file, id: file.name.replace(/\.[^.]+$/, "").replace(/__/g, ":"), label: file.name }));
      $("#uplist").innerHTML = files.length ? `<table><thead><tr><th>File</th><th>Game id</th><th class="nr">Size</th><th></th></tr></thead><tbody>${files.map(x => `<tr><td>${esc(x.file.name)}</td><td class="mono">${esc(x.id)}</td><td class="nr">${(x.file.size / 1024).toFixed(0)} KB</td><td>${x.file.size > 1048576 ? `<span class="pill bad">over 1 MB</span>` : ""}</td></tr>`).join("")}</tbody></table>` : "";
      $("#upgo").disabled = !files.length;
    };
    $("#upgo").onclick = async () => {
      const k = $("#upk").value;
      if (!confirm(`Upload ${files.length} ${k === "icon" ? "icon" : "cover"}${files.length === 1 ? "" : "s"}? Each replaces the current one.`)) return;
      await runAll(files, $("#upmsg"), async x => {
        if (x.file.size > 1048576) throw new Error("over 1 MB");
        const r = await fetch(window.FL.API + "/admin/games/" + encodeURIComponent(x.id) + (k === "icon" ? "/icon" : "/art"), { method: "POST", headers: { Authorization: "Bearer " + token(), "Content-Type": x.file.type || "application/octet-stream" }, body: x.file });
        if (!r.ok) { let m = r.status + ""; try { m = (await r.json()).error || m; } catch (_) { /* not json */ } throw new Error(m); }
      });
    };
  }
}

/* ---------- Health: is the server, its jobs, backups and email all right? ---------- */
async function health() {
  const d = await api("/admin/health");
  const ago = iso => { if (!iso) return "never"; const m = (Date.now() - new Date(iso)) / 60000; return m < 90 ? `${Math.round(m)} min ago` : m < 2880 ? `${Math.round(m / 60)} h ago` : `${Math.round(m / 1440)} days ago`; };
  const b = d.backups || {}, nb = b.newest, e = d.email || {}, jobs = d.jobs || [];
  const bOld = nb ? (Date.now() - new Date(nb.modified)) / 3600e3 > 30 : false;
  const jbad = jobs.filter(j => j.failing || j.stale).length;
  const listJob = jobs.some(j => j.job === "games_list");
  const tile = (k, v, sub, cls) => `<div class="hl ${cls || ""}"><div class="k">${k}</div><div class="v">${v}</div><div class="d">${sub}</div></div>`;
  const jobLabel = jobs.length ? (jbad ? `${jbad} need a look` : "Recorded runs OK") : "None recorded";
  const jobSub = jobs.length ? `${num(jobs.length)} stored${listJob ? "" : " · games list check not recorded"}` : "No job runs stored. That is not a pass.";
  const emailLabel = e.live ? "Sending" : e.enabled ? "Configured, not sending" : "Off";
  view.innerHTML = `<div class="hl-row">
      ${tile("Server", esc(d.version), d.started_at ? `up since ${when(d.started_at)}` : "start time not recorded", d.started_at ? "ok" : "")}
      ${tile("Files on disk", d.deploy.binary_at ? ago(d.deploy.binary_at) : "Unknown", `server binary ${d.deploy.binary_at ? when(d.deploy.binary_at) : "unknown"} · site ${d.deploy.site_at ? when(d.deploy.site_at) : "unknown"}`)}
      ${tile("Last backup", !b.readable ? "Can't read" : nb ? ago(nb.modified) : "None", !b.readable ? esc(b.error || b.dir || "") : nb ? `${esc(nb.name)} · ${bytes(nb.bytes)} · ${num(b.count)} kept` : esc(b.dir || "no dumps"), !b.readable || bOld ? "bad" : nb ? "ok" : "")}
      ${tile("Jobs", jobLabel, jobSub, jobs.length ? (jbad ? "bad" : "ok") : "")}
      ${tile("Email", emailLabel, `${e.provider ? esc(e.provider) : "no provider"} · ${num(e.alerts_sent_24h)} staff alerts sent 24 h${e.alerts_failed_7d ? ` · ${num(e.alerts_failed_7d)} failed 7 d` : ""}`, e.alerts_failed_7d ? "bad" : e.live ? "ok" : "")}</div>
    <div class="split" style="margin-top:12px"><section class="panel"><h2>Background jobs</h2>${jobs.length ? `<table><thead><tr><th>Job</th><th>Last run</th><th>Last OK</th><th>Detail</th><th>State</th></tr></thead><tbody>${jobs.map(j => `<tr><td class="mono">${esc(j.job)}</td><td class="sm">${when(j.last_run_at)}</td><td class="sm">${when(j.last_ok_at)}</td><td class="sm"><div class="exes" title="${esc(j.detail || "")}">${esc(j.detail || "-")}</div></td><td>${j.failing ? `<span class="st ret">Failing</span>` : j.stale ? `<span class="st beta">Late</span>` : j.scheduled ? `<span class="st ok">OK</span>` : `<span class="st hid">Recorded</span>`}</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No job runs recorded yet. An empty list is not a healthy result.</div>`}</section>
    <section class="panel"><h2>Server details</h2><div class="body"><dl class="kv"><dt>Version</dt><dd class="mono">${esc(d.version)}</dd><dt>Started</dt><dd>${d.started_at ? when(d.started_at) : "not recorded"}</dd><dt>Database</dt><dd>migration ${d.migration ? `${esc(String(d.migration.version))} · ${esc(d.migration.description)} (${when(d.migration.installed_on)})` : "-"}</dd>
      <dt>Backups</dt><dd>${esc(b.dir || "-")}${b.readable === false ? " · not readable" : ""}</dd><dt>Email</dt><dd>${e.live ? "live" : "not live"} · last alert sent ${when(e.last_ok_at)}${e.last_failure ? `<div class="dimt">Last failure: ${esc(e.last_failure.error || "")} ${when(e.last_failure.at)}</div>` : ""}</dd></dl>
      <p class="dimt">Email counts cover staff alerts only; other mail isn't logged. Releases and deploys aren't run from Admin.</p></div></section></div>
    <section class="panel" style="margin-top:12px"><h2>Recent warnings and errors (48 h)</h2>${d.errors == null ? `<div class="body empty">The journal isn't readable by the server.</div>` : d.errors.length ? `<pre class="log">${esc(d.errors.map(l => l.replace(/ \S+ fraglink-server\[\d+\]: \S+Z /, "  ")).join("\n"))}</pre>` : `<div class="body empty">None.</div>`}</section>`;
}

/* ---------- Suggested Merge ---------- */
async function suggestedMerge() {
  const d = await api("/admin/suggested-merge");
  const cov = (d.coverage && d.coverage.sources) || [];
  const nm = (name, id) => name && name !== id ? name : id;
  view.innerHTML = `<section class="panel"><h2>Suggested Merge</h2>
    <p class="note">You do not need to merge a Steam id onto the games list. The list already is the catalogue. Link records a fix-up for future detection and leaves every saved session and hour where it is. Merge is a separate step, only when both entries already have saved rows, and it can change the shown total. Shared Steam id alone is not a merge. Editions and folder splits are not offered.</p>
    <div class="ag-fresh">${cov.map(s => `<div><div class="k">${esc(s.label)}</div><div class="v">${s.available ? "Available" : "Not available"}</div><div class="d">${esc(s.detail)}</div></div>`).join("")}</div></section>
    <section class="panel" style="margin-top:12px"><h2>Suggestions <span class="dimt">${num(d.open)} open · ${num(d.dismissed)} dismissed</span></h2>
    ${d.rows.length ? `<table class="gl sm-suggest"><thead><tr><th>Would move</th><th>Stays</th><th>Evidence</th><th></th></tr></thead><tbody>
      ${d.rows.map(r => `<tr data-key="${esc(r.key)}" data-src="${esc(r.source)}" data-into="${esc(r.target)}" data-op="${esc(r.op)}">
        <td><div class="nm">${esc(nm(r.source_name, r.source))}</div><div class="id">${esc(r.source)}</div></td>
        <td><div class="nm">${esc(nm(r.target_name, r.target))} ${r.op === "merge" ? `<span class="pill ok">stays</span>` : ""}</div><div class="id">${esc(r.target)}</div></td>
        <td><div>${esc(r.launcher)} ${esc(r.product)}</div><div class="sm">${num(r.source_records)} saved on the id · ${num(r.target_records)} on the game that stays</div></td>
        <td><div class="sm-acts">${r.op === "merge" ? `<button class="btn sm" type="button" data-prev>Preview merge</button>` : r.op === "map" ? `<button class="btn sm" type="button" data-prev>Preview link</button>` : `<span class="dimt">Not a merge</span>`}${r.dismissed ? "" : ` <button class="btn sm" type="button" data-dismiss>Dismiss</button>`}</div></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">Nothing needs a merge. A Steam id the catalogue already names is not listed unless both sides have saved rows.</div>`}
    </section>`;
  view.onclick = async e => {
    const b = e.target.closest("button"); if (!b) return;
    const tr = b.closest("tr[data-src]"); if (!tr) return;
    if (b.matches("[data-dismiss]")) {
      await act(b, async () => { await api("/admin/suggested-merge/dismiss", { method: "POST", body: { key: tr.dataset.key } }); suggestedMerge(); });
      return;
    }
    if (!b.matches("[data-prev]")) return;
    let slot = tr.nextElementSibling;
    if (!slot || !slot.classList.contains("sm-slot")) {
      slot = document.createElement("tr");
      slot.className = "sm-slot";
      slot.innerHTML = `<td colspan="4"></td>`;
      tr.after(slot);
    }
    const cell = $("td", slot);
    cell.innerHTML = `<div class="sm-prev" role="status">Loading preview…</div>`;
    b.setAttribute("aria-busy", "true");
    b.disabled = true;
    try {
      const p = await api(`/admin/games/${encodeURIComponent(tr.dataset.src)}/merge?into=${encodeURIComponent(tr.dataset.into)}`);
      cell.innerHTML = previewHtml(p, { source: tr.dataset.src, target: tr.dataset.into, link: true });
      const form = $("[data-confirm]", cell);
      if (form) {
        const input = $("input", form), go = $("button", form);
        input.addEventListener("input", () => { go.disabled = input.value.trim() !== tr.dataset.src; });
        form.addEventListener("submit", ev => {
          ev.preventDefault();
          if (input.value.trim() !== tr.dataset.src) return;
          act(go, async () => {
            await api(`/admin/games/${encodeURIComponent(tr.dataset.src)}/merge`, { method: "POST", body: { into: tr.dataset.into, confirm: input.value.trim() } });
            suggestedMerge();
          });
        });
      }
      const link = $("[data-linkdo]", cell);
      if (link) link.onclick = () => act(link, async () => {
        await api("/admin/suggested-merge/link", { method: "POST", body: { source: tr.dataset.src, into: tr.dataset.into } });
        cell.innerHTML = `<div class="sm-prev" role="status">Linked. No sessions, imported hours, estimated hours, or Steam snapshots were moved.</div>`;
      });
    } catch (x) {
      cell.innerHTML = `<div class="sm-prev alert err" role="alert">${esc(x.message || "Preview failed.")} Nothing was changed.</div>`;
    } finally {
      b.disabled = false;
      b.removeAttribute("aria-busy");
    }
  };
}

/* ---------- Detection tester ---------- */
async function detectTest() {
  view.innerHTML = `<section class="panel"><h2>Detection tester</h2>
    <form method="post" action="/api/v1/form-fallback" class="f body" id="dtf">
      <div class="row"><label style="margin:0">Exe<input class="in" name="exe" placeholder="Wow.exe" required></label>
        <label style="margin:0;flex:1">Folder<input class="in" name="folder" placeholder="D:\\World of Warcraft\\_retail_" style="width:100%"></label>
        <button class="btn dl">Test</button></div>
      <p class="note">Read only. Uses the folder rules, signatures, games list and fix-ups the app uses. It cannot see that PC's Steam library or other launcher folders.</p>
    </form><div id="dtout"></div></section>`;
  $("#dtf").addEventListener("submit", async e => {
    e.preventDefault();
    const f = e.target;
    try {
      const d = await api("/admin/detect-test", { method: "POST", body: { exe: f.exe.value.trim(), folder: f.folder.value.trim() } });
      const m = d.matched;
      $("#dtout").innerHTML = `<section class="panel"><h2>${m ? esc(m.name) : "No game"}</h2><div class="body">
        ${m ? `<p><span class="st ok">${esc(m.source)}</span> <span class="mono">${esc(m.game_id)}</span></p>` : `<p>No rule matched this exe and folder.</p>`}
        ${d.ambiguous ? `<div class="alert err">${esc(d.ambiguous)}</div>` : ""}
        <table><tbody>${d.steps.map(s => `<tr><td>${esc(s.layer)}</td><td>${s.hit ? "matched" : "no"}</td><td class="mono">${typeof s.detail === "string" ? esc(s.detail) : esc(JSON.stringify(s.detail))}</td></tr>`).join("")}</tbody></table>
        <h3>Not reproduced on the server</h3>
        <ul>${d.not_reproduced.map(n => `<li><b>${esc(n.label)}</b> — ${esc(n.detail)}</li>`).join("")}</ul>
      </div></section>`;
    } catch (x) { $("#dtout").innerHTML = `<div class="alert err">${esc(x.message)}</div>`; }
  });
}

/* ---------- Missing artwork ---------- */
async function missingArt() {
  const d = await api("/admin/artwork");
  view.innerHTML = `<section class="panel"><h2>Missing artwork <span class="dimt">${num(d.total)}</span></h2>
    <p class="note">${esc(d.note)} Replace one icon from the game editor. There is no bulk replace.</p>
    ${d.rows.length ? `<table class="gl"><thead><tr><th>Game</th><th>Status</th><th></th></tr></thead><tbody>
      ${d.rows.map(r => `<tr><td>${esc(r.name)} <span class="mono dimt">${esc(r.id)}</span></td><td>${esc(r.status)}<div class="dimt">${esc(r.detail)}</div></td><td class="rt"><a class="btn sm" href="#games/id:${encodeURIComponent(r.id)}">Edit artwork</a></td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No missing icons in this page.</div>`}
    </section>`;
}

/* ---------- Data checks ---------- */
async function dataChecks() {
  const d = await api("/admin/data-checks");
  const dup = d.duplicate_launchers || [], rules = d.conflicting_rules || [], hours = d.hours || {};
  view.innerHTML = `<section class="panel"><h2>Data checks</h2><p class="note">${esc(d.note)}</p></section>
    <section class="panel"><h2>Duplicate launcher ids <span class="dimt">${dup.length}</span></h2>
      ${dup.length ? `<table><thead><tr><th>Launcher</th><th>Id</th><th>Games</th></tr></thead><tbody>${dup.map(r => `<tr><td>${esc(r.launcher)}</td><td class="mono">${esc(String(r.product))}</td><td class="mono">${esc((r.ids || []).join(", "))}</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No duplicate Steam app ids in game rows or fix-ups.</div>`}</section>
    <section class="panel"><h2>Conflicting detection rules <span class="dimt">${rules.length}</span></h2>
      ${rules.length ? `<table><tbody>${rules.map(r => `<tr><td class="mono">${esc(r.kind)} ${esc(r.pattern)}</td><td>${esc((r.ids || []).join(", "))}</td><td>${esc(r.detail)}</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No pattern points at two games.</div>`}</section>
    <section class="panel"><h2>Hours</h2>
      <p class="note">${esc(hours.week_note || "")}</p>
      ${(hours.length_mismatches || []).length ? `<table><tbody>${hours.length_mismatches.map(r => `<tr><td class="mono">${esc(r.session_id)}</td><td>${esc(r.detail)}</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">No session whose stored length disagrees with its start and end.</div>`}
      ${(hours.week_mismatches || []).length ? `<table><tbody>${hours.week_mismatches.map(r => `<tr><td class="mono">${esc(r.user_id)}</td><td>${esc(r.detail)} (${r.week_seconds} vs ${r.sql_seconds})</td></tr>`).join("")}</tbody></table>` : `<div class="body empty">The checked players' local weeks match the shared total.</div>`}</section>`;
}

/* ---------- Review history ---------- */
async function reviewHistory() {
  const d = await api("/admin/review-history");
  view.innerHTML = `<section class="panel"><h2>Review history</h2>
    <p class="note">${esc(d.note)}</p>
    <div class="tools"><a class="btn sm" href="#" data-g="">All</a><a class="btn sm" href="#" data-g="reviewed">Reviewed</a><a class="btn sm" href="#" data-g="dismissed">Dismissed</a><a class="btn sm" href="#" data-g="merged">Merged</a><a class="btn sm" href="#" data-g="corrected">Corrected</a></div>
    <div id="hist">${histTable(d)}</div></section>`;
  view.onclick = async e => {
    const a = e.target.closest("[data-g]"); if (!a) return;
    e.preventDefault();
    const d2 = await api("/admin/review-history?" + new URLSearchParams({ group: a.dataset.g }));
    $("#hist").innerHTML = histTable(d2);
  };
}
function histTable(d) {
  if (!d.rows.length) return `<div class="body empty">Nothing in the audit log for this filter.</div>`;
  return `<table class="gl"><thead><tr><th>When</th><th>Who</th><th>Action</th><th>Target</th></tr></thead><tbody>
    ${d.rows.map(r => `<tr><td class="sm">${when(r.at)}</td><td>${r.by ? esc(r.by) : `<span class="dimt">not recorded</span>`}</td><td class="mono">${esc(r.action)}</td><td class="mono">${esc(r.target || "")}</td></tr>`).join("")}</tbody></table>`;
}

/* ---------- Undo ---------- */
async function undoRecent() {
  const d = await api("/admin/undo");
  view.innerHTML = `<section class="panel"><h2>Undo</h2>
    <p class="note">Only edits whose before and after were stored, and which still match that after, can be put back. Merges, deleted sessions and deleted accounts are not undone here.</p>
    <table class="gl"><thead><tr><th>When</th><th>Who</th><th>Action</th><th>Can undo</th><th></th></tr></thead><tbody>
    ${d.rows.map(r => `<tr data-id="${r.id}"><td class="sm">${when(r.at)}</td><td>${esc(r.by || "")}</td><td class="mono">${esc(r.action)}</td><td>${r.undo ? "Yes" : "No"}<div class="dimt">${esc(r.why)}</div></td>
      <td>${r.undo ? `<button class="btn sm" data-undo>Undo…</button>` : ""}</td></tr>`).join("")}</tbody></table></section>`;
  view.onclick = e => {
    const b = e.target.closest("[data-undo]"); if (!b) return;
    const id = +b.closest("tr").dataset.id;
    const typed = prompt("Type UNDO to put this edit back. If the row has changed since, nothing happens.");
    if (typed == null) return;
    act(b, async () => { await api("/admin/undo", { method: "POST", body: { id, confirm: typed.trim() } }); undoRecent(); });
  };
}

route();
if (can.approvals && !location.hash.startsWith("#approvals")) Promise.all([api("/admin/suggestions"), api("/admin/unknown_games").catch(() => ({ unknown: [] }))]).then(([d, u]) => badge("approvals", d.suggestions.length + u.unknown.length)).catch(() => {});
api("/admin/suggested-merge").then(d => badge("suggested", d.open)).catch(() => {});
};

