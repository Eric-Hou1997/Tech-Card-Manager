import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {baseParse} from '@vue/compiler-dom';
import {createSSRApp,h,reactive,nextTick,ref} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {component,renderer,find} from './support/mount-vue.ts';
import {withConfirmation} from '../src/confirmation.ts';
import * as Vue from 'vue';
import ts from 'typescript';
import * as serviceControl from '../src/service-control.ts';
const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const Dialog=await component(new URL('../src/SettingsDialog.vue',import.meta.url));
const Setup=await component(new URL('../src/SetupFlow.vue',import.meta.url));
function normalize(html:string):unknown {
 function walk(node:any):unknown{if(node.type===3)return null;if(node.type===2)return node.content.trim()||null;if(node.type===0)return node.children.map(walk).filter(Boolean);return [node.tag,Object.fromEntries(node.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='class'?(p.value?.content||'').split(/\s+/).filter(Boolean).sort().join(' '):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),node.children.map(walk).filter(Boolean)];}return walk(baseParse(html));
}
test('settings shell retains the original CSS anchors, dialog roles and fixed-header markup',async()=>{
 const start=source.indexOf('<div class="modalBackdrop" id="settingsBackdrop"');
 const end=source.indexOf('<div class="settingGroup">',start);
 for(const visible of [false,true]){
  const expected=source.slice(start,end).replace('class="modalBackdrop"',visible?'class="modalBackdrop show"':'class="modalBackdrop"')+'<span>draft</span></div></div></div>';
  const context:any={};await renderToString(createSSRApp({render:()=>h(Dialog,{visible},()=>h('span','draft'))}),context);
  assert.deepEqual(normalize(context.teleports.body),normalize(expected));
 }
});
test('closing and reopening settings preserves content, with only the original close button',async()=>{
 const state=reactive({visible:true});let closes=0;
 const {body,host,render}=renderer();const app=render.createApp({render:()=>h(Dialog,{visible:state.visible,onClose:()=>{closes++;state.visible=false;}},()=>h('input',{id:'draft',value:'unsaved'}))});
 app.mount(host);const backdrop=find(body,'settingsBackdrop')!,draft=find(body,'draft')!;
 assert.equal(backdrop.props.onClick,undefined);assert.equal(backdrop.props.onKeydown,undefined);
 const close=backdrop.children[0].children[0].children.find(child=>child.type==='button')!;
 assert.equal(close.props.disabled,undefined);close.props.onClick();await nextTick();
 assert.equal(closes,1);assert.equal(find(body,'draft'),draft);assert.ok(!String(backdrop.props.class).split(' ').includes('show'));
 state.visible=true;await nextTick();assert.equal(find(body,'draft'),draft);assert.ok(String(backdrop.props.class).includes('show'));app.unmount();
 assert.equal(find(body,'settingsBackdrop'),undefined);
});
test('setup steps retain original initial text and update each completion independently',async()=>{
 const initial=source.match(/insertAdjacentHTML\('afterbegin','([^']+)'\)/)![1];
 const html=await renderToString(createSSRApp({render:()=>h(Setup,{initialized:false,embyDetected:false,rootsConfigured:false,webConfigured:false})}));
 assert.deepEqual(normalize(html),normalize(initial));
 const state=reactive({initialized:true,embyDetected:false,rootsConfigured:false,webConfigured:false});const {host,render}=renderer();const app=render.createApp({render:()=>h(Setup,state)});app.mount(host);
 for(const value of [true,false]){state.embyDetected=value;await nextTick();assert.equal(find(host,'setupEmby')!.text,value?'已完成':'等待检测');assert.equal(find(host,'setupStepEmby')!.props['data-complete'],String(value));assert.equal(find(host,'setupRoots')!.text,'请选择目录');}
 state.rootsConfigured=true;state.webConfigured=true;await nextTick();assert.equal(find(host,'setupRoots')!.text,'已完成');assert.equal(find(host,'setupWeb')!.text,'已完成');app.unmount();
});
test('confirmation must resolve positively before any product mutation',async()=>{
 let resolve!:(value:boolean)=>void,calls=0;
 const pending=withConfirmation(async<T>(command,args)=>{assert.equal(command,'confirm_product_action');assert.deepEqual(args,{action:'save-roots'});return await new Promise<boolean>(r=>resolve=r) as T;},'save-roots',async()=>{calls++;});
 assert.equal(calls,0);resolve(false);assert.equal(await pending,false);assert.equal(calls,0);
 assert.equal(await withConfirmation(async<T>()=>true as T,'repair-web',async()=>{calls++;}),true);assert.equal(calls,1);
 await assert.rejects(withConfirmation(async()=>{throw Error('dialog failed');},'rebuild-index',async()=>{calls++;}));assert.equal(calls,1);
});

