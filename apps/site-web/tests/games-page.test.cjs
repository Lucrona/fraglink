const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const layout = fs.readFileSync(path.join(root, 'layout.html'), 'utf8');
const page = fs.readFileSync(path.join(root, 'pages/games.html'), 'utf8');
const app = fs.readFileSync(path.join(root, 'static/assets/app.js'), 'utf8');
const admin = fs.readFileSync(path.join(root, 'static/assets/admin.js'), 'utf8');

test('main nav and footer include a Games tab next to FAQ', () => {
  assert.match(layout, /<nav class="nav">[^<]*<a href="\/">Home<\/a><a href="\/download">Download<\/a><a href="\/faq">FAQ<\/a><a href="\/games">Games<\/a><\/nav>/);
  assert.match(layout, /<a href="\/faq">FAQ<\/a><a href="\/games">Games<\/a><a href="\/changelog">Changelog<\/a>/);
  assert.match(app, /\["games", "\/games", "Games"\]/);
});

test('games page is an A–Z directory with slant-tab headers and no hours or user data', () => {
  assert.match(page, /page: games/);
  assert.match(page, /<span class="tab">Recent updates<\/span>/);
  assert.match(page, /<span class="tab">Games<\/span>/);
  assert.match(page, /az-jump/);
  assert.match(page, /<span class="tab">Missing a game\?<\/span>/);
  assert.match(page, /sign in to request/i);
  assert.match(page, /id="games-req"/);
  assert.match(page, /cf-turnstile/);
  assert.doesNotMatch(page, /hours|players|username|handle/i);
  assert.match(app, /pages\.games = async/);
  assert.match(app, /\/games\/catalogue/);
  assert.match(app, /api\("\/games\/updates/);
  assert.match(app, /IntersectionObserver/);
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
