const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const layout = fs.readFileSync(path.join(root, 'layout.html'), 'utf8');
const page = fs.readFileSync(path.join(root, 'pages/games.html'), 'utf8');
const app = fs.readFileSync(path.join(root, 'static/assets/app.js'), 'utf8');
const admin = fs.readFileSync(path.join(root, 'static/assets/admin.js'), 'utf8');
const css = fs.readFileSync(path.join(root, 'static/assets/site.css'), 'utf8');

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

test('games page is search-only, with live suggestions and a shareable query', () => {
  assert.match(page, /page: games/);
  assert.match(page, /<span class="tab">Recent updates<\/span>/);
  assert.match(page, /<span class="tab">Games<\/span>/);
  assert.match(page, /id="games-q"/);
  assert.match(page, /id="games-suggest"/);
  assert.match(page, /data-try/);
  assert.doesNotMatch(page, /az-jump|data-pager|Letters/);
  assert.match(page, /<span class="tab">Missing a game\?<\/span>/);
  assert.match(page, /sign in to request/i);
  assert.match(page, /id="games-req"/);
  assert.match(page, /cf-turnstile/);
  assert.doesNotMatch(page, /hours|players|username|handle/i);
  assert.match(app, /pages\.games = async/);
  assert.match(app, /\/games\/catalogue/);
  assert.match(app, /api\("\/games\/updates/);
  assert.match(app, /const LIMIT = 50/);
  assert.match(app, /const SUGGEST = 8/);
  assert.match(app, /const MIN = 2/);
  assert.match(app, /ALIASES/);
  assert.match(app, /cs2: "counter strike 2"/);
  assert.match(app, /cod: "call of duty"/);
  assert.match(app, /showSuggest/);
  assert.match(app, /ArrowDown/);
  assert.match(app, /Refine your search/);
  assert.match(app, /\/games\?q=/);
  assert.doesNotMatch(app, /\?l=|az-jump|IntersectionObserver|const PAGE = 200/);
  assert.match(app, /nameNorm/);
  assert.match(app, /\/games\/requests/);
  assert.match(app, /request it below/);
  assert.match(app, /g-thumb/);
  assert.match(app, /g-letter/);
  assert.match(app, /paintThumbs/);
  assert.match(app, /\/api\/v1\/art\//);
  assert.match(app, /\/icons\/games\/64\//);
  assert.match(app, /loading = box.closest\("#games-suggest"\) \? "eager" : "lazy"/);
  assert.match(css, /width:92px/);
  assert.match(css, /height:43px/);
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
