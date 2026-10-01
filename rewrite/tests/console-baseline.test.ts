import test,{after} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {createServer} from 'vite';
import vue from '@vitejs/plugin-vue';
import {createSSRApp,h} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {baseParse} from '@vue/compiler-dom';
import {servicePresentation,serviceStartedAt,servicePhase,type ServicePhase} from '../src/console.ts';
import {changeService,refreshAfterRootsSaved,ServiceControlFailure} from '../src/service-control.ts';
const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const html=source.slice(source.indexOf('<section class="card consolePanel">'),source.indexOf('<section class="section card catalogPanel">'));
const server=await createServer({configFile:false,root:new URL('..',import.meta.url).pathname,plugins:[vue()],server:{middlewareMode:true,watch:null,hmr:false,ws:false},optimizeDeps:{noDiscovery:true,entries:[]},appType:'custom'});
after(()=>server.close());
const Panel=(await server.ssrLoadModule('/src/ConsolePanel.vue')).default;
function normalize(node:any):unknown {
 if(node.type===3)return null;if(node.type===2)return node.content.trim()||null;
 if(node.type===0)return node.children.map(normalize).filter(Boolean);
 return [node.tag,Object.fromEntries(node.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='class'?(p.value?.content||'').split(/\s+/).filter(Boolean).sort().join(' '):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),node.children.map(normalize).filter(Boolean)];
}
function original(state:string,errors:number) {
 const tree:any=baseParse(html),ids=new Map<string,any>();
 function index(node:any){const id=node.props?.find((p:any)=>p.name==='id')?.value?.content;if(id)ids.set('#'+id,node);node.children?.forEach(index);}index(tree);
 const handles=new Map<string,any>();
 function $(id:string):any {
  if(handles.has(id))return handles.get(id);
  const node=ids.get(id)||{props:[],children:[]};
  const attr=(key:string,value?:string)=>{let p=node.props.find((p:any)=>p.name===key);if(value===undefined)return p?.value?.content||'';if(!p){p={type:6,name:key};node.props.push(p);}p.value={content:value};return value;};
  const remove=(key:string)=>{node.props=node.props.filter((p:any)=>p.name!==key);};
  const classes=(add:string[],del:string[])=>attr('class',[...new Set(attr('class').split(/\s+/).filter((s:string)=>s&&!del.includes(s)).concat(add))].join(' '));
  const element={get textContent(){return node.children.map((n:any)=>n.content||'').join('');},set textContent(value:string){node.children=[{type:2,content:String(value)}];},get className(){return attr('class');},set className(value:string){attr('class',value);},set disabled(value:boolean){if(value)attr('disabled','');else remove('disabled');},classList:{add:(...v:string[])=>classes(v,[]),remove:(...v:string[])=>classes([],v),toggle:(v:string,on:boolean)=>on?classes([v],[]):classes([],[v])},dataset:new Proxy({}, {set(_,key,value){attr('data-'+String(key),String(value));return true;}}),querySelector:()=>({textContent:''})};
  handles.set(id,element);return element;
 }
 const context:any={$,uiLanguage:'zh-CN',renderLegacy(){},rootsInitialized:true,lastDiscoveryKey:'',renderRootEditor(){},rootKey:(s:string)=>s};vm.createContext(context);
 for(const prefix of ['function esc(','function setPill(','function formatServiceStartedAt(','function renderService(','function renderStatus(']){const line=source.split('\n').find(l=>l.startsWith(prefix));assert.ok(line);vm.runInContext(line,context);}
 context.renderStatus({app_version:'5.0.0',roots_configured:true,service:{state,last_started_at:'2026-09-12T08:00:00Z'},indexed_titles:2,nfo_total:8,xml_errors:errors,web_patch:true,extra:{web_healthy:true,card_runtime_valid:state==='running',web_js_version:'5.0.0',generated_at:'2026-09-12T09:00:00Z',scan_stats:{web_eligible_specs_found:3,episode_specs_excluded_from_web:2}}});
 return {tree,updatedAt:$('#lastUpdated').textContent.replace('状态更新 ','')};
}
test('Vue console matches original DOM and wording in every original service state',async()=>{
 for(const phase of ['running','stopped','starting','stopping','migrating','exiting','stop-error','error','legacy-blocked'] as ServicePhase[])for(const errors of [0,2]){
  const expected=original(phase,errors);
  const actual=await renderToString(createSSRApp({render:()=>h(Panel,{phase,lastStartedAt:'2026-09-12T08:00:00Z',locale:'zh-CN',loading:false,updatedAt:expected.updatedAt,summary:{total:8,movie:4,tv:4,errors,displayable:2,web_eligible:3,episodes_excluded:2,generated_at:'2026-09-12T09:00:00Z'},webVersion:'5.0.0',webHealthy:true,runtimeValid:phase==='running',webInstalled:true,busy:false,platform:'Windows'})}));
  assert.deepEqual(normalize(baseParse(actual)),normalize(expected.tree),phase);
 }
});
test('presentation preserves stop recovery and original date formatting',()=>{
 assert.equal(servicePhase({phase:'failed'},null,false),'stop-error');
 assert.equal(servicePresentation(servicePhase({phase:'unverified'},null,false)).next,'service-stop');
 assert.equal(servicePhase({phase:'stopped'},'start',false),'starting');
 assert.equal(servicePresentation('stopping').label,'停止');
 assert.equal(serviceStartedAt('invalid','en-US'),'暂无');
});
test('lost lifecycle replies are read back without repeating mutations',async()=>{
 for(const start of [true,false]){
  const calls:string[]=[];
  const invoke=async<T>(name:string)=>{calls.push(name);if(name!=='emby_service_status')throw Error('lost reply');return {phase:start?'running':'stopped'} as T;};
  assert.equal((await changeService(invoke,start,'same-id')).phase,start?'running':'stopped');
  assert.deepEqual(calls,[start?'emby_start':'emby_stop','emby_service_status']);
 }
});
test('pending startup is observed without inventing completion or repeating start',async()=>{
 for(const lost of [false,true]){
  const calls:string[]=[];
  const invoke=async<T>(name:string)=>{calls.push(name);if(lost&&name==='emby_start')throw Error('lost reply');return {phase:'starting',last_started_at:null} as T;};
  const observed=await changeService(invoke,true,'pending');
  assert.equal(observed.phase,'starting');
  assert.equal(servicePhase(observed,null,false),'starting');
  assert.deepEqual(calls,lost?['emby_start','emby_service_status']:['emby_start']);
 }
});
test('failed start and unknown stop stay distinguishable and cannot claim success',async()=>{
 const failedStart=async<T>(name:string)=>{if(name==='emby_start')throw Error('denied');return {phase:'stopped'} as T;};
 await assert.rejects(changeService(failedStart,true,'start'),(error:unknown)=>error instanceof ServiceControlFailure&&error.observed.phase==='error');
 await assert.rejects(changeService(async()=>{throw Error('offline');},false,'stop'),(error:unknown)=>error instanceof ServiceControlFailure&&error.observed.phase==='unverified');
 await assert.rejects(changeService(async<T>()=>({phase:'failed',error:{message:'lease not disabled'}} as T),false,'stop'),/lease not disabled/);
});

