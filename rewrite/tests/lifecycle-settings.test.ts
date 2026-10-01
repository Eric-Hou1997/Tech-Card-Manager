import test from 'node:test';
import assert from 'node:assert/strict';
import {LifecycleSettings,type LifecycleInvoke,type LifecycleStatus} from '../src/lifecycle-settings.ts';
const initial=():LifecycleStatus=>({settings:{revision:0,close_action:'quit',launch_at_login:false,start_hidden:true},native_autostart:false,background_mode:'hide',tray_available:true,closing:false,error:null});
function transport(work:(command:string,args?:Record<string,unknown>)=>unknown){return (async(command,args)=>work(command,args)) as LifecycleInvoke;}
test('silent startup defaults off and saving it retains the observed native login state',async()=>{
 let value=initial();value.settings={...value.settings,launch_at_login:true,start_hidden:false};value.native_autostart=true;
 const writes:any[]=[];
 const model=new LifecycleSettings(transport((cmd,args)=>{
  if(cmd==='lifecycle_status')return value;
  assert.equal(cmd,'lifecycle_apply');writes.push(structuredClone(args));value={...value,settings:{...(args!.settings as typeof value.settings),revision:1}};
  return {phase:'committed',desired:value.settings};
 }));
 assert.equal(model.settings.start_hidden,false);await model.read();assert.equal(model.settings.start_hidden,false);assert.equal(model.settings.launch_at_login,true);
 model.settings.start_hidden=true;await model.apply();assert.equal(writes.length,1);assert.equal(writes[0].settings.launch_at_login,true);assert.equal(model.settings.start_hidden,true);assert.equal(model.error,'');assert.equal(model.verified,true);
});
test('a failed initial read cannot save invented default settings and can be retried',async()=>{
 let fail=true,saves=0;const model=new LifecycleSettings(transport(cmd=>{if(cmd==='lifecycle_apply')saves++;if(fail)throw Error('unavailable');return initial();}));
 await model.read();model.settings.launch_at_login=true;await model.apply();assert.equal(saves,0);assert.equal(model.baseline,null);assert.equal(model.error,'unavailable');
 fail=false;await model.read();assert.equal(model.settings.launch_at_login,false);assert.equal(model.error,'');
});
test('lost apply reply retries the exact operation and committed settings survive status failure',async()=>{
 let calls:unknown[]=[];let failStatus=false;const model=new LifecycleSettings(transport((cmd,args)=>{
  if(cmd==='lifecycle_status'){if(failStatus)throw Error('query unavailable');return initial();}
  if(cmd==='operation_result')throw Error('receipt unavailable');
  calls.push(structuredClone(args));if(calls.length===1)throw Error('reply lost');failStatus=true;return {phase:'committed',desired:{...(args?.settings as object),revision:1}};
 }),()=> 'same-operation');
 await model.read();model.settings.launch_at_login=true;await model.apply();assert.equal(model.dirty,true);await model.apply();assert.deepEqual(calls[0],calls[1]);assert.equal(model.dirty,false);assert.equal(model.settings.revision,1);assert.equal(model.verified,false);assert.equal(model.error,'query unavailable');
});
test('editing after an ambiguous failure preserves the previous receipt until it is resolved',async()=>{
 let ids=0;const calls:unknown[]=[];const model=new LifecycleSettings(transport((cmd,args)=>{if(cmd==='lifecycle_status')return initial();if(cmd==='operation_result')throw Error('receipt unavailable');calls.push(structuredClone(args));throw Error('failed');}),()=>String(++ids));
 await model.read();model.settings.launch_at_login=true;await model.apply();model.settings.start_hidden=false;await model.apply();assert.deepEqual(calls[0],calls[1]);assert.equal(model.settings.start_hidden,false);
 await model.read();assert.equal(model.dirty,false);assert.equal(model.pending,null);
});
test('reverting a checkbox after a lost committed reply applies the newer choice at the new revision',async()=>{
 let ids=0,state=initial();const writes:any[]=[],receipts=new Map<string,any>();let lose=true;
 const model=new LifecycleSettings(transport((cmd,args)=>{
  if(cmd==='lifecycle_status')return structuredClone(state);
  const id=String(args?.id);
  if(cmd==='operation_result')return {kind:'lifecycle',result:receipts.get(id)};
  assert.equal(cmd,'lifecycle_apply');writes.push(structuredClone(args));
  if(receipts.has(id))return receipts.get(id);
  const settings=args!.settings as typeof state.settings;assert.equal(settings.revision,state.settings.revision);
  state={...state,settings:{...settings,revision:settings.revision+1},native_autostart:settings.launch_at_login};
  const receipt={id,phase:'committed',desired:{...state.settings}};receipts.set(id,receipt);
  if(lose){lose=false;throw Error('reply lost');}return receipt;
 }),()=>String(++ids));
 await model.read();model.settings.launch_at_login=true;await model.apply();assert.equal(state.native_autostart,true);assert.equal(model.verified,false);
 model.settings.launch_at_login=false;assert.equal(model.dirty,false);await model.apply();
 assert.equal(writes.length,3);assert.deepEqual(writes[0],writes[1]);assert.notEqual(writes[1].id,writes[2].id);assert.equal(writes[2].settings.revision,1);assert.equal(writes[2].settings.launch_at_login,false);
 assert.equal(state.native_autostart,false);assert.equal(model.pending,null);assert.equal(model.dirty,false);assert.equal(model.verified,true);assert.equal(model.settings.revision,2);
});
test('a persisted native failure allows an explicit retry with a fresh operation',async()=>{
 let ids=0,state=initial();const writes:any[]=[];const failure={code:'autostart-conflict',message:'other owner',path:'HKCU/Run/test'};
 const model=new LifecycleSettings(transport((cmd,args)=>{
  if(cmd==='lifecycle_status')return structuredClone(state);
  if(cmd==='operation_result')return {kind:'lifecycle',result:{id:args!.id,phase:'failed',error:failure}};
  assert.equal(cmd,'lifecycle_apply');writes.push(structuredClone(args));if(writes.length===1)throw failure;
  state={...state,settings:{...(args!.settings as typeof state.settings),revision:1},native_autostart:true};return {phase:'committed',desired:state.settings};
 }),()=>String(++ids));
 await model.read();model.settings.launch_at_login=true;await model.apply();assert.equal(model.pending,null);assert.equal(model.verified,false);assert.equal(model.error,'other owner');assert.equal(state.native_autostart,false);
 assert.equal(model.settings.launch_at_login,false);assert.equal(model.dirty,false);
 model.settings.launch_at_login=true;await model.apply();assert.notEqual(writes[0].id,writes[1].id);assert.equal(model.verified,true);assert.equal(model.error,'');assert.equal(model.settings.launch_at_login,true);
});
test('duplicate in-flight calls and closing with an unsaved draft are suppressed',async()=>{
 let release:((value:unknown)=>void)|undefined,calls=0;
 const model=new LifecycleSettings(transport(cmd=>{if(cmd==='lifecycle_status')return initial();calls++;return new Promise(resolve=>release=resolve);}));
 await model.read();model.settings.close_action='background';await model.window('quit_probe');assert.equal(calls,0);
 const first=model.apply();await model.apply();assert.equal(calls,1);release!({phase:'committed',desired:{...model.settings,revision:1}});await first;
});
test('window errors are visible and late reads cannot mutate a disposed panel',async()=>{
 let release:((value:unknown)=>void)|undefined;
 const model=new LifecycleSettings(transport(cmd=>{if(cmd==='background_window')throw Error('window-unavailable');return new Promise(resolve=>release=resolve);}));
 await model.window('background_window');assert.equal(model.error,'window-unavailable');
 const pending=model.read();model.dispose();release!(initial());await pending;assert.equal(model.baseline,null);assert.equal(model.status,null);
});

