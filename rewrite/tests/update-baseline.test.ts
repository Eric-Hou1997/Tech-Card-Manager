import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {baseParse} from '@vue/compiler-dom';
import {createSSRApp,h,reactive,nextTick} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {component,renderer,find} from './support/mount-vue.ts';
import * as updateModule from '../src/card-update.ts';
import {languageKey} from '../src/baseline-language.ts';
import type {ReleaseCheck} from '../src/contracts';
const {initialCardUpdate,createCardUpdate}=updateModule;
const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const fixture:ReleaseCheck={current_version:'v5.0.0',latest_version:'v5.0.1',available:true,package_name:'TCM-v5.0.1-Windows-x64-EXE.zip',package_url:'https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/download/v5.0.1/TCM-v5.0.1-Windows-x64-EXE.zip',release_url:'',published_at:'',portable_directory:'C:\\TCM',source:'github-api',checked_at:'2026-09-13T00:00:00Z',warning:null,warning_code:null,retry_at:null,candidate_id:'receipt'};
function baseline(result:ReleaseCheck|Error,locale='zh-CN'){
 const nodes=new Map<string,any>();const $=(id:string)=>{if(!nodes.has(id))nodes.set(id,{textContent:'',classList:{add(){},remove(){}},disabled:false});return nodes.get(id);};
 const context:any={$,uiLanguage:locale,Date,document:{querySelector:$},api:async()=>{if(result instanceof Error)throw result;return result;}};
 vm.createContext(context);vm.runInContext(source.slice(source.indexOf("let cardReleaseURL=''")).split('</script>')[0],context);
 return {context,$};
}
function normalize(html:string):unknown {
 function walk(node:any):unknown{if(node.type===3)return null;if(node.type===2)return node.content.trim()||null;if(node.type===0)return node.children.map(walk).filter(Boolean);return [node.tag,Object.fromEntries(node.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='class'?(p.value?.content||'').split(/\s+/).filter(Boolean).sort().join(' '):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),node.children.map(walk).filter(Boolean)];}return walk(baseParse(html));
}
test('actual Vue update section keeps the original initial markup and inline confirmation',async()=>{
 const Panel=await component(new URL('../src/UpdatePanel.vue',import.meta.url),{'@tauri-apps/api/core':{invoke:()=>assert.fail('hidden panel must not check')},'./card-update':updateModule});
 const html=await renderToString(createSSRApp({render:()=>h(Panel,{active:false,identity:null})}));
 const start=source.indexOf('<div class="aboutUpdate">'),end=source.indexOf('<div class="aboutRule">',start);
 const expected=source.slice(start,end).replaceAll('TCM-v4.1.0-Windows-x64-EXE.zip','TCM-v5.0.0-Windows-x64-EXE.zip');
 assert.deepEqual(normalize(html),normalize(expected));
});
test('release, current, cached, rate-limit warning and error labels match original functions',async()=>{
 for(const locale of ['zh-CN','en-US'])for(const result of [fixture,{...fixture,available:false},{...fixture,source:'stale-cache',warning:'GitHub 匿名 API 的出口 IP 额度已用完',retry_at:'2026-09-13T00:01:00Z'},Object.assign(Error('无法连接 GitHub'),{data:{retry_at:'2026-09-13T00:01:00Z'}})]){
  const old=baseline(result,locale);await old.context.checkCardUpdate(false);
  const state=initialCardUpdate();const model=createCardUpdate(async<T>()=>{if(result instanceof Error)throw {message:result.message,...(result as any).data};return result as T;},state,()=>{},()=>locale);await model.check(false);
  assert.equal(state.label,old.$('#cardUpdateState').textContent);assert.equal(state.button,old.$('#checkCardUpdate').textContent);assert.equal(state.busy,old.$('#checkCardUpdate').disabled);model.dispose();
 }
});
test('check coalesces clicks, requires inline confirmation, and ignores late responses after disposal',async()=>{
 let resolve!:(value:ReleaseCheck)=>void;const calls:any[]=[];const state=initialCardUpdate();
 const model=createCardUpdate(async<T>(name,args)=>{calls.push([name,args]);if(name==='check_card_update')return await new Promise<ReleaseCheck>(r=>resolve=r) as T;return undefined as T;},state,()=>{});
 const check=model.check(false);await model.activate();await model.check(true);assert.equal(calls.length,1);assert.equal(state.busy,true);
 resolve(fixture);await check;assert.equal(state.prompt,false);await model.activate();assert.equal(state.prompt,true);assert.equal(calls.length,1);
 model.cancel();assert.equal(state.prompt,false);await model.activate();await model.download();assert.deepEqual(calls[1],['open_card_update',{candidateId:'receipt'}]);
 const pending=model.check(false);model.dispose();const before=structuredClone(state);resolve({...fixture,latest_version:'v5.0.2'});await pending;assert.deepEqual(state,before);
});
test('opening settings checks once per opening; real Vue events only open the confirmed download',async()=>{
 const calls:any[]=[];const errors:string[]=[];let rejectOpen=false;
 const Panel=await component(new URL('../src/UpdatePanel.vue',import.meta.url),{'@tauri-apps/api/core':{invoke:async(name:string,args:any)=>{calls.push([name,args]);if(name==='check_card_update')return fixture;if(rejectOpen)throw {message:'无法打开安装包下载地址'};}},'./card-update':updateModule});
 const state=reactive({active:false});const {host,render}=renderer();const app=render.createApp({render:()=>h(Panel,{active:state.active,identity:null,onNotify:(message:string)=>errors.push(message)})});app.mount(host);
 assert.equal(calls.length,0);state.active=true;await nextTick();await nextTick();assert.deepEqual(calls,[['check_card_update',{force:false}]]);
 await find(host,'checkCardUpdate')!.props.onClick();await nextTick();assert.equal(find(host,'cardInstallPrompt')!.props.class,'aboutInstall');assert.equal(calls.length,1);assert.equal(find(host,'cardInstallTitle')!.text,'最新版本 v5.0.1 可用');
 find(host,'cancelCardInstall')!.props.onClick();await nextTick();assert.equal(find(host,'cardInstallPrompt')!.props.class,'aboutInstall hide');
 await find(host,'checkCardUpdate')!.props.onClick();await find(host,'confirmCardInstall')!.props.onClick();assert.equal(calls[1][0],'open_card_update');
 rejectOpen=true;await find(host,'confirmCardInstall')!.props.onClick();assert.deepEqual(errors,['无法打开安装包下载地址']);
 state.active=false;await nextTick();state.active=true;await nextTick();await nextTick();assert.deepEqual(calls.at(-1),['check_card_update',{force:false}]);assert.equal(find(host,'cardInstallPrompt')!.props.class,'aboutInstall hide');app.unmount();
});

