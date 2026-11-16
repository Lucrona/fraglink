'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', 'static', 'assets');
const css = fs.readFileSync(path.join(root, 'site.css'), 'utf8');
const app = fs.readFileSync(path.join(root, 'app.js'), 'utf8');

test('art links do not stretch to 100% width as a global flex child', () => {
  // The PR 48 rule `a.glink-art{width:100%;height:100%}` made the art link
  // eat the home "Your recent games" row, squeezing the name and hours right.
  assert.match(css, /^a\.glink-art\{width:auto;height:auto;max-width:100%\}/m);
  assert.doesNotMatch(css, /^a\.glink-art\{width:100%/m);
  assert.match(css, /\.home42 \.rg a\.glink-art\{flex:none/);
  // Sized art frames can still fill their box.
  assert.match(css, /\.gp-art a\.glink-art/);
});

test('recent games row keeps the name on one line with an ellipsis', () => {
  assert.match(css, /\.home42 \.rg \.w\{flex:1;min-width:0\}/);
  assert.match(css, /\.home42 \.rg b,.home42 \.rg b a\.glink\{[^}]*text-overflow:ellipsis[^}]*white-space:nowrap/);
  assert.match(css, /\.home42 \.rg \.h\{flex:none;/);
  assert.match(css, /\.home42 \.rg small\{[^}]*text-overflow:ellipsis[^}]*white-space:nowrap/);
  // Name tooltips come from gameLink's title, so a truncated title is still readable.
  const start = app.indexOf('function gameLink');
  const fn = app.slice(start, app.indexOf('function iconLink'));
  assert.match(fn, /title="\$\{esc\(name\)\}"/);
});

test('games list and suggestions also ellipsize long names', () => {
  assert.match(css, /\.g-name\{min-width:0;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap\}/);
  const start = app.indexOf('`<a class="g-row glink"');
  const row = app.slice(start, start + 400);
  assert.match(row, /title="\$\{esc\(g\[1\]\)\}"/);
});
