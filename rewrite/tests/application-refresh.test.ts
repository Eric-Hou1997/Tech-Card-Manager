import test from 'node:test';
import assert from 'node:assert/strict';
import {nextTick,ref} from 'vue';
import {readFile} from 'node:fs/promises';
import {component,renderer} from './support/mount-vue.ts';
import {LifecycleSettings} from '../src/lifecycle-settings.ts';
import {Languages,languageKey} from '../src/baseline-language.ts';

test('starting the original read-only service does not require a healthy Web Card',async()=>{
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),source=await readFile(file,'utf8'),deps:Record<string,unknown>={};
 for(const path of source.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{name:path[1],render:()=>null}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};
 deps['./confirmation']={};deps['./maintenance']={};deps['./legacy-migration']={};
 const calls:string[]=[];
 const target=ref<any>({target:'/Emby/web',healthy:false,installed:false,requires_permission:false,issues:[],legacy_patch:null});
 const model={target,service:ref({phase:'stopped'}),plan:ref(null),legacyComponents:ref(null),legacyPlan:ref(null),busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref(''),
  run:async(work:()=>Promise<void>)=>work(),refreshLegacy:async()=>{calls.push('review');},control:async(start:boolean)=>{calls.push(start?'start':'stop');}};
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>model};
 deps['@tauri-apps/api/core']={invoke:async(name:string)=>{
  if(name==='configuration')return {revision:1,locale:'zh-CN',roots:[{id:'movie',path:'/Movie',space:'movie'}]};
  if(name==='manager_catalog'||name==='task_history')return [];
  if(name==='catalog_summary')return {total:0,roots_configured:true};
  assert.fail(name);
 }};deps['@tauri-apps/api/event']={listen:async()=>()=>{}};
 const Panel=await component(file,deps),{host,render}=renderer();let vnode:any;
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function consoleNode(value:any=vnode):any{if(value?.type?.name==='./ConsolePanel.vue')return value;for(const child of Array.isArray(value)?value:Array.isArray(value?.children)?value.children:[]){const found=consoleNode(child);if(found)return found;}}
 try{
  app.mount(host);for(let i=0;i<24;i++)await nextTick();
  await consoleNode().props.onToggle();assert.deepEqual(calls,['review','start']);
  calls.length=0;target.value={...target.value,requires_permission:true};await nextTick();
  await consoleNode().props.onToggle();assert.deepEqual(calls,['review']);
  calls.length=0;target.value=null;await nextTick();
  await consoleNode().props.onToggle();assert.deepEqual(calls,['review']);
 }finally{app.unmount();}
});

test('catalog read failure replaces stale rows and preview with original errors, then restores selection',async(t)=>{
 t.mock.timers.enable({apis:['setTimeout','Date'],now:0});
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),source=await readFile(file,'utf8'),deps:Record<string,unknown>={};
 for(const path of source.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{name:path[1],render:()=>null}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};
 deps['./confirmation']={};deps['./maintenance']={};deps['./legacy-migration']={};
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>({target:ref(null),service:ref({phase:'stopped'}),plan:ref(null),legacyComponents:ref(null),legacyPlan:ref(null),busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref('')})};
 const item={id:'selected',space:'movie',title:'原选中项',path:'/Movies/movie.nfo',specs:{},tags:[],source_hash:'source'};
 const view=()=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
 const events=new Map<string,()=>void>();let fail=false,reads=0,inspections=0,vnode:any;
 deps['@tauri-apps/api/core']={invoke:async(name:string,args:any)=>{
  if(name==='configuration')return {revision:0,locale:'zh-CN',roots:[]};
  if(name==='ui_state')return {revision:0,active_space:'movie',movie:view(),tv:view()};
  if(name==='manager_catalog'){reads++;if(fail)throw Error('database unavailable');return [{item,series_title:''}];}
  if(name==='task_history')return [];
  if(name==='catalog_summary')return {roots_configured:true,total:1};
  if(name==='inspector'){inspections++;return item;}
  if(name==='save_ui_state')return {revision:args.value.revision+1};
  assert.fail('Unexpected command '+name);
 }};
 deps['@tauri-apps/api/event']={listen:async(name:string,callback:()=>void)=>{events.set(name,callback);return ()=>events.delete(name);}};
 const Panel=await component(file,deps),{host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(predicate:(value:any)=>boolean,value:any=vnode):any{if(Array.isArray(value)){for(const child of value){const found=node(predicate,child);if(found)return found;}return;}if(value&&predicate(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(predicate,child);if(found)return found;}}
 const panel=(name:string)=>node(value=>value.type?.name===name);
 const settle=async()=>{for(let i=0;i<24;i++)await nextTick();};
 try{
  app.mount(host);await settle();assert.equal(panel('./CatalogPreview.vue').props.item.id,item.id);
  fail=true;events.get('task-changed')!();t.mock.timers.tick(150);await settle();
  assert.equal(panel('./CatalogList.vue').props.error,'database unavailable');
  assert.equal(panel('./CatalogPreview.vue').props.unavailable,true);
  assert.equal(panel('./CatalogPreview.vue').props.item,null);
  // Recovery must not depend on a new scan event or a manual button click.
  fail=false;t.mock.timers.tick(5250);await settle();
  assert.equal(panel('./CatalogList.vue').props.error,'');
  assert.equal(panel('./CatalogPreview.vue').props.unavailable,false);
  assert.equal(panel('./CatalogPreview.vue').props.item.id,item.id);
  assert.equal(reads,3);assert.equal(inspections,0);
  const search=()=>node(value=>value.props?.id==='catalogSearch');
  search().props['onUpdate:modelValue']('absent');search().props.onInput();await settle();
  assert.equal(panel('./CatalogList.vue').props.rows.length,1);
  t.mock.timers.tick(199);await settle();assert.equal(panel('./CatalogList.vue').props.rows.length,1);
  t.mock.timers.tick(1);await settle();assert.equal(panel('./CatalogList.vue').props.rows.length,0);
  assert.equal(panel('./CatalogPreview.vue').props.item,null);
  search().props['onUpdate:modelValue']('');search().props.onInput();t.mock.timers.tick(200);await settle();
  assert.equal(panel('./CatalogPreview.vue').props.item.id,item.id);
  node(value=>value.props?.id==='catalogTvTab').props.onClick();await settle();
  assert.equal(panel('./CatalogPreview.vue').props.item,null);
  node(value=>value.props?.id==='catalogMovieTab').props.onClick();await settle();
  assert.equal(panel('./CatalogPreview.vue').props.item.id,item.id);
  assert.equal(reads,3);assert.equal(inspections,0);
 }finally{app.unmount();}
});

