const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const layout = fs.readFileSync(path.join(root, 'layout.html'), 'utf8');
const page = fs.readFileSync(path.join(root, 'pages/games.html'), 'utf8');
const app = fs.readFileSync(path.join(root, 'static/assets/app.js'), 'utf8');
const admin = fs.readFileSync(path.join(root, 'static/assets/admin.js'), 'utf8');

test('Games is in the left nav after Profile and Friends when signed in, and after FAQ when signed out', () => {
  assert.match(layout, /<nav class="nav">[^<]*<a href="\/">Home<\/a><a href="\/download">Download<\/a><a href="\/faq">FAQ<\/a><a href="\/games">Games<\/a><\/nav>/);
  assert.doesNotMatch(layout, /top-end|nav-end/);
  assert.match(layout, /<div class="right"><a class="btn" href="\/login">Sign in<\/a>/);
  assert.match(layout, /<a href="\/faq">FAQ<\/a><a href="\/games">Games<\/a><a href="\/changelog">Changelog<\/a>/);
  assert.doesNotMatch(app, /nav-end/);
  const start = app.indexOf('function renderHeader');
  const hdr = app.slice(start, app.indexOf('async function logout'));
  assert.match(hdr, /\["games", "\/games", "Games"\]/);
  const friendsThenGames = hdr.indexOf('["friends"') >= 0 && hdr.indexOf('["games", "/games", "Games"]') > hdr.indexOf('["friends"');
  assert.ok(friendsThenGames, 'signed-in Games should follow Profile and Friends');
  assert.match(hdr, /items\.push\(\["download", "\/download", "Download"\], \["faq", "\/faq", "FAQ"\], \["games", "\/games", "Games"\]\)/);
});

test('games page shows one letter at a time, with shareable tabs, paging and search', () => {
  assert.match(page, /page: games/);
  assert.match(page, /<span class="tab">Recent updates<\/span>/);
  assert.match(page, /<span class="tab">Games<\/span>/);
  assert.match(page, /az-jump/);
  assert.match(page, /aria-label="Letters"/);
  assert.match(page, /data-pager/);
  assert.match(page, /<span class="tab">Missing a game\?<\/span>/);
  assert.match(page, /sign in to request/i);
  assert.match(page, /id="games-req"/);
  assert.match(page, /cf-turnstile/);
  assert.doesNotMatch(page, /hours|players|username|handle/i);
  assert.match(app, /pages\.games = async/);
  assert.match(app, /\/games\/catalogue/);
  assert.match(app, /api\("\/games\/updates/);
  assert.match(app, /const PAGE = 200/);
  assert.match(app, /sp\.set\("l"/);
  assert.match(app, /sp\.set\("q"/);
  assert.doesNotMatch(app, /IntersectionObserver/);
  assert.match(app, /loading = "lazy"/);
  assert.match(app, /nameNorm/);
  assert.match(app, /\/games\/requests/);
});

test('admin games page has a change log and a requests queue', () => {
  assert.match(admin, /#games\/log/);
  assert.match(admin, /async function gamesLog/);
  assert.match(admin, /\/admin\/games-history/);
  assert.match(admin, /Hide from \/games/);
  assert.match(admin, /Add a public note/);
  assert.match(admin, /Open in Change log/);
  assert.match(admin, /#games\/requests/);
  assert.match(admin, /async function gamesRequests/);
  assert.match(admin, /\/admin\/game-requests/);
  assert.match(admin, /Approve and add/);
});
