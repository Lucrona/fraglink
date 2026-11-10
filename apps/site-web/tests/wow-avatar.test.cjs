const {readFileSync}=require('node:fs');
const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const source=readFileSync(__dirname+'/../static/assets/app.js','utf8');
const start=source.indexOf('const WOWAV =');
const helper=source.slice(start,source.indexOf('\n})();',start)+6);
const src='https://render.worldofwarcraft.com/forever-us/character/1/42-avatar.jpg';
const esc=s=>String(s??'').replaceAll('"','&quot;');
function harness(api=async()=>({avatar_src:src})) {
 const ctx={esc,imgOk:'data-hide-broken',WOW:{clsIcon:()=>'/class.jpg',raceIcon:()=>'/retail.jpg'},$$:(_,root)=>root,api};
 return vm.runInNewContext(helper+';WOWAV',ctx);
}
function image(id='42') {
 const attrs={src:'/class.jpg'},dataset={wowAvatar:id};
 return {attrs,dataset,isConnected:true,style:{},tagName:'IMG',getAttribute:k=>attrs[k],setAttribute(k,v){attrs[k]=v},hasAttribute:k=>Object.hasOwn(attrs,k),removeAttribute(k){delete attrs[k]},get src(){return attrs.src},set src(v){attrs.src=v}};
}
test('Forever image prefers the avatar and class fallback; Retail stays unchanged',()=>{
 const av=harness(),c={id:'42',game_id:'wow_forever',class:'WARRIOR',avatar_src:src};
 assert.ok(av.image(c).includes(`src="${src}"`));assert.ok(av.image(c).includes('data-wow-fallback="/class.jpg"'));
 assert.ok(av.image({...c,avatar_src:null}).includes('src="/class.jpg"'));
 assert.ok(av.image({...c,avatar_src:'https://example.com/fake.jpg'}).includes('src="/class.jpg"'));
 const retail=av.image({...c,game_id:'wow'});assert.ok(retail.includes('/retail.jpg'));assert.ok(!retail.includes('data-wow-avatar'));
});
test('media hydration deduplicates requests and retains class icons on missing/error',async()=>{
 let calls=0;const av=harness(async path=>{calls++;assert.ok(path.startsWith('/users/test/wow/'));return {avatar_src:src}}),a=image(),b=image();
 await av.fill([a,b],'test');assert.equal(calls,1);assert.equal(a.src,src);assert.equal(b.src,src);assert.equal(a.attrs['data-wow-fallback'],'/class.jpg');
 for(const api of [async()=>({avatar_src:null}),async()=>{throw Error('unavailable')},async()=>({avatar_src:'https://evil.test/avatar.jpg'})]) {
  const i=image();await harness(api).fill([i],'test');assert.equal(i.src,'/class.jpg');
 }
});
test('media hydration limits concurrency and ignores detached images',async()=>{
 let active=0,max=0;
 const av=harness(async()=>{max=Math.max(max,++active);await new Promise(setImmediate);active--;return {avatar_src:src}});
 const images=Array.from({length:18},(_,i)=>image(String(i)));images[0].isConnected=false;
 await av.fill(images,'test');assert.equal(max,6);assert.equal(images[0].src,'/class.jpg');assert.equal(images[17].src,src);
});
test('broken avatars fall back once to class icons, then use normal broken handling',()=>{
 let error;const ctx={document:{addEventListener:(_,fn)=>error=fn}};
 vm.runInNewContext(source.slice(source.indexOf('document.addEventListener("error"'),source.indexOf('document.addEventListener("click"')),ctx);
 const i=image();i.setAttribute('data-wow-fallback','/class.jpg');i.setAttribute('data-hide-broken','');i.src=src;
 error({target:i});assert.equal(i.src,'/class.jpg');assert.equal(i.style.visibility,undefined);
 error({target:i});assert.equal(i.style.visibility,'hidden');
});
test('profile, gallery, manager and popup all use the shared portrait helper',()=>{
 for(const anchor of ['${WOWAV.image(c)}','${WOWAV.image(c, \'class="r" loading="lazy"\')}','WOWAV.fill(list, (me() || {}).handle)','WOWAV.fill(root, p.handle)','WOWAV.fill(box, p.handle)','WOWAV.fill(box, handle)'])assert.ok(source.includes(anchor),anchor);
 assert.ok(source.includes('forever ? WOWAV.image({ ...c'));
});

test('initial profile hydrates only after the profile cards are mounted',()=>{
 const mount=source.indexOf('root.innerHTML = `<div class="pv3">`');
 const hydrate=source.indexOf('WOWAV.fill(root, p.handle)',mount);
 assert.ok(mount>0 && hydrate>mount);
 assert.ok(hydrate<source.indexOf('const sm =',mount));
});
