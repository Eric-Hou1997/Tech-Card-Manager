import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {baseParse} from '@vue/compiler-dom';
import {createSSRApp,h,ref,nextTick} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {component,renderer} from './support/mount-vue.ts';
const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const Dialog=await component(new URL('../src/LegacyDialog.vue',import.meta.url));
function normalize(html:string):unknown {
 function walk(n:any):unknown{if(n.type===3)return null;if(n.type===2)return n.content.trim()||null;if(n.type===0)return n.children.map(walk).filter(Boolean);return [n.tag,Object.fromEntries(n.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='class'?(p.value?.content||'').split(/\s+/).filter(Boolean).sort().join(' '):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),n.children.map(walk).filter(Boolean)];}return walk(baseParse(html));
}
test('legacy dialog retains original unsafe and reviewable markup, text and escaped items',async()=>{
 const original=source.split('\n').find(line=>line.startsWith('<div class="modalBackdrop" id="legacyBackdrop"'))!;
 for(const unsafePatch of [false,true])for(const visible of [false,true]){
  let expected=original.replace('v4.1.0','v5.0.0').replace('class="modalBackdrop"',visible?'class="modalBackdrop show"':'class="modalBackdrop"').replace('<ul class="legacyList" id="legacyList"></ul>','<ul class="legacyList" id="legacyList"><li>路径 &lt;测试&gt; &amp; 旧组件</li></ul>');
  if(unsafePatch)expected=expected.replace('id="confirmLegacy"','id="confirmLegacy" disabled').replace('迁移所列组件并继续','需要先人工确认网页文件');
  const actual=await renderToString(createSSRApp({render:()=>h(Dialog,{visible,unsafePatch,items:['路径 <测试> & 旧组件']})}));
  assert.deepEqual(normalize(actual),normalize(expected));
 }
});
test('unsafe patch prompt cancels without mutation, reopens on Start and rechecks changed evidence',async()=>{
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),code=await readFile(file,'utf8');
 const deps:Record<string,unknown>={};
 for(const path of code.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{name:path[1],render:()=>null}};
 const target=ref<any>({target:'/web',installed:true,healthy:false,issues:['emby-unknown-ownership'],legacy_patch:{unsafe_patch:true,items:['无法确认所有权的网页补丁（标记、脚本数量或块内容异常）'],fingerprint:'first'}});
 const service=ref({phase:'stopped'}),plan=ref<any>(null),legacyComponents=ref<any>(null),legacyPlan=ref<any>(null);let reads=0,vnode:any,systemReady=false,systemStarts=0;
 const config={revision:0,locale:'zh-CN',roots:[]};const view=()=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
 deps['@tauri-apps/api/core']={invoke:async(name:string)=>{
  if(name==='emby_migrate_legacy_system'){if(!systemReady)throw {code:'legacy-system-component-unverified',message:'system cleanup not connected'};assert.equal(legacyPlan.value.id,'resumed-system');legacyPlan.value={...legacyPlan.value,phase:'complete'};return legacyPlan.value;}if(name==='emby_legacy_operation')return legacyPlan.value;if(name==='emby_legacy_components')return legacyComponents.value;if(name==='configuration')return config;if(name==='ui_state')return {revision:0,active_space:'movie',movie:view(),tv:view()};
  if(name==='manager_catalog'||name==='task_history')return [];
  if(name==='catalog_summary')return {roots_configured:true,total:0};
  assert.fail('Unexpected mutation or IPC '+name);
 }};
 deps['@tauri-apps/api/event']={listen:async()=>()=>{}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 let migrationCalls=0,finishMigration:(value:unknown)=>void=()=>{},failMigration:(error:unknown)=>void=()=>{};
 const migration=await import('../src/legacy-migration.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};deps['./confirmation']={};deps['./maintenance']={};deps['./legacy-migration']={...migration,migrateLegacyCard:async(_invoke:unknown,attempt:any)=>{migrationCalls++;if(migrationCalls===1)assert.equal(attempt.id,'restored-attempt');else assert.notEqual(attempt.id,'restored-attempt');assert.equal(attempt.target,'/web');assert.equal(attempt.reviewed,'changed');return new Promise((resolve,reject)=>{finishMigration=resolve;failMigration=reject;});}};
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>({target,service,plan,legacyComponents,legacyPlan,refreshLegacy:async()=>{},busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref(''),refresh:async()=>{reads++;},run:async(work:()=>Promise<void>)=>work(),acceptService:(value:any)=>{service.value=value;},control:()=>{assert(systemReady,'Unsafe migration cannot start/stop the service');systemStarts++;service.value={phase:'running'};}})};
 const Panel=await component(file,deps),{host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(predicate:(v:any)=>boolean,value:any=vnode):any{if(value&&predicate(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(predicate,child);if(found)return found;}}
 const modal=()=>node(n=>n.type?.name==='./LegacyDialog.vue');const settle=async()=>{for(let i=0;i<15;i++)await nextTick();};
 try{app.mount(host);await settle();assert.equal(modal().props.visible,true);assert.equal(modal().props['unsafe-patch'],true);await modal().props.onConfirm();
  modal().props.onCancel();await settle();assert.equal(modal().props.visible,false);
  assert.equal(node(n=>n.props?.id==='toast').children,'已取消；旧版保持不变，新版服务未启动');
  await node(n=>n.type?.name==='./ConsolePanel.vue').props.onToggle();await settle();assert.equal(reads,1);assert.equal(modal().props.visible,true);
  modal().props.onCancel();await settle();target.value={...target.value,legacy_patch:{...target.value.legacy_patch,fingerprint:'changed'}};await settle();assert.equal(modal().props.visible,true);
  assert.equal(migrationCalls,0);
  plan.value={id:'restored-attempt',target:'/web',action:'adopt',phase:'planned',legacy_review:'changed'};
  target.value={...target.value,legacy_patch:{...target.value.legacy_patch,unsafe_patch:false}};await settle();
  assert.equal(modal().props['unsafe-patch'],false);
  const confirm=modal().props.onConfirm;const completing=confirm();await settle();
  assert.equal(modal().props.visible,false);assert.equal(node(n=>n.type?.name==='./ConsolePanel.vue').props.phase,'migrating');
  await confirm();assert.equal(migrationCalls,1);
  failMigration(new migration.LegacyMigrationFailure('review changed',true));await assert.rejects(completing);await settle();
  const retry=modal().props.onConfirm();await settle();assert.equal(migrationCalls,2);
  finishMigration({phase:'running'});await retry;await settle();assert.equal(modal(),undefined);assert.equal(service.value.phase,'running');
  service.value={phase:'stopped'};target.value={...target.value,legacy_patch:null,healthy:true,issues:[]};
  legacyComponents.value={fingerprint:'system-first',items:['旧版程序 PID 123：C:\\旧版\\Manager.exe'],errors:[]};await settle();
  assert.equal(modal().props.visible,true);assert.deepEqual(modal().props.items,legacyComponents.value.items);
  assert.equal(node(n=>n.type?.name==='./ConsolePanel.vue').props.phase,'legacy-blocked');
  modal().props.onCancel();await settle();assert.equal(modal().props.visible,false);
  await node(n=>n.type?.name==='./ConsolePanel.vue').props.onToggle();await settle();assert.equal(modal().props.visible,true);
  await assert.rejects(modal().props.onConfirm(),{message:'system cleanup not connected'});assert.equal(migrationCalls,2);
  modal().props.onCancel();legacyComponents.value={fingerprint:'system-changed',items:[],errors:[{code:'denied',message:'读取失败',path:'C:\\原目录'}]};await settle();
  assert.equal(modal().props.visible,true);assert.deepEqual(modal().props.items,['读取失败：C:\\原目录']);
  legacyComponents.value={fingerprint:'empty',items:[],errors:[]};legacyPlan.value={id:'resumed-system',reviewed:'original-system',phase:'running',items:['旧版登录启动项']};await settle();
  assert.equal(modal().props.visible,true);assert.deepEqual(modal().props.items,['旧版登录启动项']);
  modal().props.onCancel();await settle();assert.equal(node(n=>n.props?.id==='toast').children,'旧版迁移未完成，请查看故障处理信息。');
  systemReady=true;await modal().props.onConfirm();await settle();assert.equal(systemStarts,1);assert.equal(modal(),undefined);
  target.value={...target.value,index_current:false,details:{script_version:'4.1.0',data_valid:true,runtime_valid:false}};await settle();
  assert.equal(node(n=>n.type?.name==='./ConsolePanel.vue').props['web-healthy'],false);
  assert.equal(node(n=>n.props?.id==='productBannerText').children,'媒体目录与卡片索引不一致，程序正在重建；完成前不会显示过期卡片。');
  target.value={...target.value,index_current:true};await settle();
  assert.equal(node(n=>n.type?.name==='./ConsolePanel.vue').props['web-healthy'],true);
  assert.ok(!String(node(n=>n.props?.id==='productBanner').props.class).split(' ').includes('show'));



 }finally{app.unmount();}
});
