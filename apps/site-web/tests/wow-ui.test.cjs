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
test('crests are accessible self-hosted artwork; unknown factions render nothing', () => {
  for (const name of ['Horde','Alliance']) {
    assert.match(factionCrest(name), /<img/);
    assert.ok(factionCrest(name).includes(`alt="${name}"`));
    assert.ok(factionCrest(name).includes(`/assets/wow-factions/${name.toLowerCase()}-64.webp`));
    assert.ok(factionCrest(name).includes(`/assets/wow-factions/${name.toLowerCase()}-128.webp 2x`));
    assert.ok(!factionCrest(name).includes("https://"));
    for (const size of [64,128]) {
      const asset = readFileSync(`${__dirname}/../static/assets/wow-factions/${name.toLowerCase()}-${size}.webp`);
      assert.equal(asset.toString('ascii',0,4),'RIFF');
      assert.equal(asset.toString('ascii',8,12),'WEBP');
      assert.ok(asset.length < 32768);
    }
  }
  for (const value of [null,undefined,'','NEUTRAL','<script>']) assert.equal(factionCrest(value),'');
});
test('gallery omits realm on cards; profile retains it; upload follows active tab', () => {
  assert.ok(source.includes('g.rows.map(c => wowCard(c, false))'));
  assert.ok(source.includes('const wowCard = (c, showRealm = true)'));
  assert.ok(source.includes('const realm = showRealm && wowRealm(c)'));
  assert.ok(source.includes('up.textContent = "Upload " + (st.game === "wow_forever" ? "Forever" : "Retail") + " /played"'));
  assert.ok(source.includes('up.href = "/connect/wow?version=" + (st.game === "wow_forever" ? "forever" : "retail")'));
});
test('manager and uploader share the filled, bordered version switch', () => {
  assert.ok(source.includes('class="seg wow-version-switch" data-tabs'));
  assert.ok(source.includes('class="seg wow-version-switch" role="tablist" aria-label="Which version to upload"'));
  const css = readFileSync(__dirname + '/../static/assets/site.css','utf8');
  assert.ok(css.includes('.seg.wow-version-switch button[aria-selected="true"]{background:#2c4d6b;color:#fff}'));
  assert.ok(css.includes('.seg.wow-version-switch button:hover'));
  assert.ok(css.includes('.wow-version-control{padding:10px 12px}'));
  assert.ok(!css.includes('.bn-tabs button.on'));
});

test('Forever characters group under the ruleset name, never Unknown server', () => {
  const groups = groupWowRealms([
    {game_id:'wow_forever',realm:'',ruleset:'PvP',region:'us',name:'Lucrona Ashborne'},
    {game_id:'wow_forever',realm:null,ruleset:'PvP',region:'us',name:'Lucroner Ashborne'},
    {game_id:'wow_forever',realm:'',ruleset:'Normal',region:'us',name:'Other'},
  ]);
  assert.equal(JSON.stringify(groups.map(g => [g.realm, g.region, g.rows.length, g.showRegion])), JSON.stringify([
    ['PvP','US',2,false],['Normal','US',1,false],
  ]));
  assert.ok(!JSON.stringify(groups).includes('Unknown'));
  const unsourced = groupWowRealms([{game_id:'wow_forever',realm:'',region:'us'}]);
  assert.equal(unsourced[0].realm,'Forever');
  const retail = groupWowRealms([{game_id:'wow',realm:'Area 52',region:'us'},{game_id:'wow_forever',ruleset:'PvP',region:'us'}]);
  assert.equal(retail[0].realm,'Area 52');
  assert.equal(retail[1].realm,'PvP');
});