test('incremental interval retries an ambiguous save with the same receipt and refreshes when settings reopen',async()=>{
 let revision=0,intervalSeconds=60,first=true,receiptUnavailable=true,vnode:any;const saves:any[]=[],events:any[]=[],props=reactive({active:true});
 const Panel=await component(new URL('../src/IncrementalPanel.vue',import.meta.url),{'@tauri-apps/api/core':{invoke:async(name:string,args:any)=>{
  if(name==='incremental_settings')return {revision,interval_seconds:intervalSeconds};
  if(name==='save_incremental_settings'){
   saves.push(structuredClone(args));
   if(first){first=false;revision=1;intervalSeconds=args.value.interval_seconds;throw {message:'reply lost'};}
   return {revision,interval_seconds:intervalSeconds};
  }
  if(name==='operation_result'){
   if(receiptUnavailable){receiptUnavailable=false;throw {message:'receipt unavailable'};}
   return {kind:'incremental-settings',result:{revision,interval_seconds:intervalSeconds}};
  }
  assert.fail(name);
 }}});
 const {host,render}=renderer();const app=render.createApp({setup(){const draw=Panel.setup(props,{attrs:{},slots:{default:()=>[]},expose(){},emit:(_name:string,message:string)=>events.push(message)}),cache:unknown[]=[];return ()=>{vnode=draw({$slots:{default:()=>[]}},cache);return null;};}});app.mount(host);await nextTick();await nextTick();
 function node(id:string,value:any=vnode):any{if(value?.props?.id===id)return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(id,child);if(found)return found;}}
 const select=()=>node('interval');
 select().props['onUpdate:modelValue'](300);await nextTick();await select().props.onChange();await nextTick();
 assert.equal(select().dirs[0].value,60);assert.deepEqual(events,['reply lost']);assert.equal(saves.length,1);
 select().props['onUpdate:modelValue'](300);await nextTick();await select().props.onChange();await nextTick();
 assert.equal(select().dirs[0].value,300);assert.deepEqual(events,['reply lost','增量检查周期已保存']);assert.equal(saves.length,2);assert.deepEqual(saves[1],saves[0]);
 props.active=false;await nextTick();revision=2;intervalSeconds=900;props.active=true;await nextTick();await nextTick();assert.equal(select().dirs[0].value,900);
 app.unmount();
});