test('a task in either media space disables the original global refresh and maintenance actions',async()=>{
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),source=await readFile(file,'utf8'),deps:Record<string,unknown>={};
 for(const path of source.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{name:path[1],render:()=>null}};
 deps['./SettingsDialog.vue']={default:{name:'./SettingsDialog.vue',setup:(_props:any,{slots}:any)=>()=>slots.default?.()}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};deps['./confirmation']={};deps['./maintenance']={};deps['./legacy-migration']={};
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>({target:ref(null),service:ref({phase:'stopped'}),plan:ref(null),legacyComponents:ref(null),legacyPlan:ref(null),busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref('')})};
 deps['@tauri-apps/api/core']={invoke:async(name:string)=>{
  if(name==='configuration')return {revision:0,locale:'zh-CN',roots:[{id:'movie',path:'/Movie',space:'movie'},{id:'tv',path:'/TV',space:'tv'}]};
  if(name==='manager_catalog')return [];
  if(name==='task_history')return [{id:'tv-job',space:'tv',state:'running',processed:0,errors:0}];
  if(name==='catalog_summary')return {total:0,movie:0,tv:0,errors:0,displayable:0,web_eligible:0,episodes_excluded:0,generated_at:null,roots_configured:true};
  assert.fail(name);
 }};deps['@tauri-apps/api/event']={listen:async()=>()=>{}};
 const Panel=await component(file,deps),{host,render}=renderer();let vnode:any;
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(predicate:(value:any)=>boolean,value:any=vnode):any{if(Array.isArray(value)){for(const child of value){const found=node(predicate,child);if(found)return found;}return;}if(value&&predicate(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(predicate,child);if(found)return found;}}
 const panel=(name:string)=>node(value=>value.type?.name===name),byId=(id:string)=>node(value=>value.props?.id===id);
 try{app.mount(host);for(let i=0;i<20;i++)await nextTick();
  assert.equal(byId('refreshLibrary').props.disabled,true);
  byId('openSettings').props.onClick();await nextTick();await nextTick();
  const settings=panel('./SettingsDialog.vue').children.default();
  const nested=(name:string)=>node(value=>value.type?.name===name,settings);
  assert.equal(nested('./RootSettings.vue').props.blocked,true);
  assert.equal(nested('./DiagnosticsPanel.vue').props.blocked,true);
  assert.equal(nested('./DiagnosticsPanel.vue').props['task-running'],true);
  assert.equal(nested('./IncrementalPanel.vue').children.default()[0].props.disabled,true);
  assert.ok(nested('./DiagnosticsPanel.vue').children.default().every((button:any)=>button.props.disabled===true));
 }finally{app.unmount();}
});

