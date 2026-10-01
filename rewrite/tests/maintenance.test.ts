import test from 'node:test';
import assert from 'node:assert/strict';
import {maintainCard,restoredMaintenance,MaintenanceFailure,type MaintenanceAttempt} from '../src/maintenance.ts';
const target={installed:true,healthy:true,requires_permission:false,target:'/isolated/emby',issues:[]};
const attempt=(action:MaintenanceAttempt['action']='disable-integration'):MaintenanceAttempt=>({id:'same',action,target:target.target});
function backend(options:{deny?:boolean;stopFailed?:boolean;lost?:boolean;committed?:boolean;ambiguous?:boolean;unverified?:boolean;receiptLost?:boolean}={}){
 const calls:string[]=[];const argumentsSeen:Array<Record<string,unknown>|undefined>=[];
 let installed=true,plan:any=null,reads=0;const controls={after:(_name:string)=>{},target:target.target};
 const invoke=async<T>(name:string,args?:Record<string,unknown>):Promise<T>=>{
  calls.push(name);argumentsSeen.push(args);let value:unknown;
  switch(name){
   case 'emby_status':value=options.ambiguous?null:{...target,target:controls.target,requires_permission:Boolean(options.deny),installed:installed||Boolean(options.unverified)};break;
   case 'emby_discover':value=[{web:'/a',endpoint:'http://localhost'},{web:'/b',endpoint:'http://localhost'}];break;
   case 'emby_authorize':throw Error('denied');
   case 'emby_stop':value={phase:options.stopFailed?'failed':'stopped'};break;
   case 'emby_plan':plan={id:args!.id,target:controls.target,fingerprint:'exact',phase:'planned',action:args!.action};value={...plan};break;
   case 'emby_repair':installed=true;plan={id:args!.id,target:controls.target,fingerprint:'exact',phase:options.lost&&!options.committed?'prepared':'committed',action:'repair-web'};if(options.lost)throw Error('lost repair');value={...target,target:controls.target};break;
   case 'emby_apply':assert.equal(args!.fingerprint,'exact');installed=false;plan.phase=options.lost&&!options.committed?'prepared':'committed';if(options.lost)throw Error('lost');value={...target,target:controls.target,installed:options.unverified};break;
   case 'emby_operation':reads++;if(options.receiptLost&&reads===2)throw Error('receipt unavailable');value=plan?{...plan}:null;break;
   default:throw Error(name);
  }
  controls.after(name);return value as T;
 };
 return {calls,argumentsSeen,invoke,controls,get plan(){return plan;},set plan(value:any){plan=value;}};
}
test('restore native Emby verifies its receipt and current result after stopping and applying',async()=>{
 const b=backend();await maintainCard(b.invoke,attempt());
 assert.deepEqual(b.calls,['emby_status','emby_operation','emby_stop','emby_status','emby_plan','emby_apply','emby_operation','emby_status']);
});
test('denied permission or failed stop never starts a file transaction',async()=>{
 for(const opts of [{deny:true},{stopFailed:true}]){const b=backend(opts);await assert.rejects(maintainCard(b.invoke,attempt()));assert.ok(!b.calls.includes('emby_plan'));assert.ok(!b.calls.includes('emby_apply'));}
});
test('lost apply reply checks exactly that receipt without repeating the write',async()=>{
 const b=backend({lost:true,committed:true});await maintainCard(b.invoke,attempt());
 assert.equal(b.calls.filter(name=>name==='emby_apply').length,1);assert.deepEqual(b.calls.slice(-2),['emby_operation','emby_status']);
});
test('uncertain results and ambiguous installations cannot report completion',async()=>{
 for(const opts of [{lost:true},{unverified:true},{ambiguous:true}]){const b=backend(opts);await assert.rejects(maintainCard(b.invoke,{...attempt(),target:null}));}
});
test('repair retains its original entry and verifies completion without stopping the service',async()=>{
 const b=backend();await maintainCard(b.invoke,attempt('repair-web'));
 assert.deepEqual(b.calls,['emby_status','emby_operation','emby_status','emby_repair','emby_operation','emby_status']);
});
test('online repair recovers a lost response through its committed receipt',async()=>{
 const b=backend({lost:true,committed:true});await maintainCard(b.invoke,attempt('repair-web'));
 assert.equal(b.calls.filter(name=>name==='emby_repair').length,1);assert.ok(!b.calls.includes('emby_stop'));
});
test('unfinished repair receipt stays unresolved on retry without repeating repair',async()=>{
 const b=backend({lost:true}),pending=attempt('repair-web');
 await assert.rejects(maintainCard(b.invoke,pending));const count=b.calls.length;
 await assert.rejects(maintainCard(b.invoke,pending),{message:'维护结果尚未确认，请先恢复当前操作。'});
 assert.deepEqual(b.calls.slice(count),['emby_status','emby_operation']);assert.equal(b.calls.filter(name=>name==='emby_repair').length,1);
});
test('a lost result and lost receipt reuse the same operation without another stop or apply',async()=>{
 const b=backend({lost:true,committed:true,receiptLost:true}),pending=attempt();
 await assert.rejects(maintainCard(b.invoke,pending),{message:'receipt unavailable'});
 await maintainCard(b.invoke,pending);
 assert.equal(b.calls.filter(name=>name==='emby_apply').length,1);assert.equal(b.calls.filter(name=>name==='emby_stop').length,1);
 for(let i=0;i<b.calls.length;i++)if(['emby_apply','emby_operation'].includes(b.calls[i]))assert.equal(b.argumentsSeen[i]?.id,'same');
});
test('a restored planned removal applies that plan instead of preparing a changed payload',async()=>{
 const b=backend();b.plan={id:'same',target:target.target,fingerprint:'exact',phase:'planned',action:'remove'};
 const pending=restoredMaintenance(b.plan)!;assert.deepEqual(pending,attempt());
 await maintainCard(b.invoke,pending);assert.ok(!b.calls.includes('emby_plan'));assert.equal(b.calls.filter(name=>name==='emby_apply').length,1);
 assert.equal(restoredMaintenance({...b.plan,phase:'committed'}),null);assert.equal(restoredMaintenance({...b.plan,action:'adopt',phase:'planned'}),null);
});
test('wrong receipt identity, action or directory cannot authorize a write or completion',async()=>{
 for(const replacement of [{id:'other'},{action:'install'},{target:'/different'},{fingerprint:''}]){
  const b=backend();b.plan={id:'same',action:'remove',target:target.target,phase:'committed',fingerprint:'exact',...replacement};
  await assert.rejects(maintainCard(b.invoke,attempt()),{message:'维护回执与已确认的操作不一致。'});assert.deepEqual(b.calls,['emby_status','emby_operation']);
 }
});
test('changing target after stop or unmounting prevents the remaining mutation',async()=>{
 for(const unmount of [false,true]){
  const b=backend();let alive=true;
  b.controls.after=name=>{if(name==='emby_stop'){if(unmount)alive=false;else b.controls.target='/changed';}};
  await assert.rejects(maintainCard(b.invoke,attempt(),()=>alive));assert.ok(!b.calls.includes('emby_plan'));assert.ok(!b.calls.includes('emby_apply'));
 }
});
test('rollback releases the attempt only after reading its exact matching receipt',async()=>{
 const b=backend();b.plan={id:'same',action:'repair-web',target:target.target,phase:'rolled-back',fingerprint:'exact'};
 await assert.rejects(maintainCard(b.invoke,attempt('repair-web')),(error:any)=>error instanceof MaintenanceFailure&&error.releaseAttempt);
 assert.ok(!b.calls.includes('emby_repair'));
});
