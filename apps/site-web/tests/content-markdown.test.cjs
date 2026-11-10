const {test}=require('node:test');
const assert=require('node:assert/strict');
const {readFileSync}=require('node:fs');
const vm=require('node:vm');
const admin=readFileSync(__dirname+'/../static/assets/admin.js','utf8');
const app=readFileSync(__dirname+'/../static/assets/app.js','utf8');
const start=admin.indexOf('const MD =');
const helper=admin.slice(start,admin.indexOf('\n})();',start)+6);
function harness(api=async()=>({html:'<p>safe</p>'})) {
 const timers=new Map();let id=0;
 const MD=vm.runInNewContext(helper+';MD',{esc:s=>String(s).replaceAll('"','&quot;'),api,setTimeout:fn=>{timers.set(++id,fn);return id},clearTimeout:id=>timers.delete(id)});
 return {MD,timers,async tick(){const f=[...timers.values()].at(-1);timers.clear();return f&&f()}};
}
test('toolbar has accessible non-submit controls for each requested format',()=>{
 const {MD}=harness(),html=MD.toolbar('body');
 for(const label of ['Bold','Italic','Link','Bullet list','Code','Line break','Paragraph'])assert.ok(html.includes('>'+label+'</button>'));
 assert.equal((html.match(/type="button"/g)||[]).length,7);assert.ok(html.includes('role="toolbar"'));assert.ok(html.includes('aria-label="Format body"'));
});
test('selection formatting and inserted breaks preserve surrounding text',()=>{
 const {MD}=harness();
 for(const [action,expected] of [['bold','a **word** z'],['italic','a *word* z'],['code','a `word` z']])assert.equal(MD.edit('a word z',2,6,action).value,expected);
 assert.equal(MD.edit('ab',1,1,'break').value,'a\nb');assert.equal(MD.edit('ab',1,1,'paragraph').value,'a\n\nb');
 assert.equal(MD.edit('word',0,4,'link','https://example.com').value,'[word](https://example.com)');
 for(const url of ['javascript:alert(1)','data:text/html,bad','https://example.com/"bad','//example.com'])assert.equal(MD.edit('word',0,4,'link',url),null);
});
test('bullet formatting skips blank lines and does not duplicate existing markers',()=>{
 const {MD}=harness();const text='first\n\n- second\n\nthird';
 assert.equal(MD.edit(text,0,text.length,'bullet').value,'- first\n\n- second\n\n- third');
 assert.equal(MD.blocks('\n \n').length,0);assert.deepEqual(Array.from(MD.blocks('- one\n\n- two')),['- one\n\n- two']);
 const a=admin.indexOf('const clLines ='),b=admin.indexOf('\n',a);const clLines=vm.runInNewContext(admin.slice(a,b)+';clLines');
 assert.deepEqual(Array.from(clLines('- one\n\n\n- two\n   \n')),['one','two']);
});
test('legacy drafts become editable Markdown without interpreting old formatting',()=>{
 const {MD}=harness();assert.equal(MD.legacy(['**literal**','two']),'- \\*\\*literal\\*\\*\n- two');
 assert.equal(MD.legacy(['1. literal','+ literal','- literal']),'- 1\\. literal\n- \\+ literal\n- \\- literal');
});
test('live preview uses the authenticated shared server renderer and ignores stale replies',async()=>{
 const pending=[],seen=[];let text='first';
 const h=harness((path,options)=>{assert.equal(path,'/admin/markdown/preview');assert.equal(options.method,'POST');return new Promise(resolve=>pending.push({resolve,body:options.body}))});
 const draw=h.MD.preview(()=>({kind:'news',format:'markdown',body:text}),d=>seen.push(d.html),()=>assert.fail('preview failed'));
 draw();const first=h.tick();text='second';draw();const second=h.tick();
 pending[0].resolve({html:'first'});await first;assert.deepEqual(seen,[]);
 pending[1].resolve({html:'second'});await second;assert.deepEqual(seen,['second']);assert.equal(pending[0].body.body,'first');
});
test('preview failures do not render raw input as HTML',async()=>{
 const h=harness(async()=>{throw Error('offline')});let failed=false,drawn=false;
 const preview=h.MD.preview(()=>({body:'<script>bad</script>'}),()=>drawn=true,()=>failed=true);preview();await h.tick();assert.ok(failed);assert.ok(!drawn);
});
test('public pages consume the same server HTML, with escaped legacy fallbacks',()=>{
 assert.ok(app.includes('e.new_html'));assert.ok(app.includes('e.changes_html'));assert.ok(app.includes('e.fixes_html'));assert.ok(app.includes('typeof p.body_html === "string" ? p.body_html : newsBody(p.body)'));
 assert.ok(admin.includes('MD.preview(()=>({kind:"changelog"'));assert.ok(admin.includes('MD.preview(()=>({kind:"news"'));
 for(const name of ['new','changes','fixes','body'])assert.ok(admin.includes('MD.toolbar("'+name+'")'));
 assert.ok(admin.includes('MD.bind(f)'));assert.ok(!admin.includes('innerHTML=f.body.value'));
});

test('changing legacy draft metadata does not convert or rewrite its sections',()=>{
 const {MD}=harness(),a=admin.indexOf('  const cur =',admin.indexOf('async function changelog(')),b=admin.indexOf('\n',a);
 const legacyEntry={new:['**literal**'],changes:[],fixes:[]};
 const f={area:{value:'Website'},date:{value:'2026-11-11'},version:{value:''},new:{value:MD.legacy(legacyEntry.new)},changes:{value:''},fixes:{value:''}};
 const value=vm.runInNewContext(admin.slice(a,b)+';cur()',{f,MD,clFormat:null,legacyEntry});
 assert.equal(value.format,null);assert.equal(value.new,legacyEntry.new);
 assert.ok(admin.includes('if(["new","changes","fixes"].includes(e.target.name)) clFormat="markdown"'));
});