test('copying the selected NFO path retains original success and failure feedback',async()=>{
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),source=await readFile(file,'utf8'),deps:Record<string,unknown>={};
 for(const path of source.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{name:path[1],render:()=>null}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};
 deps['./confirmation']={};deps['./maintenance']={};deps['./legacy-migration']={};
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>({target:ref(null),service:ref({phase:'stopped'}),plan:ref(null),legacyComponents:ref(null),legacyPlan:ref(null),busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref('')})};
 const item={id:'selected',space:'movie',title:'片名',path:'/Movies/原路径/movie.nfo',specs:{},source_hash:'source'};
 const view=()=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
 deps['@tauri-apps/api/core']={invoke:async(name:string)=>{
  if(name==='configuration')return {revision:0,locale:'zh-CN',roots:[]};
  if(name==='ui_state')return {revision:0,active_space:'movie',movie:view(),tv:view()};
  if(name==='manager_catalog')return [{item,series_title:''}];
  if(name==='task_history')return [];
  if(name==='catalog_summary')return {roots_configured:true,total:1};
  if(name==='inspector')return item;
  assert.fail('Unexpected command '+name);
 }};
 deps['@tauri-apps/api/event']={listen:async()=>()=>{}};
 const prior=Object.getOwnPropertyDescriptor(navigator,'clipboard'),copies:string[]=[];
 let reject=false,vnode:any;
 Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async(path:string)=>{copies.push(path);if(reject)throw Error('permission denied');}}});
 const Panel=await component(file,deps),{host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(predicate:(value:any)=>boolean,value:any=vnode):any{if(value&&predicate(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(predicate,child);if(found)return found;}}
 const settle=async()=>{for(let i=0;i<20;i++)await nextTick();};
 try{
  app.mount(host);await settle();
  const preview=()=>node(value=>value.type?.name==='./CatalogPreview.vue'),toast=()=>node(value=>value.props?.id==='toast');
  assert.equal(preview().props.item.id,item.id);
  await preview().props.onCopy();await settle();assert.equal(toast().children,'路径已复制');
  reject=true;await preview().props.onCopy();await settle();assert.equal(toast().children,'无法复制路径');
  assert.deepEqual(copies,[item.path,item.path]);assert.equal(preview().props.item.id,item.id);
 }finally{app.unmount();if(prior)Object.defineProperty(navigator,'clipboard',prior);else delete (navigator as any).clipboard;}
});

test('failed first status read retains loading and retries without inventing empty data',async(t)=>{
 t.mock.timers.enable({apis:['setTimeout']});
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),source=await readFile(file,'utf8'),deps:Record<string,unknown>={};
 for(const path of source.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{name:path[1],render:()=>null}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};
 deps['./confirmation']={};deps['./maintenance']={};deps['./legacy-migration']={};
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>({target:ref(null),service:ref({phase:'stopped'}),plan:ref(null),legacyComponents:ref(null),legacyPlan:ref(null),busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref('')})};
 let reads=0,released=0,ready=0,vnode:any;
 const summary={total:0,movie:0,tv:0,errors:0,displayable:0,web_eligible:0,episodes_excluded:0,generated_at:'2026-09-13T12:00:00Z',roots_configured:true};
 const view=()=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
 deps['@tauri-apps/api/core']={invoke:async(name:string)=>{
  if(name==='configuration'){reads++;if(reads===1)throw Error('database unavailable');return {revision:0,locale:'zh-CN',roots:[]};}
  if(name==='ui_state')return {revision:0,active_space:'movie',movie:view(),tv:view()};
  if(name==='manager_catalog'||name==='task_history')return [];
  if(name==='catalog_summary')return summary;
  assert.fail('Unexpected command '+name);
 }};
 deps['@tauri-apps/api/event']={listen:async()=>()=>{released++;}};
 const Panel=await component(file,deps),{host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit:(name:string)=>{if(name==='ready')ready++;}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(predicate:(value:any)=>boolean,value:any=vnode):any{if(value&&predicate(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(predicate,child);if(found)return found;}}
 const panel=(name:string)=>node(value=>value.type?.name===name),byId=(id:string)=>node(value=>value.props?.id===id);
 const settle=async()=>{for(let i=0;i<20;i++)await nextTick();};let mounted=false;
 try{
  app.mount(host);mounted=true;await settle();
  assert.equal(reads,1);assert.equal(ready,0);assert.equal(released,1);
  assert.equal(panel('./ConsolePanel.vue').props.loading,true);
  assert.equal(panel('./CatalogList.vue').props.loading,true);
  assert.ok(!String(byId('productBanner').props.class).split(' ').includes('show'));
  assert.match(byId('toast').children,/database unavailable/);
  t.mock.timers.tick(2000);await settle();
  assert.equal(reads,2);assert.equal(ready,1);assert.equal(panel('./ConsolePanel.vue').props.loading,false);
  assert.deepEqual(panel('./ConsolePanel.vue').props.summary,summary);
  app.unmount();mounted=false;t.mock.timers.tick(6000);await settle();assert.equal(reads,2);assert.equal(released,3);
 }finally{if(mounted)app.unmount();}
});

