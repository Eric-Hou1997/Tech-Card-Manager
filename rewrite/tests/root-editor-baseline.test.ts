import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {runInNewContext} from 'node:vm';
import {baseParse} from '@vue/compiler-dom';
import {createSSRApp,reactive,nextTick} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {component,renderer} from './support/mount-vue.ts';
import {withConfirmation} from '../src/confirmation.ts';
const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const clone=<T,>(value:T):T=>JSON.parse(JSON.stringify(value));
const folder=(id='movies',enabled=true)=>({id,path:'/media/'+id,name:id,kind:'movies',source:'manual',enabled});
const settle=async()=>{for(let n=0;n<15;n++)await nextTick();};
function normalize(html:string):unknown {
 function walk(node:any):unknown{
  if(node.type===3)return null;if(node.type===2)return node.content.trim()||null;if(node.type===0)return node.children.map(walk).filter(Boolean);
  return [node.tag,Object.fromEntries(node.props.filter((p:any)=>p.type===6&&!(p.name==='value'&&!p.value?.content)).map((p:any)=>[p.name,p.name==='class'?(p.value?.content||'').split(/\s+/).filter(Boolean).sort().join(' '):p.name==='style'?(p.value?.content||'').replace(/;$/,''):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),node.children.map(walk).filter(Boolean)];
 }return walk(baseParse(html,{isVoidTag:tag=>['input','br','img','hr'].includes(tag)}));
}
function original(rows:any[],online:Record<string,boolean>){
 const host={innerHTML:''};
 const declarations=source.split('\n').filter(line=>line.startsWith('function esc(')||line.startsWith('function rootKey(')||line.startsWith('function renderRootEditor(')).join('\n');
 runInNewContext(declarations+'\nrenderRootEditor();',{rootRows:rows,lastStatus:{libraries:rows.filter(row=>row.id in online).map(row=>({path:row.path,online:online[row.id]}))},$:()=>host});
 return host.innerHTML;
}
async function mount(invoke:(name:string,args:any)=>Promise<any>){
 const Panel=await component(new URL('../src/RootSettings.vue',import.meta.url),{'./confirmation':{withConfirmation},'@tauri-apps/api/core':{invoke}});
 const props=reactive({configuration:{revision:0,locale:'zh-CN',roots:[]},active:true,blocked:false,afterSave:async()=>{}}),events:any[]=[];
 let vnode:any;const {host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup(props,{expose(){},emit:(...args:any[])=>events.push(args)}),cache:unknown[]=[];return ()=>{vnode=draw({},cache);return null;};}});
 function find(match:(node:any)=>boolean,value:any=vnode):any{if(match(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=find(match,child);if(found)return found;}}
 const node=(id:string)=>find(node=>node?.props?.id===id);
 app.mount(host);
 return {props,events,app,node,find,html:async()=>renderToString(createSSRApp({render:()=>vnode}))};
}
test('directory editor reproduces original initial, empty, online, offline and disabled row DOM',async()=>{
 let ready!:(value:any)=>void;
 const model=await mount(async()=>new Promise(resolve=>ready=resolve));
 try{
  const markup=source.split('\n').find(line=>line.includes('<h3>设置媒体目录</h3>'))!.split('<h3>设置媒体目录</h3>')[1].replace(/<\/div>$/,'');
  assert.deepEqual(normalize(await model.html()),normalize(markup));
  ready({settings:{revision:0,folders:[]},online:{}});await settle();
  assert.deepEqual(normalize(await renderToString(createSSRApp({render:()=>model.node('rootEditor')}))),normalize('<div id="rootEditor" class="rootEditor">'+original([],{})+'</div>'));
 }finally{model.app.unmount();}
 const rows=[folder('online'),folder('offline'),folder('waiting'),folder('disabled',false),{...folder('mixed'),kind:'mixed',source:'auto'},{...folder('tv'),kind:'tv',source:'auto'},{...folder('auto'),kind:'auto'}, {...folder('escaped'),name:'<电影> & "TV"',path:'/media/<&"'}];
 const online={online:true,offline:false,disabled:true};
 const loaded=await mount(async()=>({settings:{revision:0,folders:rows},online}));
 try{await settle();assert.deepEqual(normalize(await renderToString(createSSRApp({render:()=>loaded.node('rootEditor')}))),normalize('<div id="rootEditor" class="rootEditor">'+original(rows,online)+'</div>'));
  loaded.props.blocked=true;await settle();assert.equal(loaded.find(n=>n?.props?.['data-action']==='discover-roots').props.disabled,true);assert.equal(loaded.node('manualRootPath').props.disabled,undefined);assert.equal(loaded.node('saveLibraryRoots').props.disabled,undefined);
 }finally{loaded.app.unmount();}
});
test('folder picker and discovery retain typed input; manual add clears it even for a duplicate',async()=>{
 const model=await mount(async(name)=>{
  if(name==='folder_settings')return {settings:{revision:0,folders:[]},online:{}};
  if(name==='choose_library_root')return '/media/chosen';
  if(name==='emby_libraries')return [{local_path:'/media/discovered',name:'Discovered',spaces:['tv']}];
  assert.fail(name);
 });
 try{
  await settle();model.node('manualRootPath').props['onUpdate:modelValue']('/media/typed');await settle();
  await model.node('chooseLibraryRoot').props.onClick();await settle();assert.equal(model.node('manualRootPath').dirs[0].value,'/media/typed');
  await model.find(n=>n?.props?.['data-action']==='discover-roots').props.onClick();await settle();assert.equal(model.node('manualRootPath').dirs[0].value,'/media/typed');
  model.node('manualRootPath').props['onUpdate:modelValue']('/media/chosen/');await settle();model.node('addLibraryRoot').props.onClick();await settle();assert.equal(model.node('manualRootPath').dirs[0].value,'');assert.deepEqual(model.events.at(-1),['notify','这个目录已经在列表中']);
 }finally{model.app.unmount();}
});
test('directory comparison retains Windows casing rules without merging distinct Unix paths',async()=>{
 const previous=Object.getOwnPropertyDescriptor(navigator,'platform');
 try{
  for(const platform of ['Win32','MacIntel','Linux aarch64']){
   Object.defineProperty(navigator,'platform',{configurable:true,value:platform});
   const model=await mount(async()=>({settings:{revision:0,folders:[]},online:{}}));
   try{
    await settle();
    for(const path of ['/media/Movies','/media/movies']){
     model.node('manualRootPath').props['onUpdate:modelValue'](path);await settle();
     model.node('addLibraryRoot').props.onClick();await settle();
    }
    assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/Movies'));
    assert.equal(Boolean(model.find(n=>n?.props?.['data-scan-root']==='/media/movies')),platform!=='Win32');
    if(platform==='Win32')assert.deepEqual(model.events.at(-1),['notify','这个目录已经在列表中']);
   }finally{model.app.unmount();}
  }
 }finally{if(previous)Object.defineProperty(navigator,'platform',previous);else delete (navigator as any).platform;}
});
test('configuration rereads preserve unsaved folders and save receipts preserve edits made in flight',async()=>{
 let settings={revision:0,folders:[folder()]},receipt!:(value:any)=>void;const saves:any[]=[];
 const model=await mount(async(name,args)=>{
  if(name==='folder_settings')return {settings:clone(settings),online:{}};
  if(name==='confirm_product_action')return true;
  if(name==='save_media_folders'){saves.push(clone(args));return new Promise(resolve=>receipt=resolve);}
  assert.fail(name);
 });
 const add=async(path:string)=>{model.node('manualRootPath').props['onUpdate:modelValue'](path);await settle();model.node('addLibraryRoot').props.onClick();await settle();};
 try{
  await settle();await add('/media/draft');settings.revision=1;model.props.configuration.revision=1;await settle();
  assert.equal(model.events.filter(row=>row[0]==='notify').length,0);assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/draft'));
  settings={revision:2,folders:[folder('external')]};model.props.configuration.revision=2;await settle();model.props.active=false;await settle();model.props.active=true;await settle();assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/draft'));assert.equal(model.find(n=>n?.props?.['data-scan-root']==='/media/external'),undefined);assert.equal(model.events.length,0);
  void model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(saves[0].settings.revision,2);
  await add('/media/later');settings={revision:3,folders:saves[0].settings.folders};receipt({settings:clone(settings),configuration:{...model.props.configuration,revision:3}});await settle();
  assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/later'));
  assert.equal(model.node('rootEditorState').children,'有未保存的修改');
  void model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(saves[1].settings.revision,3);assert.equal(saves[1].settings.folders.length,3);
  model.app.unmount();receipt({settings,configuration:{...model.props.configuration,revision:4}});await settle();
 }finally{model.app.unmount();}
});

test('a lost folder-save reply is resolved before saving the next confirmed draft',async()=>{
 let settings={revision:0,folders:[folder()]},lose=true;
 const saves:any[]=[],receipts=new Map<string,any>(),followups:number[]=[];
 const model=await mount(async(name,args)=>{
  if(name==='folder_settings')return {settings:clone(settings),online:{}};
  if(name==='confirm_product_action')return true;
  if(name==='save_media_folders'){
   saves.push(clone(args));if(receipts.has(args.id))return clone(receipts.get(args.id));
   if(args.settings.revision!==settings.revision)throw {code:'configuration-conflict',message:'stale settings'};
   settings={...clone(args.settings),revision:settings.revision+1};
   const receipt={settings:clone(settings),configuration:{revision:settings.revision,locale:'zh-CN',roots:[]}};receipts.set(args.id,receipt);
   if(lose){lose=false;throw Error('reply lost');}return clone(receipt);
  }
  if(name==='operation_result'){
   if(receipts.has(args.id))return {kind:'folders',result:clone(receipts.get(args.id))};
   throw {code:'operation-not-found',message:'no receipt'};
  }
  assert.fail(name);
 });
 const add=async(path:string)=>{model.node('manualRootPath').props['onUpdate:modelValue'](path);await settle();model.node('addLibraryRoot').props.onClick();await settle();};
 try{
  model.props.afterSave=async(revision:number)=>{followups.push(revision);};await settle();
  await add('/media/first');await model.node('saveLibraryRoots').props.onClick();await settle();
  assert.equal(settings.revision,1);assert.deepEqual(followups,[]);
  await add('/media/second');
  const toggle=model.find(n=>n?.props?.['data-root-toggle']===0);toggle.props['onUpdate:modelValue'](false);toggle.props.onChange();
  model.find(n=>n?.props?.['data-root-remove']===1).props.onClick();await settle();
  await model.node('saveLibraryRoots').props.onClick();await settle();
  assert.equal(saves.length,3);assert.deepEqual(saves[0],saves[1]);assert.notEqual(saves[1].id,saves[2].id);
  assert.equal(saves[2].settings.revision,1);assert.equal(settings.folders.length,2);assert.equal(settings.folders[0].enabled,false);assert.equal(settings.folders[1].path,'/media/second');assert.deepEqual(followups,[2]);
  assert.equal(model.events.filter(row=>row[0]==='changed').length,1);
  assert.equal(model.events.filter(row=>row[0]==='notify'&&row[1]==='媒体目录已保存').length,1);
 }finally{model.app.unmount();}
});

test('a definite rejected folder transaction releases its ID so a corrected directory can be saved',async()=>{
 let settings={revision:0,folders:[folder()]};const saves:any[]=[],reads:string[]=[];
 const model=await mount(async(name,args)=>{
  if(name==='folder_settings')return {settings:clone(settings),online:{}};
  if(name==='confirm_product_action')return true;
  if(name==='operation_result'){reads.push(args.id);throw {code:'operation-not-found',message:'no receipt'};}
  if(name==='save_media_folders'){
   saves.push(clone(args));if(saves.length===1)throw {code:'invalid-folders',message:'invalid directory'};
   settings={...clone(args.settings),revision:1};return {settings:clone(settings),configuration:{revision:1,locale:'zh-CN',roots:[]}};
  }
  assert.fail(name);
 });
 try{
  await settle();model.node('manualRootPath').props['onUpdate:modelValue']('/media/bad');await settle();model.node('addLibraryRoot').props.onClick();await settle();
  await model.node('saveLibraryRoots').props.onClick();await settle();assert.deepEqual(reads,[saves[0].id]);assert.equal(settings.revision,0);
  model.find(n=>n?.props?.['data-root-remove']===1).props.onClick();await settle();
  await model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(saves.length,2);assert.notEqual(saves[0].id,saves[1].id);assert.equal(settings.folders.length,1);
  assert.equal(model.events.filter(row=>row[0]==='changed').length,1);
 }finally{model.app.unmount();}
});

test('a folder revision conflict rereads the revision while keeping the unsaved rows for explicit retry',async()=>{
 let settings={revision:0,folders:[folder()]};const saves:any[]=[];
 const model=await mount(async(name,args)=>{
  if(name==='folder_settings')return {settings:clone(settings),online:{}};
  if(name==='confirm_product_action')return true;
  if(name==='operation_result')throw {code:'operation-not-found',message:'no receipt'};
  if(name==='save_media_folders'){
   saves.push(clone(args));if(args.settings.revision!==settings.revision)throw {code:'configuration-conflict',message:'stale settings'};
   settings={...clone(args.settings),revision:settings.revision+1};return {settings:clone(settings),configuration:{revision:settings.revision,locale:'zh-CN',roots:[]}};
  }
  assert.fail(name);
 });
 try{
  await settle();model.node('manualRootPath').props['onUpdate:modelValue']('/media/draft');await settle();model.node('addLibraryRoot').props.onClick();await settle();
  settings={revision:1,folders:[folder('elsewhere')]};await model.node('saveLibraryRoots').props.onClick();await settle();
  assert.equal(saves.length,1);assert.equal(model.events.filter(row=>row[0]==='changed').length,0);assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/draft'));
  await model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(saves[1].settings.revision,1);assert.equal(settings.folders.length,2);assert.equal(settings.revision,2);
 }finally{model.app.unmount();}
});

test('a stale directory read cannot overwrite a newer saved revision or publish stale discovery',async()=>{
 let settings={revision:0,folders:[folder()]},reads=0,release!:(value:any)=>void;const saves:any[]=[];
 const model=await mount(async(name,args)=>{
  if(name==='folder_settings'){reads++;if(reads===2)return new Promise(resolve=>release=resolve);return {settings:clone(settings),online:{}};}
  if(name==='confirm_product_action')return true;
  if(name==='save_media_folders'){saves.push(clone(args));settings={...clone(args.settings),revision:settings.revision+1};return {settings:clone(settings),configuration:{revision:settings.revision,locale:'zh-CN',roots:[]}};}
  assert.fail(name);
 });
 try{
  await settle();model.props.active=false;await settle();model.props.active=true;await settle();assert.equal(reads,2);
  await model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(settings.revision,1);
  release({settings:{revision:0,folders:[folder()]},online:{},discovered:[{local_path:'/obsolete/discovery',spaces:['tv'],name:'Old'}]});await settle();
  assert.equal(model.find(n=>n?.props?.['data-scan-root']==='/obsolete/discovery'),undefined);
  await model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(saves[1].settings.revision,1);
 }finally{model.app.unmount();}
});

test('cached discovery follows original initialization, path-set merging and offline inclusion rules',async()=>{
 const tv={id:'emby-tv',name:'TV',server_path:'/offline/TV',local_path:null,spaces:['tv']};
 let discovered:any[]=[tv,{...tv,id:'ignored',server_path:'/ignored',spaces:[]}];
 let configured=false;
 const model=await mount(async(name)=>{
  if(name==='folder_settings')return {settings:{revision:0,folders:configured?[folder()]:[]},online:{},roots_configured:configured,discovered};
  if(name==='emby_libraries')return discovered;
  assert.fail(name);
 });
 try{
  await settle();assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/offline/TV'));assert.equal(model.find(n=>n?.props?.['data-scan-root']==='/ignored'),undefined);assert.equal(model.node('rootEditorState').children,'');
  model.find(n=>n?.props?.['data-root-remove']===0).props.onClick();await settle();
  model.props.active=false;await settle();model.props.active=true;await settle();assert.equal(model.find(n=>n?.props?.['data-scan-root']==='/offline/TV'),undefined);
  discovered=[...discovered,{...tv,id:'new',server_path:'/new/TV'}];
  await model.find(n=>n?.props?.['data-action']==='discover-roots').props.onClick();await settle();assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/new/TV'));assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/offline/TV'));assert.equal(model.node('rootEditorState').children,'发现结果已加入，请确认后保存');
 }finally{model.app.unmount();}
 configured=true;
 const restored=await mount(async()=>({settings:{revision:0,folders:[folder()]},online:{},roots_configured:true,discovered}));
 try{await settle();assert.ok(restored.find(n=>n?.props?.['data-scan-root']==='/media/movies'));assert.equal(restored.find(n=>n?.props?.['data-scan-root']==='/offline/TV'),undefined);}finally{restored.app.unmount();}
});