test('catalog controls stay in the current session and never add rewrite-only UI persistence',async()=>{
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),code=await readFile(file,'utf8');
 const dependencies:Record<string,unknown>={};
 for(const path of code.matchAll(/from '([^']+\.(?:vue|png))'/g))dependencies[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{render:()=>null}};
 const scans:any[]=[];let ready=false,vnode:any;
 const callbacks=new Map<string,(event:any)=>void>(),released:string[]=[];
 const config={revision:0,locale:'zh-CN',roots:[{id:'movie',path:'/media/Movie',space:'movie'},{id:'tv',path:'/media/TV',space:'tv'}]};
 const empty=()=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
 dependencies['@tauri-apps/api/core']={invoke:async(name:string,args:any)=>{
  if(name==='configuration'){
   assert.ok(callbacks.has('configuration-changed'),'subscribe before reading the startup snapshot');
   callbacks.get('configuration-changed')!({payload:{...config,revision:3}});
   return config; // An older in-flight reply must not undo the migration event.
  }
  if(name==='manager_catalog'||name==='task_history')return [];
  if(name==='catalog_summary')return {total:0,movie:0,tv:0,errors:0,displayable:0,web_eligible:0,episodes_excluded:0,generated_at:null,roots_configured:true};
  if(name==='ui_state'||name==='save_ui_state')assert.fail('v4.1.0 does not persist catalog presentation state');
  if(name==='scan_library'){scans.push(args);return [];}
  assert.fail('Unexpected IPC '+name);
 }};
 dependencies['@tauri-apps/api/event']={listen:async(name:string,callback:(event:any)=>void)=>{callbacks.set(name,callback);return ()=>{released.push(name);callbacks.delete(name);};}};
 dependencies['./console']=await import('../src/console.ts');
 dependencies['./catalog']=await import('../src/catalog.ts');
 dependencies['./window-state']=await import('../src/window-state.ts');
 dependencies['./baseline-layout']={installBaselineLayout:()=>()=>{}};
 dependencies['./confirmation']={withConfirmation};dependencies['./maintenance']={};dependencies['./legacy-migration']=await import('../src/legacy-migration.ts');
 dependencies['./useEmby']={embyKey:Symbol(),useEmby:()=>({legacyComponents:ref(null),legacyPlan:ref(null),target:ref(null),service:ref({phase:'stopped'}),busy:ref(false),error:ref(''),ready:ref(true),pending:ref(null),observedAt:ref('')})};
 const Panel=await component(file,dependencies);
 const {host,render}=renderer();
 // Execute the real component state, watchers and rendered VNodes. No native
 // window, filesystem write, or browser pixel evidence is asserted here.
 const app=render.createApp({setup(){const draw=Panel.setup({}, {expose(){},emit(name:string){if(name==='ready')ready=true;}}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(id:string,value:any=vnode):any{if(value?.props?.id===id)return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(id,child);if(found)return found;}}
 const settle=async()=>{for(let n=0;n<15;n++)await nextTick();};
 app.mount(host);let mounted=true;
 try{
  await settle();assert.ok(ready);
  await node('refreshLibrary').props.onClick();assert.equal(scans[0].space,'movie');assert.equal(scans[0].revision,3);assert.equal(typeof scans[0].id,'string');assert.deepEqual(Object.keys(scans[0]).sort(),['id','revision','space']);
  callbacks.get('configuration-changed')!({payload:{...config,revision:1}});await settle();
  node('catalogTvTab').props.onClick();await settle();
  await node('refreshLibrary').props.onClick();assert.equal(scans[1].space,'tv');assert.equal(scans[1].revision,3);
  node('catalogMovieTab').props.onClick();await settle();node('catalogTvTab').props.onClick();await settle();
  app.unmount();mounted=false;assert.deepEqual(released.sort(),['configuration-changed','task-changed']);
 }finally{if(mounted)app.unmount();}
});

test('folder save waits for confirmation and service follow-up, preserving saved rows when the service fails',async()=>{
 let saves=0,confirms=0,followups=0,vnode:any,answer!:(yes:boolean)=>void,finish!:(e:Error)=>void;const scans:any[]=[];
 const settings={revision:0,folders:[{id:'movies',path:'/media/Movies',name:'Movies',kind:'movies',source:'manual',enabled:true}]};
 const config={revision:0,locale:'zh-CN',roots:[{id:'movies',path:'/media/Movies',space:'movie'}]};
 const events:any[]=[];
 const Panel=await component(new URL('../src/RootSettings.vue',import.meta.url),{'./confirmation':{withConfirmation},'@tauri-apps/api/core':{invoke:async(name:string,args:any)=>{
  if(name==='folder_settings')return {settings,online:{movies:true}};
  if(name==='confirm_product_action'){confirms++;return await new Promise(resolve=>answer=resolve);}
  if(name==='save_media_folders'){saves++;if(saves>1)throw Error('storage unavailable');settings.revision=1;return {settings,configuration:{...config,revision:1}};}
  if(name==='scan_media_folder'){scans.push(args);return [];}
  assert.fail(name);
 }}});
 const props=reactive({configuration:config,active:true,afterSave:async(revision:number)=>{followups++;assert.equal(revision,1);await new Promise((_,reject)=>finish=reject);}});
 const {host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup(props,{expose(){},emit:(...args:any[])=>events.push(args)}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function node(id:string,value:any=vnode):any{if(value?.props?.id===id)return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=node(id,child);if(found)return found;}}
 const settle=async()=>{for(let n=0;n<10;n++)await nextTick();};
 app.mount(host);let mounted=true;
 try{
  await settle();const click=()=>node('saveLibraryRoots').props.onClick();
  function scanButton(value:any=vnode):any{if(value?.props?.['data-scan-root'])return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=scanButton(child);if(found)return found;}}
  void scanButton().props.onClick();void scanButton().props.onClick();await settle();assert.equal(scans.length,1);assert.equal(scans[0].folderId,'movies');assert.equal(scans[0].revision,0);assert.deepEqual(Object.keys(scans[0]).sort(),['folderId','id','revision']);
  void click();void click();assert.equal(confirms,1);assert.equal(saves,0);answer(false);await settle();assert.equal(saves,0);
  void click();answer(true);await settle();assert.equal(saves,1);assert.equal(followups,1);assert.equal(node('rootEditorState').children,'目录设置已保存');
  void click();await settle();assert.equal(confirms,2);assert.equal(saves,1);
  finish(Error('已有任务正在运行'));await settle();assert.equal(node('rootEditorState').children,'目录设置已保存');assert.deepEqual(events.at(-1),['notify','已有任务正在运行']);assert.equal(node('saveLibraryRoots').props.disabled,undefined);
  void click();answer(true);await settle();assert.equal(saves,2);assert.equal(followups,1);assert.deepEqual(events.at(-1),['notify','storage unavailable']);
  void click();app.unmount();mounted=false;answer(true);await settle();assert.equal(saves,2);
 }finally{if(mounted)app.unmount();}
});

test('startup discovery errors remain visible without a saved web path and unmount prevents follow-up reads',async()=>{
 const source=await readFile(new URL('../src/useEmby.ts',import.meta.url),'utf8');
 const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 function load(invoke:(name:string)=>Promise<unknown>){const exports:any={};new Function('require','exports',code)((name:string)=>name==='vue'?Vue:name==='@tauri-apps/api/core'?{invoke}:name==='./service-control'?serviceControl:assert.fail(name),exports);return exports.useEmby;}
 const {host,render}=renderer();let model:any;
 const useEmby=load(async(name)=>{if(name==='emby_legacy_components')return {fingerprint:'empty',items:[],errors:[]};if(name==='emby_legacy_operation')return null;if(name==='emby_authorization_available')return false;if(name==='emby_status'||name==='emby_operation')return null;if(name==='emby_environment')return {restore_error:{message:'检测到多个 Emby 安装目录'}};if(name==='emby_service_status')return {phase:'stopped'};if(name==='incremental_status')return {phase:'stopped',error:null};assert.fail(name);});
 const app=render.createApp({setup(){model=useEmby();return ()=>null;}});app.mount(host);for(let n=0;n<20;n++)await nextTick();assert.equal(model.error.value,'检测到多个 Emby 安装目录');assert.equal(model.environment.value,null);assert.ok(model.ready.value);app.unmount();
 const calls:string[]=[];let answer!:(value:boolean)=>void;
 const late=load(async(name)=>{calls.push(name);return await new Promise(resolve=>answer=resolve);});
 const mounted=render.createApp({setup(){model=late();return ()=>null;}});mounted.mount(host);mounted.unmount();answer(false);for(let n=0;n<10;n++)await nextTick();await model.refresh();assert.deepEqual(calls,['emby_authorization_available']);assert.equal(model.ready.value,false);
});