test('saving roots refreshes a running service and starts a stopped service exactly once',async()=>{
 for(const phase of ['running','stopped']){
  const calls:any[]=[];let starts=0;
  const invoke=async<T>(name:string,args?:Record<string,unknown>)=>{calls.push([name,args]);return {phase:name==='emby_start'?'running':phase} as T;};
  assert.equal((await refreshAfterRootsSaved(invoke,7,'save-start',{onStart:()=>{starts++;}})).phase,'running');
  assert.deepEqual(calls,phase==='running'?[['emby_service_status',undefined],['refresh_libraries',{revision:7}] ]:[['emby_service_status',undefined],['emby_start',{id:'save-start'}]]);
  assert.equal(starts,phase==='stopped'?1:0);
 }
});
test('post-save service failures never repeat a refresh or pretend an uncertain service is stopped',async()=>{
 for(const phase of ['failed','unverified','starting']){
  const calls:string[]=[];await assert.rejects(refreshAfterRootsSaved(async<T>(name)=>{calls.push(name);return {phase} as T;},1,'save'),ServiceControlFailure);assert.deepEqual(calls,['emby_service_status']);
 }
 const calls:string[]=[];
 await assert.rejects(refreshAfterRootsSaved(async<T>(name)=>{calls.push(name);if(name==='refresh_libraries')throw Error('已有任务正在运行');return {phase:'running'} as T;},1,'save'),/已有任务正在运行/);
 assert.deepEqual(calls,['emby_service_status','refresh_libraries']);
 const disposed:string[]=[];await assert.rejects(refreshAfterRootsSaved(async<T>(name)=>{disposed.push(name);return {phase:'stopped'} as T;},1,'save',{alive:()=>false}),/操作已取消/);assert.deepEqual(disposed,['emby_service_status']);
});

test('initial console preserves the original pre-status loading markup',async()=>{
 const actual=await renderToString(createSSRApp({render:()=>h(Panel,{phase:'starting',lastStartedAt:null,locale:'zh-CN',loading:true,updatedAt:'',summary:{total:0,movie:0,tv:0,errors:0,displayable:0,web_eligible:0,episodes_excluded:0,generated_at:null},webVersion:null,webHealthy:false,runtimeValid:false,webInstalled:false,busy:false,platform:'Windows'})}));
 assert.deepEqual(normalize(baseParse(actual)),normalize(baseParse(html)));
});
