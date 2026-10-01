import test from 'node:test';
import assert from 'node:assert/strict';
import {migrateLegacyCard,migrateLegacySystem,LegacyMigrationFailure} from '../src/legacy-migration.ts';
const attempt={id:'migration',target:'/web',reviewed:'original'};

test('system reply loss resolves the exact durable receipt and requires a fresh empty inventory',async()=>{
 for(const lost of [false,true]){
  const calls:string[]=[];let dirty=false;
  const invoke=async<T>(name:string,args?:Record<string,unknown>):Promise<T>=>{
   calls.push(name);
   if(name==='emby_migrate_legacy_system'){assert.deepEqual(args,{id:attempt.id,reviewed:attempt.reviewed});if(lost)throw Error('reply lost');return {...attempt,phase:'complete'} as T;}
   if(name==='emby_legacy_operation')return {...attempt,phase:'complete'} as T;
   if(name==='emby_legacy_components')return {fingerprint:'now',items:dirty?['new task']:[],errors:[]} as T;
   assert.fail(name);
  };
  await migrateLegacySystem(invoke,attempt);assert.deepEqual(calls,['emby_migrate_legacy_system','emby_legacy_operation','emby_legacy_components']);
  dirty=true;await assert.rejects(migrateLegacySystem(invoke,attempt),error=>error instanceof LegacyMigrationFailure&&error.releaseAttempt);
 }
});
test('unknown system results keep their operation ID; explicit rejection or failed receipt allows fresh confirmation',async()=>{
 for(const mode of ['missing','running','failed','rejected','mismatch']){
  const calls:string[]=[];
  const invoke=async<T>(name:string):Promise<T>=>{
   calls.push(name);
   if(name==='emby_migrate_legacy_system')throw mode==='rejected'?{code:'legacy-system-component-unverified',message:'unknown component'}:Error('reply lost');
   if(name==='emby_legacy_operation')return (mode==='missing'||mode==='rejected'?null:{...attempt,id:mode==='mismatch'?'wrong':attempt.id,phase:mode,recovery_errors:[{message:'恢复未确认',path:'original Run value'},{message:'状态文件已被替换，未覆盖',path:'data/agent.pid'}]}) as T;
   assert.fail(name);
  };
  await assert.rejects(migrateLegacySystem(invoke,attempt),error=>{
   assert(error instanceof LegacyMigrationFailure);
   assert.equal(error.releaseAttempt,mode==='failed'||mode==='rejected');
   if(mode==='failed'){assert.match(error.message,/original Run value/);assert.match(error.message,/data\/agent.pid/);assert.doesNotMatch(error.message,/登录项恢复未完成/);}
   return true;
  });
  assert.deepEqual(calls,['emby_migrate_legacy_system','emby_legacy_operation']);
 }
});
test('system migration disposal stops follow-up reads and does not initiate another effect',async()=>{
 let active=true;const calls:string[]=[];
 const invoke=async<T>(name:string):Promise<T>=>{calls.push(name);active=false;return {...attempt,phase:'complete'} as T;};
 await assert.rejects(migrateLegacySystem(invoke,attempt,()=>active),/操作已取消/);
 assert.deepEqual(calls,['emby_migrate_legacy_system']);
});
function fixture(){
 const state={target:{target:'/web',installed:true,healthy:false,requires_permission:false,issues:[],legacy_patch:{fingerprint:'original',unsafe_patch:false,items:['旧版网页卡片 v4.1.0']}} as any,plan:null as any,calls:[] as string[],lost:''};
 const invoke=async<T>(name:string,args?:Record<string,unknown>):Promise<T>=>{
  state.calls.push(name);
  if(name==='emby_status')return structuredClone(state.target);
  if(name==='emby_authorize'){state.target.requires_permission=false;return structuredClone(state.target);}
  if(name==='emby_operation')return structuredClone(state.plan);
  if(name==='emby_plan'){
   assert.deepEqual(args,{id:'migration',action:'adopt',reviewed:'original'});
   state.plan={id:'migration',target:'/web',action:'adopt',legacy_review:'original',fingerprint:'plan',phase:'planned'};
   if(state.lost==='plan')throw Error('reply lost');return structuredClone(state.plan);
  }
  if(name==='emby_apply'){
   assert.deepEqual(args,{id:'migration',fingerprint:'plan'});
   state.plan.phase='committed';state.target.healthy=true;state.target.legacy_patch=null;
   if(state.lost==='apply')throw Error('reply lost');return structuredClone(state.target);
  }
  if(name==='emby_start'){assert.deepEqual(args,{id:'migration-start'});return {phase:'running'} as T;}
  assert.fail(name);
 };
 return {state,invoke};
}
test('original confirmation authorizes, adopts the reviewed snapshot, then starts after committed receipt',async()=>{
 const {state,invoke}=fixture();state.target.requires_permission=true;
 assert.equal((await migrateLegacyCard(invoke,attempt)).phase,'running');
 assert.ok(state.calls.indexOf('emby_authorize')<state.calls.indexOf('emby_plan'));
 assert.ok(state.calls.lastIndexOf('emby_operation')<state.calls.indexOf('emby_start'));
});
test('lost plan/apply replies and an explicit retry reuse the exact committed receipt',async()=>{
 for(const lost of ['plan','apply']){
  const {state,invoke}=fixture();state.lost=lost;
  await migrateLegacyCard(invoke,attempt);await migrateLegacyCard(invoke,attempt);
  assert.equal(state.calls.filter(name=>name==='emby_plan').length,1);
  assert.equal(state.calls.filter(name=>name==='emby_apply').length,1);
 }
});
test('changed or unsafe observations never authorize a plan and can be reviewed afresh',async()=>{
 for(const changed of [{fingerprint:'new'},{unsafe_patch:true}]){
  const {state,invoke}=fixture();Object.assign(state.target.legacy_patch,changed);
  await assert.rejects(migrateLegacyCard(invoke,attempt),error=>error instanceof LegacyMigrationFailure&&error.releaseAttempt);
  assert.ok(!state.calls.includes('emby_plan'));assert.ok(!state.calls.includes('emby_start'));
 }
});
test('another target or mismatched durable receipt cannot be adopted or started',async()=>{
 for(const kind of ['target','receipt']){
  const {state,invoke}=fixture();
  if(kind==='target')state.target.target='/other';else state.plan={id:'other',target:'/web',action:'adopt',legacy_review:'original',fingerprint:'plan',phase:'committed'};
  await assert.rejects(migrateLegacyCard(invoke,attempt));assert.ok(!state.calls.includes('emby_apply'));assert.ok(!state.calls.includes('emby_start'));
 }
});
test('unconfirmed, failed and rolled back receipts never fall through to service start',async()=>{
 for(const phase of ['prepared','rolled-back']){
  const {state,invoke}=fixture();state.plan={...attempt,action:'adopt',legacy_review:'original',fingerprint:'plan',phase};
  await assert.rejects(migrateLegacyCard(invoke,attempt),error=>error instanceof LegacyMigrationFailure&&error.releaseAttempt===(phase==='rolled-back'));
  assert.ok(!state.calls.includes('emby_apply'));assert.ok(!state.calls.includes('emby_start'));
 }
});
test('unmount after a read cancels before authorization or mutation',async()=>{
 const {state,invoke}=fixture();
 await assert.rejects(migrateLegacyCard(invoke,attempt,()=>false));
 assert.deepEqual(state.calls,['emby_status']);
});
test('an unverified apply response retains the attempt and never starts',async()=>{
 const {state,invoke}=fixture();
 const disconnected=async<T>(name:string,args?:Record<string,unknown>):Promise<T>=>{
  if(name==='emby_apply'){state.calls.push(name);throw Error('disconnected');}
  return invoke<T>(name,args);
 };
 await assert.rejects(migrateLegacyCard(disconnected,attempt),error=>error instanceof LegacyMigrationFailure&&!error.releaseAttempt);
 assert.ok(!state.calls.includes('emby_start'));assert.equal(state.plan.phase,'planned');
});
test('a failed start preserves its observed state and retries without another migration',async()=>{
 const {state,invoke}=fixture();let fail=true;
 const startup=async<T>(name:string,args?:Record<string,unknown>):Promise<T>=>{
  if(name==='emby_start'&&fail){state.calls.push(name);throw Error('start denied');}
  if(name==='emby_service_status')return {phase:'stopped'} as T;
  return invoke<T>(name,args);
 };
 await assert.rejects(migrateLegacyCard(startup,attempt),error=>error instanceof LegacyMigrationFailure&&error.service?.phase==='error'&&!error.releaseAttempt);
 fail=false;assert.equal((await migrateLegacyCard(startup,attempt)).phase,'running');
 assert.equal(state.calls.filter(name=>name==='emby_apply').length,1);
});
