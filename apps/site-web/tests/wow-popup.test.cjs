const {readFileSync}=require('node:fs');
const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const source=readFileSync(__dirname+'/../static/assets/app.js','utf8');
const linkSource=source.slice(source.indexOf('const WOWLINK ='),source.indexOf('/* Pager:'));
const avStart=source.indexOf('const WOWAV =');
const avatarSource=source.slice(avStart,source.indexOf('\n})();',avStart)+6);
const start=source.indexOf('const WOWPOP =');
const popupSource=source.slice(start,source.indexOf('\n})();',start)+6);
const esc=x=>String(x??'').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');
function harness(data={name:'Sample',region:'us',level:18,gear:[]}, opts={}) {
 const events={}, calls=[], copied=[], nodes=[], timers=[];
 const location={pathname:'/u/sample',search:'',hash:''};
 const history={state:null,pushState(s,_,path){this.state=s;calls.push(['push',path]);location.pathname=path},replaceState(s,_,path){this.state=s;calls.push(['replace',path]);location.pathname=path},back(){calls.push(['back']);location.pathname='/u/sample'}};
 const inside={innerHTML:''};
 const ctx={console,URL,encodeURIComponent,Number,Date,Map,Set,location,history,navigator:{clipboard:{async writeText(s){copied.push(s)}}},
  WOW:{clsIcon:c=>'/class/'+c,raceIcon:()=>'/race/test'},siteBase:()=> 'https://playfraglink.com',esc,nf:new Intl.NumberFormat('en'),plural:(n,s)=>`${n} ${s}`,factionCrest:value=>{const f=String(value||'').toUpperCase();return (f==='HORDE'||f==='ALLIANCE')?`<img class="faction-crest" alt="${f==='HORDE'?'Horde':'Alliance'}" src="/assets/wow-factions/${f.toLowerCase()}-64.webp">`:'';},WH:{ok:!!opts.whOk,scan(){}},imgOk:'',
  addEventListener:(name,fn)=>events[name]=fn,document:{createElement(){const node={remove(){},innerHTML:'',className:''};nodes.push(node);return node},body:{appendChild(){},classList:{add(){},remove(){}}},addEventListener(){},removeEventListener(){}},
  $:(selector)=>selector==='.cp-x'?{focus(){}}:selector==='.cp-in'?inside:null,$$:()=>[],api:async()=>data,setTimeout:fn=>{timers.push(fn);return 0}};
 const {WOWPOP,WOWLINK}=vm.runInNewContext(linkSource+avatarSource+popupSource+';({WOWPOP,WOWLINK})',ctx);
 return {WOWPOP,WOWLINK,calls,copied,events,location,history,inside,nodes,timers};
}
test('stable share links use version and UUID, not names',()=>{
 const {WOWLINK}=harness();const id='01234567-89ab-cdef-0123-456789abcdef';
 for(const game of ['wow','wow_forever']){
  const path=WOWLINK.path('some user',{id,game_id:game,name:'Mutable Name'});
  const link=WOWLINK.parse(path);assert.equal(link.handle,'some user');assert.equal(link.id,id);assert.equal(link.game_id,game);
 }
 for(const path of ['/u/s/wow/forever/42','/u/s/wow/classic/'+id,'/u/s/wow/forever/../../private'])assert.equal(WOWLINK.parse(path),null);
});
test('empty popup sections stay absent; Forever gear uses Wowhead Forever, not Retail',()=>{
 const {WOWPOP}=harness();const c={id:'test',name:'Sample',game_id:'wow_forever'};
 let html=WOWPOP.body({name:'Sample',level:18,region:'us',game_id:'wow_forever'},c);
 for(const word of ['<h3>Gear','<h3>Stats','Raid progress','Mythic+','Collections','Professions','cp-empty','not available','No gear'])assert.ok(!html.includes(word),word);
 assert.ok(html.includes('>Share</button>'));assert.ok(!html.includes('View on Armory'));
 html=WOWPOP.body({name:'Sample',region:'us',game_id:'wow_forever',gear:[{slot:'RANGED',id:3111,name:'Bow',quality:'POOR',icon:'inv_weapon_bow_01',ilvl:5}],stats:[{name:'Health',value:508},{name:'Melee crit',value:4.8004,percent:true}]},c);
 assert.ok(html.includes('q-poor'));assert.ok(html.includes('Ranged'));assert.ok(html.includes('508'));assert.ok(html.includes('4.80%'));
 assert.ok(html.includes('data-whi="3111"'));assert.ok(html.includes('data-whenv="16"'));assert.ok(!html.includes('data-whenv="1"'));
 assert.ok(!html.includes('www.wowhead.com/item=3111'));assert.ok(!html.includes('cp-empty'));
});
test('hover links use the Forever Wowhead path and domain',()=>{
 const {WOWPOP}=harness({name:'Sample',region:'us',gear:[]},{whOk:true});
 const c={id:'test',name:'Sample',game_id:'wow_forever'};
 const html=WOWPOP.body({name:'Sample',region:'us',game_id:'wow_forever',gear:[{slot:'RANGED',id:3111,name:'Bow',quality:'POOR',ilvl:5}]},c);
 assert.ok(html.includes('href="https://www.wowhead.com/forever/item=3111"'));
 assert.ok(html.includes('data-wowhead="domain=forever&amp;ilvl=5"') || html.includes('data-wowhead="domain=forever&ilvl=5"'));
 assert.ok(!html.includes('https://www.wowhead.com/item=3111"'));
 assert.ok(!html.includes('data-whi'));
});
test('Retail keeps Armory, item details and populated sections',()=>{
 const {WOWPOP}=harness();const html=WOWPOP.body({name:'Retail',region:'us',level:80,armory:'https://worldofwarcraft.blizzard.com/character/test',gear:[{slot:'HEAD',id:5,name:'Helm',quality:'EPIC',ilvl:100}],mplus:{rating:1234,runs:[{dungeon:'Test dungeon',level:5,timed:true}]},professions:[{name:'Mining',skill:50,max:75}]},{game_id:'wow'});
 for(const word of ['View on Armory','data-whi="5"','data-whenv="1"','q-epic','Mythic+','Professions','>Share</button>'])assert.ok(html.includes(word),word);
 assert.ok(!html.includes('forever/item='));assert.ok(!html.includes('cp-empty'));
});
test('Retail hover links stay on the live Wowhead item path',()=>{
 const {WOWPOP}=harness({name:'Retail',region:'us',gear:[]},{whOk:true});
 const html=WOWPOP.body({name:'Retail',region:'us',game_id:'wow',gear:[{slot:'HEAD',id:5,name:'Helm',quality:'EPIC',ilvl:100}]},{game_id:'wow'});
 assert.ok(html.includes('href="https://www.wowhead.com/item=5"'));
 assert.ok(html.includes('data-wowhead="ilvl=100"'));
 assert.ok(!html.includes('domain=forever'));assert.ok(!html.includes('forever/item='));
});
test('opening pushes the share URL; closing goes back; direct entry closes in place',async()=>{
 const h=harness(), c={id:'01234567-89ab-cdef-0123-456789abcdef',name:'Sample',game_id:'wow'};
 await h.WOWPOP.open('sample',c);assert.equal(h.calls[0][0],'push');assert.ok(h.location.pathname.includes('/wow/retail/'));
 h.WOWPOP.close();assert.equal(h.calls.at(-1)[0],'back');
 h.calls.length=0;
 await h.WOWPOP.open('sample',c,{deep:true,returnUrl:'/u/sample'});assert.equal(h.calls[0][0],'replace');
 h.WOWPOP.close();assert.deepEqual(h.calls.at(-1),['replace','/u/sample']);
});
test('browser history closes the popup and resolves the current share route',async()=>{
 const h=harness();let resolved=0;h.WOWPOP.setResolver(()=>resolved++);
 await h.WOWPOP.open('sample',{id:'01234567-89ab-cdef-0123-456789abcdef',name:'Sample',game_id:'wow'});
 h.events.popstate();assert.equal(resolved,1);assert.equal(h.calls.length,1);
});