test('discovery owns the original busy state and releases it on failure or unmount',async()=>{
 let finish!:(value:any)=>void,fail!:(value:any)=>void,calls=0;
 const model=await mount(async(name)=>{
  if(name==='folder_settings')return {settings:{revision:0,folders:[]},online:{}};
  if(name==='emby_libraries'){calls++;return new Promise((resolve,reject)=>{finish=resolve;fail=reject;});}
  assert.fail(name);
 });
 try{
  await settle();const click=()=>model.find(n=>n?.props?.['data-action']==='discover-roots').props.onClick();
  void click();void click();await settle();assert.equal(calls,1);assert.deepEqual(model.events,[['busy',true]]);
  assert.equal(model.node('saveLibraryRoots').props.disabled,undefined);
  fail({message:'任务已取消'});await settle();assert.deepEqual(model.events.slice(-2),[['notify','任务已取消'],['busy',false]]);
  void click();await settle();assert.equal(calls,2);model.app.unmount();const count=model.events.length;assert.deepEqual(model.events.at(-1),['busy',false]);
  finish([{server_path:'/late',local_path:'/late',name:'Late',spaces:['movie']}]);await settle();assert.equal(model.events.length,count);
 }finally{model.app.unmount();}
});

test('a damaged discovery snapshot reports its path without blocking valid folder settings',async()=>{
 let confirms=0,saves=0;
 const settings={revision:0,folders:[folder()]};
 const model=await mount(async(name)=>{
  if(name==='folder_settings')return {settings,online:{},discovered:[],discovery_error:{message:'旧发现数据损坏',path:'/emby/custom-tech-specs/manager-root-discovery.json'}};
  if(name==='confirm_product_action'){confirms++;return true;}
  if(name==='save_media_folders'){saves++;return {settings,configuration:{revision:1,locale:'zh-CN',roots:[]}};}
  assert.fail(name);
 });
 try{await settle();assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/movies'));assert.deepEqual(model.events[0],['notify','旧发现数据损坏 · /emby/custom-tech-specs/manager-root-discovery.json']);await model.node('saveLibraryRoots').props.onClick();await settle();assert.equal(confirms,1);assert.equal(saves,1);}finally{model.app.unmount();}
});

test('migration notifications during the first directory read load the committed rows once',async()=>{
 let finish!:(value:any)=>void,reads=0;
 const model=await mount(async name=>{
  assert.equal(name,'folder_settings');reads++;
  if(reads===1)return new Promise(resolve=>finish=resolve);
  return {settings:{revision:2,folders:[folder('migrated')]},online:{migrated:false},roots_configured:true};
 });
 try{
  model.props.configuration.revision=1;await settle();model.props.configuration.revision=2;await settle();assert.equal(reads,1);
  finish({settings:{revision:0,folders:[]},online:{},roots_configured:false});await settle();
  assert.equal(reads,2);assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/migrated'));
  assert.equal(model.find(n=>n?.props?.['data-scan-root']==='/media/migrated').props.disabled,true);
  assert.deepEqual(model.events,[]);
 }finally{model.app.unmount();}
});

test('closing the directory component cancels a queued migration reread',async()=>{
 let finish!:(value:any)=>void,reads=0;
 const model=await mount(async()=>{reads++;return new Promise(resolve=>finish=resolve);});
 model.props.configuration.revision=1;await settle();model.app.unmount();const events=model.events.length;
 finish({settings:{revision:0,folders:[]},online:{}});await settle();
 assert.equal(reads,1);assert.equal(model.events.length,events);
});

test('open directory settings refresh online status while preserving drafts and stop polling when hidden',async t=>{
 t.mock.timers.enable({apis:['setTimeout']});
 let reads=0,online=true,fail=false;
 const saved=folder();
 const model=await mount(async name=>{
  assert.equal(name,'folder_settings');reads++;
  if(fail)throw {message:'directory status unavailable'};
  return {settings:{revision:0,folders:[saved]},online:{movies:online}};
 });
 const scan=()=>model.find(n=>n?.props?.['data-scan-root']===saved.path);
 try{
  await settle();assert.equal(scan().props.disabled,false);
  model.node('manualRootPath').props['onUpdate:modelValue']('/media/unsaved');await settle();
  model.node('addLibraryRoot').props.onClick();await settle();
  online=false;t.mock.timers.tick(2000);await settle();
  assert.equal(reads,2);assert.equal(scan().props.disabled,true);
  assert.ok(model.find(n=>n?.props?.['data-scan-root']==='/media/unsaved'));
  fail=true;t.mock.timers.tick(2000);await settle();assert.equal(reads,3);
  assert.equal(scan().props.disabled,true);
  fail=false;online=true;t.mock.timers.tick(2000);await settle();
  assert.equal(reads,4);assert.equal(scan().props.disabled,false);
  model.props.active=false;await settle();t.mock.timers.tick(10000);await settle();assert.equal(reads,4);
  model.props.active=true;await settle();assert.equal(reads,5);
  model.app.unmount();t.mock.timers.tick(10000);await settle();assert.equal(reads,5);
 }finally{model.app.unmount();}
});