test('application settings subscribe before reading and refresh migrated values without saving',async()=>{
 const callbacks=new Map<string,(event:any)=>void>(),released:string[]=[],order:string[]=[];
 let reads=0,vnode:any;
 let settings={revision:0,close_action:'quit',launch_at_login:false,start_hidden:false};
 const Panel=await component(new URL('../src/LifecyclePanel.vue',import.meta.url),{
  './lifecycle-settings':{LifecycleSettings},'./LanguagePicker.vue':{default:{render:()=>null}},
  '@tauri-apps/api/core':{invoke:async(name:string)=>{assert.equal(name,'lifecycle_status');reads++;order.push('read');return {settings:{...settings},native_autostart:settings.launch_at_login,closing:false,error:null};}},
  '@tauri-apps/api/event':{listen:async(name:string,callback:(event:any)=>void)=>{order.push(name);callbacks.set(name,callback);return ()=>{released.push(name);callbacks.delete(name);};}},
 });
 const {host,render}=renderer();const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(id:string,value:any=vnode):any {if(value?.props?.id===id)return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(id,child);if(found)return found;}}
 const settle=async()=>{for(let i=0;i<16;i++)await nextTick();};let mounted=false;
 try{
  app.mount(host);mounted=true;await settle();assert.ok(order.indexOf('configuration-changed')<order.indexOf('read'));assert.equal(reads,1);assert.equal(node('silentStart').dirs[0].value,false);
  const changed=callbacks.get('configuration-changed')!;
  settings={...settings,revision:1,launch_at_login:true,start_hidden:true};changed({payload:{revision:1}});await settle();
  assert.equal(reads,2);assert.equal(node('autoStart').dirs[0].value,true);assert.equal(node('silentStart').dirs[0].value,true);assert.equal(node('silentStart').props.disabled,false);
  app.unmount();mounted=false;assert.deepEqual(released.sort(),['configuration-changed','lifecycle-closing','lifecycle-error']);changed({payload:{revision:2}});await settle();assert.equal(reads,2);
 }finally{if(mounted)app.unmount();}
});

test('the migration configuration event refreshes the active language and releases application listeners',async()=>{
 const callbacks=new Map<string,()=>void>(),released:string[]=[],translated:string[]=[];
 let instance!:Languages,locale:'zh-CN'|'en-US'='zh-CN',reads=0,restores=0,disposed=0;
 class ObservedLanguages extends Languages {constructor(...args:ConstructorParameters<typeof Languages>){super(...args);instance=this;}}
 const App=await component(new URL('../src/App.vue',import.meta.url),{
  './LibraryPanel.vue':{default:{render:()=>null}},'./styles/baseline.css':{},'./styles/product.css':{},
  './baseline-language':{Languages:ObservedLanguages,languageKey,translateDocument:()=>({refresh:()=>translated.push(instance.snapshot.locale),dispose:()=>{disposed++;}})},
  '@tauri-apps/api/event':{listen:async(name:string,callback:()=>void)=>{callbacks.set(name,callback);return ()=>{released.push(name);callbacks.delete(name);};}},
  '@tauri-apps/api/core':{invoke:async(name:string)=>{
   if(name==='restore_language_packs'){restores++;return [];}
   assert.equal(name,'language_status');reads++;return {...instance.snapshot,locale};
  }},
 });
 const documentBefore=Object.getOwnPropertyDescriptor(globalThis,'document');Object.defineProperty(globalThis,'document',{value:{},configurable:true});
 const {host,render}=renderer();const app=render.createApp(App);let mounted=false;
 const settle=async()=>{for(let i=0;i<20;i++)await nextTick();};
 try{app.mount(host);mounted=true;await settle();assert.equal(instance.snapshot.locale,'zh-CN');const changed=callbacks.get('configuration-changed')!;
  locale='en-US';changed();await settle();assert.equal(instance.snapshot.locale,'en-US');assert.equal(instance.text('设置'),'Settings');assert.ok(translated.includes('en-US'));assert.equal(restores,2);
  const priorReads=reads;app.unmount();mounted=false;changed();await settle();assert.equal(reads,priorReads);assert.equal(disposed,1);assert.deepEqual(released.sort(),['configuration-changed','language-state-changed']);
 }finally{if(mounted)app.unmount();if(documentBefore)Object.defineProperty(globalThis,'document',documentBefore);else delete (globalThis as any).document;}
});