test('both versions expose Share and a supplied Armory link in the same footer',()=>{
 const {WOWPOP}=harness();
 for(const game of ['wow','wow_forever']) {
  const armory=game==='wow_forever'?'https://worldofwarcraft.blizzard.com/en-us/forever/us/armory/character/sample-character':'https://worldofwarcraft.blizzard.com/en-us/character/us/test/sample';
  const html=WOWPOP.body({name:'Sample Character',region:'us',game_id:game,armory},{game_id:game});
  assert.ok(html.includes('data-copychar>Share</button>'));assert.ok(html.includes('View on Armory'));assert.ok(html.includes(armory));
 }
 const css=readFileSync(__dirname+'/../static/assets/site.css','utf8');
 assert.ok(css.includes('.cp-ft{flex-wrap:nowrap}'));assert.ok(css.includes('flex:0 0 auto;flex-wrap:nowrap;white-space:nowrap'));
 assert.ok(source.includes('copy.textContent = "Link copied"'));
});

test('Share copies the stable URL for both versions, confirms, then resets',async()=>{
 for(const game_id of ['wow','wow_forever']) {
  const h=harness(),c={id:'01234567-89ab-cdef-0123-456789abcdef',game_id,name:'Sample'};
  await h.WOWPOP.open('sample',c);
  const button={textContent:'Share',isConnected:true};
  await h.nodes[0].onclick({target:{closest:s=>s==='[data-copychar]'?button:null}});
  assert.equal(h.copied[0],'https://playfraglink.com'+h.WOWLINK.path('sample',c));
  assert.equal(button.textContent,'Link copied');h.timers.at(-1)();assert.equal(button.textContent,'Share');
 }
});

