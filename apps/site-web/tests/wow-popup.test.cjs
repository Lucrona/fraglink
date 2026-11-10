const {readFileSync}=require('node:fs');
const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const source=readFileSync(__dirname+'/../static/assets/app.js','utf8');
const linkSource=source.slice(source.indexOf('const WOWLINK ='),source.indexOf('/* Pager:'));
const start=source.indexOf('const WOWPOP =');
const popupSource=source.slice(start,source.indexOf('\n})();',start)+6);
const esc=x=>String(x??'').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');
function harness(data={name:'Sample',region:'us',level:18,gear:[]}) {
 const events={}, calls=[], copied=[];
 const location={pathname:'/u/sample',search:'',hash:''};
 const history={state:null,pushState(s,_,path){this.state=s;calls.push(['push',path]);location.pathname=path},replaceState(s,_,path){this.state=s;calls.push(['replace',path]);location.pathname=path},back(){calls.push(['back']);location.pathname='/u/sample'}};
 const inside={innerHTML:''};
 const ctx={console,URL,encodeURIComponent,Number,Date,Map,Set,location,history,navigator:{clipboard:{async writeText(s){copied.push(s)}}},
  siteBase:()=> 'https://playfraglink.com',esc,nf:new Intl.NumberFormat('en'),plural:(n,s)=>`${n} ${s}`,factionCrest:()=>'',WH:{ok:false,scan(){}},imgOk:'',
  addEventListener:(name,fn)=>events[name]=fn,document:{createElement(){return {remove(){},innerHTML:'',className:''}},body:{appendChild(){},classList:{add(){},remove(){}}},addEventListener(){},removeEventListener(){}},
  $:(selector)=>selector==='.cp-x'?{focus(){}}:selector==='.cp-in'?inside:null,$$:()=>[],api:async()=>data,setTimeout:()=>0};
 const {WOWPOP,WOWLINK}=vm.runInNewContext(linkSource+popupSource+';({WOWPOP,WOWLINK})',ctx);
 return {WOWPOP,WOWLINK,calls,copied,events,location,history,inside};
}
test('stable share links use version and UUID, not names',()=>{
 const {WOWLINK}=harness();const id='01234567-89ab-cdef-0123-456789abcdef';
 for(const game of ['wow','wow_forever']){
  const path=WOWLINK.path('some user',{id,game_id:game,name:'Mutable Name'});
  const link=WOWLINK.parse(path);assert.equal(link.handle,'some user');assert.equal(link.id,id);assert.equal(link.game_id,game);
 }
 for(const path of ['/u/s/wow/forever/42','/u/s/wow/classic/'+id,'/u/s/wow/forever/../../private'])assert.equal(WOWLINK.parse(path),null);
});
test('empty popup sections and unsupported Forever item lookups are absent',()=>{
 const {WOWPOP}=harness();const c={id:'test',name:'Sample',game_id:'wow_forever'};
 let html=WOWPOP.body({name:'Sample',level:18,region:'us',game_id:'wow_forever'},c);
 for(const word of ['<h3>Gear','<h3>Stats','Raid progress','Mythic+','Collections','Professions','cp-empty','not available','No gear'])assert.ok(!html.includes(word),word);
 assert.ok(html.includes('Copy link'));assert.ok(!html.includes('View on Armory'));
 html=WOWPOP.body({name:'Sample',region:'us',game_id:'wow_forever',gear:[{slot:'RANGED',id:3111,name:'Bow',quality:'POOR',icon:'inv_weapon_bow_01',ilvl:5}],stats:[{name:'Health',value:508},{name:'Melee crit',value:4.8004,percent:true}]},c);
 assert.ok(html.includes('q-poor'));assert.ok(html.includes('Ranged'));assert.ok(html.includes('508'));assert.ok(html.includes('4.80%'));
 assert.ok(!html.includes('data-whi'));assert.ok(!html.includes('wowhead.com'));assert.ok(!html.includes('cp-empty'));
});
test('Retail keeps Armory, item details and populated sections',()=>{
 const {WOWPOP}=harness();const html=WOWPOP.body({name:'Retail',region:'us',level:80,armory:'https://worldofwarcraft.blizzard.com/character/test',gear:[{slot:'HEAD',id:5,name:'Helm',quality:'EPIC',ilvl:100}],mplus:{rating:1234,runs:[{dungeon:'Test dungeon',level:5,timed:true}]},professions:[{name:'Mining',skill:50,max:75}]},{game_id:'wow'});
 for(const word of ['View on Armory','data-whi="5"','q-epic','Mythic+','Professions','Copy link'])assert.ok(html.includes(word),word);
 assert.ok(!html.includes('cp-empty'));
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