test('configuration notifications during an old read coalesce into one fresh read',async()=>{
 let reads=0,release!:(value:LifecycleStatus)=>void;
 const updated=initial();updated.settings={...updated.settings,revision:1,start_hidden:false};
 const model=new LifecycleSettings(transport(cmd=>{assert.equal(cmd,'lifecycle_status');reads++;return reads===1?new Promise(resolve=>release=resolve):updated;}));
 const first=model.read();await model.refresh();await model.refresh();assert.equal(reads,1);
 release(initial());await first;for(let i=0;i<6;i++)await Promise.resolve();
 assert.equal(reads,2);assert.equal(model.settings.revision,1);assert.equal(model.settings.start_hidden,false);assert.equal(model.pending,null);
});
test('configuration refresh cannot discard a failed save and drains after its receipt succeeds',async()=>{
 let reads=0,writes=0;const saved=initial();const ids:string[]=[];
 const model=new LifecycleSettings(transport((cmd,args)=>{
  if(cmd==='lifecycle_status'){reads++;return saved;}
  if(cmd==='operation_result')throw Error('receipt unavailable');
  assert.equal(cmd,'lifecycle_apply');ids.push(String(args?.id));if(++writes===1)throw Error('reply lost');
  saved.settings={...(args!.settings as typeof saved.settings),revision:1};saved.native_autostart=saved.settings.launch_at_login;
  return {phase:'committed',desired:saved.settings};
 }),()=> 'same-save');
 await model.read();model.settings.launch_at_login=true;await model.apply();await model.refresh();assert.equal(reads,1);assert.equal(model.settings.launch_at_login,true);assert.ok(model.pending);
 await model.apply();for(let i=0;i<6;i++)await Promise.resolve();assert.deepEqual(ids,['same-save','same-save']);assert.equal(reads,3);assert.equal(model.pending,null);assert.equal(model.dirty,false);
});
test('disposal cancels a queued refresh without starting another read',async()=>{
 let reads=0,release!:(value:LifecycleStatus)=>void;const model=new LifecycleSettings(transport(()=>{reads++;return new Promise(resolve=>release=resolve);}));
 const reading=model.read();await model.refresh();model.dispose();release(initial());await reading;await Promise.resolve();assert.equal(reads,1);assert.equal(model.baseline,null);
});
