// No browser or network needed: exercise the actual shared render/group helpers.
const { readFileSync } = require('node:fs');
const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const source = readFileSync(__dirname + '/../static/assets/app.js', 'utf8');
const helpers = source.slice(source.indexOf('const factionCrest ='), source.indexOf('const WOW ='));
const { factionCrest, groupWowRealms } = vm.runInNewContext(helpers + '; ({ factionCrest, groupWowRealms })');

test('servers sorted by count then alphabetically, same-name regions distinguished', () => {
  const rows = [
    {realm:'Zuljin',region:'us',name:'a'}, {realm:'Zuljin',region:'us',name:'b'},
    {realm:'Area 52',region:'us',name:'c'}, {realm:'Area 52',region:'eu',name:'d'},
    {realm:'Frostmourne',region:'us',name:'e'},
  ];
  const before = JSON.stringify(rows);
  const groups = groupWowRealms(rows);
  assert.equal(JSON.stringify(groups.map(g => [g.realm, g.region, g.rows.length, g.showRegion])), JSON.stringify([
    ['Zuljin','US',2,false],['Area 52','EU',1,true],['Area 52','US',1,true],['Frostmourne','US',1,false],
  ]));
  assert.equal(JSON.stringify(rows),before);
  assert.equal(groupWowRealms([]).length,0);
});
test('case variants group into the same server', () => {
  const groups = groupWowRealms([{realm:'Area 52',region:'us'},{realm:'area 52',region:'US'}]);
  assert.equal(groups.length,1); assert.equal(groups[0].rows.length,2);
});
test('crests are accessible inline SVG; unknown factions render nothing', () => {
  for (const name of ['Horde','Alliance']) {
    assert.match(factionCrest(name), /<svg/);
    assert.ok(factionCrest(name).includes(`aria-label="${name}"`));
    assert.match(factionCrest(name), /<path/);
  }
  for (const value of [null,undefined,'','NEUTRAL','<script>']) assert.equal(factionCrest(value),'');
});
test('gallery omits realm on cards; profile retains it; upload follows active tab', () => {
  assert.ok(source.includes('g.rows.map(c => wowCard(c, false))'));
  assert.ok(source.includes('const wowCard = (c, showRealm = true)'));
  assert.ok(source.includes('const realm = showRealm && c.realm'));
  assert.ok(source.includes('up.textContent = "Upload " + (st.game === "wow_forever" ? "Forever" : "Retail") + " /played"'));
  assert.ok(source.includes('up.href = "/connect/wow?version=" + (st.game === "wow_forever" ? "forever" : "retail")'));
});
