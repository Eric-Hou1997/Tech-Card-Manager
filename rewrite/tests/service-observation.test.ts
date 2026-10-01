import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import * as vue from 'vue';
import {renderer} from './support/mount-vue.ts';
import * as control from '../src/service-control.ts';

const source=await readFile(new URL('../src/useEmby.ts',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const settle=async()=>{for(let i=0;i<30;i++)await vue.nextTick();};
function mount(invoke:(name:string)=>Promise<unknown>){
 const timers=new Map<number,()=>void>();let serial=0;
 const module={exports:{} as any};
 const require=(id:string)=>{if(id==='vue')return vue;if(id==='@tauri-apps/api/core')return {invoke};if(id==='./service-control')return control;throw Error(id);};
 new Function('require','module','exports','setTimeout','clearTimeout',code)(require,module,module.exports,(callback:()=>void)=>{timers.set(++serial,callback);return serial;},(id:number)=>timers.delete(id));
 const {host,render}=renderer();let model:any;
 const app=render.createApp({setup(){model=module.exports.useEmby();return ()=>null;}});app.mount(host);
 return {model,close:()=>app.unmount(),poll:()=>{assert.equal(timers.size,1);const [id,callback]=timers.entries().next().value!;timers.delete(id);callback();},timers};
}

test('status read errors retain the last service observation just as the original refresh does',async()=>{
 const baseline=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
 const refresh=baseline.split('\n').find(line=>line.startsWith('async function refresh()'))!;
 let renders=0;const messages:string[]=[];
 const context=vm.createContext({api:async()=>{throw Error('connection lost');},renderStatus:()=>renders++,renderJob:()=>renders++,loadCatalog:()=>renders++,toast:(message:string)=>messages.push(message)});
 vm.runInContext(refresh,context);await vm.runInContext('refresh()',context);
 assert.equal(renders,0);assert.deepEqual(messages,['状态读取失败：connection lost']);
 for(const phase of ['running','stopped','unverified']){
  let fail=false,next=phase;
  const mounted=mount(async name=>{
   if(name==='emby_service_status'){if(fail)throw {message:'connection lost'};return {phase:next,last_started_at:'2026-09-12T08:00:00Z',error:null};}
   if(name==='emby_status'||name==='emby_operation')return null;
   if(name==='emby_environment')return {web:'',data:null,endpoint:'',version:null,issues:[]};
   if(name==='emby_legacy_components')return {fingerprint:'empty',items:[],errors:[]};if(name==='emby_legacy_operation')return null;if(name==='emby_authorization_available')return false;
   if(name==='incremental_status')return {phase:'stopped',error:null};
   throw Error('unexpected mutation: '+name);
  });
  try{
   await settle();const previous=mounted.model.service.value,observedAt=mounted.model.observedAt.value;
   fail=true;mounted.poll();await settle();
   assert.equal(mounted.model.service.value,previous);assert.equal(mounted.model.service.value.phase,phase);
   assert.equal(mounted.model.observedAt.value,observedAt);assert.equal(mounted.model.error.value,messages[0]);
   fail=false;next='stopped';mounted.poll();await settle();assert.equal(mounted.model.service.value.phase,'stopped');
  }finally{mounted.close();assert.equal(mounted.timers.size,0);}
 }
});

test('an unavailable initial status does not invent a stop failure and disposal cancels polling',async()=>{
 const mounted=mount(async()=>{throw Error('unavailable');});
 try{await settle();assert.equal(mounted.model.service.value.phase,'checking');assert.equal(mounted.model.ready.value,false);assert.equal(mounted.model.observedAt.value,'');assert.equal(mounted.model.error.value,'状态读取失败：unavailable');}
 finally{mounted.close();assert.equal(mounted.timers.size,0);}
});
test('a transient startup read retries the complete Emby snapshot before marking the UI ready',async()=>{
 for(const failedCommand of ['emby_environment','emby_legacy_components','emby_service_status']){
  let fail=true;const calls:string[]=[];
  const environment={web:'/isolated/Emby/web',endpoint:'http://127.0.0.1:8096',issues:[]};
  const legacy={fingerprint:'known',items:['旧版登录启动项'],errors:[]};
  const mounted=mount(async name=>{
   calls.push(name);if(fail&&name===failedCommand)throw Error('temporary read failure');
   if(name==='emby_authorization_available')return true;
   if(name==='emby_status')return {phase:'ready',target:environment.web,healthy:true};
   if(name==='emby_environment')return environment;
   if(name==='emby_operation'||name==='emby_legacy_operation')return null;
   if(name==='emby_legacy_components')return legacy;
   if(name==='emby_service_status')return {phase:'stopped',error:null};
   if(name==='incremental_status')return {phase:'stopped',error:null};
   throw Error('unexpected mutation: '+name);
  });
  try{
   await settle();assert.equal(mounted.model.ready.value,false);
   mounted.poll();await settle();assert.equal(mounted.model.ready.value,false);
   fail=false;mounted.poll();await settle();
   assert.equal(mounted.model.ready.value,true);
   assert.deepEqual(mounted.model.environment.value,environment);
   assert.deepEqual(mounted.model.legacyComponents.value,legacy);
   assert.equal(mounted.model.authorizationAvailable.value,true);
   const completedReads=calls.filter(name=>name==='emby_legacy_components').length;
   mounted.poll();await settle();assert.equal(calls.filter(name=>name==='emby_legacy_components').length,completedReads);
  }finally{mounted.close();assert.equal(mounted.timers.size,0);}
 }
});
test('migration startup failure remains visible across stopped polls until an explicit success',async()=>{
 const mounted=mount(async name=>{
  if(name==='emby_service_status')return {phase:'stopped',error:null};
  if(name==='emby_status'||name==='emby_operation')return null;
  if(name==='emby_environment')return {web:'',issues:[]};
  if(name==='emby_legacy_components')return {fingerprint:'empty',items:[],errors:[]};if(name==='emby_legacy_operation')return null;if(name==='emby_authorization_available')return false;
  if(name==='incremental_status')return {phase:'stopped',error:null};
  throw Error(name);
 });
 try{
  await settle();mounted.model.acceptService({phase:'error',error:{message:'start denied'}});
  mounted.poll();await settle();assert.equal(mounted.model.service.value.phase,'error');
  mounted.model.acceptService({phase:'running',error:null});mounted.poll();await settle();
  assert.equal(mounted.model.service.value.phase,'stopped');
 }finally{mounted.close();assert.equal(mounted.timers.size,0);}
});

test('system component discovery runs on initial/explicit review, not each status poll, and ignores late reads',async()=>{
 let reads=0;const replies:((value:unknown)=>void)[]=[];
 const original={fingerprint:'initial',items:['旧版登录启动项'],errors:[]};
 const mounted=mount(async name=>{
  if(name==='emby_legacy_components'){if(++reads===1)return original;return new Promise(resolve=>replies.push(resolve));}
  if(name==='emby_service_status')return {phase:'stopped',error:null};
  if(name==='emby_status'||name==='emby_operation')return null;
  if(name==='emby_environment')return {web:'',issues:[]};
  if(name==='emby_legacy_operation')return null;if(name==='emby_authorization_available')return false;
  if(name==='incremental_status')return {phase:'stopped',error:null};
  throw Error(name);
 });
 try{
  await settle();assert.deepEqual(mounted.model.legacyComponents.value,original);
  mounted.poll();await settle();assert.equal(reads,1);
  const old=mounted.model.refreshLegacy(),newer=mounted.model.refreshLegacy();
  const denied={fingerprint:'changed',items:[],errors:[{code:'read-denied',message:'读取失败'}]};
  replies[1]!(denied);await newer;replies[0]!({fingerprint:'stale-empty',items:[],errors:[]});await old;
  assert.deepEqual(mounted.model.legacyComponents.value,denied);
  const late=mounted.model.refreshLegacy();mounted.close();replies[2]!({fingerprint:'after-unmount',items:[],errors:[]});await late;
  assert.deepEqual(mounted.model.legacyComponents.value,denied);assert.equal(mounted.timers.size,0);
  await mounted.model.refreshLegacy();assert.equal(reads,4);
 }finally{mounted.close();}
});