test('about links keep the original anchors and route only fixed destinations through the native browser',async()=>{
 const calls:any[]=[];
 const About=await component(new URL('../src/AboutPanel.vue',import.meta.url),{
  '@tauri-apps/api/core':{invoke:async(name:string,args:any)=>{calls.push([name,args]);if(name==='update_identity')return {product:'TCM',os:'windows',arch:'aarch64',channel:'nsis'};}},
  './assets/TCM_logo_tiny.png':{default:'logo.png'},
  './UpdatePanel.vue':{default:{render:()=>null}},
  './baseline-language':await import('../src/baseline-language.ts'),
 });
 const {host,render}=renderer();const notices:string[]=[];const app=render.createApp({render:()=>h(About,{active:true,onNotify:(message:string)=>notices.push(message)})});
 const language=reactive({locale:'en-US'});app.provide(languageKey,{snapshot:language} as any);app.mount(host);await nextTick();await nextTick();
 const currentAnchors=()=>{const anchors:any[]=[];const visit=(node:any)=>{if(node.type==='a')anchors.push(node);for(const child of node.children||[])visit(child);};visit(host);return anchors;};let anchors=currentAnchors();
 assert.deepEqual(anchors.map(anchor=>anchor.props.href),[
  'https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/docs/legal/PRIVACY.en.md',
  'https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/docs/legal/TERMS.en.md',
  'https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/LICENSE',
  'https://github.com/Eric-Hou1997/Tech-Card-Manager',
 ]);
 language.locale='zh-Hant';await nextTick();
 anchors=currentAnchors();
 assert.deepEqual(anchors.slice(0,2).map(anchor=>anchor.props.href),[
  'https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/PRIVACY.md',
  'https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/TERMS.md',
 ]);
 language.locale='en-US';await nextTick();
 anchors=currentAnchors();
 for(const anchor of anchors)await anchor.props.onClick({preventDefault(){}});
 assert.deepEqual(calls.slice(1),[
  ['open_product_link',{kind:'privacy',english:true}],
  ['open_product_link',{kind:'terms',english:true}],
  ['open_product_link',{kind:'license',english:true}],
  ['open_product_link',{kind:'repository',english:true}],
 ]);
 assert.deepEqual(notices,[]);app.unmount();
});