test('Forever characters without an Armory page stay open with a short note',()=>{
 const {WOWPOP}=harness();
 const html=WOWPOP.body({name:'Lucroena Ashborne',level:4,region:'us',realm:'PvP',game_id:'wow_forever',armory_unavailable:true,gear:[],stats:[]},{game_id:'wow_forever',name:'Lucroena Ashborne'});
 const text=html.replace(/<[^>]+>/g,'');
 assert.ok(text.includes('PvP · Forever · US'));
 assert.ok(html.includes('class="cp-realm">PvP</b>'));
 assert.ok(html.includes('class="cp-ver"> · Forever · US</span>'));
 assert.ok(html.includes("The Armory doesn't list this character yet."));
 assert.ok(!html.includes('View on Armory'));
 assert.ok(html.includes('>Share</button>'));
});

function hexLum(hex){
 const h=hex.replace('#','');
 const n=h.length===3?h.split('').map(c=>c+c).join(''):h;
 const rgb=[0,2,4].map(i=>parseInt(n.slice(i,i+2),16)/255);
 const lin=c=>c<=0.04045?c/12.92:((c+0.055)/1.055)**2.4;
 const [r,g,b]=rgb.map(lin);
 return 0.2126*r+0.7152*g+0.0722*b;
}
function contrast(a,b){
 const L1=hexLum(a),L2=hexLum(b),hi=Math.max(L1,L2),lo=Math.min(L1,L2);
 return (hi+0.05)/(lo+0.05);
}
function ruleColor(css,sel){
 const m=css.match(new RegExp(sel.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')+'\\s*\\{[^}]*\\}','m'));
 assert.ok(m,sel+' rule');
 const c=(m[0].match(/color:([^;}]+)/)||[])[1];
 assert.ok(c,sel+' color');
 const hex=(c.trim().match(/#(?:[0-9a-fA-F]{3}){1,2}/)||[])[0];
 assert.ok(hex,sel+' hex color');
 return hex;
}

test('popup header uses class colour, places the crest by the name, and keeps AA contrast',()=>{
 const {WOWPOP}=harness();
 const html=WOWPOP.body({
  name:'Lucrona',level:22,race:'Undead',spec:'Retribution',class:'Paladin',class_id:2,
  realm:'PvP',region:'us',game_id:'wow_forever',faction:'HORDE',
  title:'Lucrona the Kingslayer',
  guild:{name:'Ashen Host',rank:0}
 },{game_id:'wow_forever',name:'Lucrona',faction:'HORDE'});
 const text=html.replace(/<[^>]+>/g,'');
 assert.ok(text.includes('Level 22 Undead Retribution Paladin'));
 assert.ok(text.includes('PvP · Forever · US'));
 assert.ok(text.includes('<Ashen Host> · Guild Master') || text.includes('&lt;Ashen Host&gt; · Guild Master') || html.includes('&lt;Ashen Host&gt; · Guild Master'));
 assert.ok(html.includes('class="cp-cl">Paladin</span>'));
 assert.ok(html.includes('class="cp-realm">PvP</b>'));
 assert.match(html,/<h2 class="cp-nm"[^>]*>Lucrona<img class="faction-crest"/);
 assert.ok(!html.includes('cp-ver">') || !/cp-ver">[^<]*faction-crest/.test(html));
 const afterSrv=html.slice(html.indexOf('class="cp-srv"'));
 assert.ok(!afterSrv.slice(0,afterSrv.indexOf('cp-gu')).includes('faction-crest'),'crest is not after the server line');
 assert.ok(html.includes('style="--cc:#F48CBA"') || html.includes('style="--cc:#f48cba"'));

 const css=readFileSync(__dirname+'/../static/assets/site.css','utf8');
 const bgs=['#111419','#1a2533']; // popup header gradient ends
 const checks={
  '.cp-id':'#c8ced6',
  '.cp-realm':'#eef2f6',
  '.cp-ver':'#b9bec5',
  '.cp-gu':'#a7d08c',
  '.cp-ti':'#c99a62', // var(--label)
 };
 // --label is a variable; .cp-ti may not have a hex. Handle both.
 for(const [sel,expect] of Object.entries(checks)){
  let color;
  if(sel==='.cp-ti'){
   const rule=(css.match(/\.cp-ti\{[^}]+\}/)||[])[0]||'';
   assert.ok(rule.includes('var(--label)') || rule.includes('#c99a62'));
   color='#c99a62';
  } else {
   color=ruleColor(css,sel);
   assert.equal(color.toLowerCase(),expect);
  }
  for(const bg of bgs){
   const ratio=contrast(color,bg);
   assert.ok(ratio>=4.5,`${sel} ${color} on ${bg} is ${ratio.toFixed(2)}:1`);
  }
 }
 assert.ok(css.includes('.cp-cl{color:var(--cc,#eef2f6)'));
 assert.ok(css.includes('.cp-who .faction-crest'));
 assert.ok(css.includes('drop-shadow'));
 assert.ok(!/cp-who .faction-crest\{[^}]*glow/.test(css));
 const crest=css.match(/\.cp-who \.faction-crest\{[^}]+\}/)[0];
 assert.ok(/width:20px/.test(crest) && /height:22px/.test(crest));
});

test('Retail popup header shows the realm in medium-bold ahead of the version line',()=>{
 const {WOWPOP}=harness();
 const html=WOWPOP.body({
  name:'Lucroena',level:80,race:'Dracthyr',spec:'Devastation',class:'Evoker',class_id:13,
  realm:'Frostmourne',region:'us',game_id:'wow',faction:'ALLIANCE'
 },{game_id:'wow',name:'Lucroena',faction:'ALLIANCE'});
 const text=html.replace(/<[^>]+>/g,'');
 assert.ok(text.includes('Level 80 Dracthyr Devastation Evoker'));
 assert.ok(text.includes('Frostmourne · Retail · US'));
 assert.ok(html.includes('class="cp-realm">Frostmourne</b>'));
 assert.ok(html.includes('class="cp-ver"> · Retail · US</span>'));
 assert.ok(html.includes('class="cp-cl">Evoker</span>'));
 assert.match(html,/<h2 class="cp-nm"[^>]*>Lucroena<img class="faction-crest" alt="Alliance"/);
});
