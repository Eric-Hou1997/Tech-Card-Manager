import {changeService,ServiceControlFailure} from './service-control.ts';
import type {IntegrationObservation,ServiceObservation} from './console';
import type {LegacyComponentsReview as NativeLegacyComponentsReview} from './contracts';
type Invoke=<T>(name:string,args?:Record<string,unknown>)=>Promise<T>;
export interface LegacyAttempt {id:string;target:string;reviewed:string}
export type LegacyComponentsReview=NativeLegacyComponentsReview;
export interface LegacySystemAttempt {id:string;reviewed:string;target:string|null}
export interface SystemPlan {id:string;reviewed:string;phase:string;items?:string[];recovery_errors?:{message:string;path?:string|null}[]}
interface Plan {id:string;target:string;action:string;phase:string;fingerprint:string;legacy_review?:string}
export class LegacyMigrationFailure extends Error {
 readonly releaseAttempt:boolean;
 readonly service?:ServiceObservation;
 constructor(message:string,releaseAttempt=false,service?:ServiceObservation){super(message);this.releaseAttempt=releaseAttempt;this.service=service;}
}
const message=(error:unknown)=>error&&typeof error==='object'&&'message' in error?String(error.message):String(error);
const code=(error:unknown)=>error&&typeof error==='object'&&'code' in error?String(error.code):'';

export async function migrateLegacySystem(invoke:Invoke,attempt:LegacySystemAttempt,alive=()=>true):Promise<void>{
 const active=()=>{if(!alive())throw new LegacyMigrationFailure('操作已取消');};
 const matches=(plan:SystemPlan|null)=>{
  if(plan&&(plan.id!==attempt.id||plan.reviewed!==attempt.reviewed))throw new LegacyMigrationFailure('系统迁移回执与已确认清单不一致。');
  return plan;
 };
 let failure:unknown;
 active();
 try{matches(await invoke<SystemPlan>('emby_migrate_legacy_system',{id:attempt.id,reviewed:attempt.reviewed}));}
 catch(error){failure=error;}
 active();
 const receipt=matches(await invoke<SystemPlan|null>('emby_legacy_operation',{id:attempt.id}));active();
 const rejectedBeforePlan=['legacy-system-component-unverified','legacy-components-review-changed','legacy-startup-unowned','legacy-system-validation','legacy-system-platform','legacy-system-service-active'].includes(code(failure));
 if(receipt?.phase!=='complete'){
  const recovery=receipt?.recovery_errors?.map(error=>error.path?`${error.message}：${error.path}`:error.message).join('；');
  throw new LegacyMigrationFailure((failure?message(failure):'系统迁移结果尚未确认，请重新检查。')+(recovery?'；'+recovery:''),receipt?.phase==='failed'||(!receipt&&rejectedBeforePlan));
 }
 const current=await invoke<LegacyComponentsReview>('emby_legacy_components',{reviewed:null});active();
 if(current.items.length||current.errors.length)throw new LegacyMigrationFailure('系统组件状态再次改变，请重新确认迁移清单。',true);
}

// The original confirmation reviews one target and its complete fixed-file
// snapshot. Keep this identity across transport failures and explicit retries.
export async function migrateLegacyCard(invoke:Invoke,attempt:LegacyAttempt,alive=()=>true):Promise<ServiceObservation>{
 const active=()=>{if(!alive())throw new LegacyMigrationFailure('操作已取消');};
 const targetMatches=(value:IntegrationObservation|null)=>{
  if(!value||value.target!==attempt.target)throw new LegacyMigrationFailure('Emby 安装目录已改变，请重新检查。');
  return value;
 };
 const planMatches=(plan:Plan)=>{
  if(plan.id!==attempt.id||plan.target!==attempt.target||plan.action!=='adopt'||plan.legacy_review!==attempt.reviewed||!plan.fingerprint)
   throw new LegacyMigrationFailure('迁移回执与已确认的清单不一致。');
  return plan;
 };
 const receipt=async()=>{
  try{const plan=await invoke<Plan|null>('emby_operation',{id:attempt.id});return plan?planMatches(plan):null;}
  catch(error){if(code(error)==='operation-not-found')return null;throw error;}
 };
 const reviewed=(value:IntegrationObservation)=>{
  if(!value.legacy_patch||value.legacy_patch.unsafe_patch||value.legacy_patch.fingerprint!==attempt.reviewed)
   throw {code:'emby-legacy-review-changed',message:'旧版网页文件已改变，请重新确认迁移清单'};
 };
 try{
  let target=targetMatches(await invoke<IntegrationObservation|null>('emby_status'));active();
  if(target.requires_permission){target=targetMatches(await invoke<IntegrationObservation>('emby_authorize'));active();}
  if(target.requires_permission)throw new LegacyMigrationFailure('尚未取得 Emby 安装目录访问权限。');
  let plan=await receipt();active();
  if(!plan){
   reviewed(target);
   try{plan=planMatches(await invoke<Plan>('emby_plan',{id:attempt.id,action:'adopt',reviewed:attempt.reviewed}));}
   catch(error){plan=await receipt();if(!plan)throw error;}
   active();
  }
  if(plan.phase==='rolled-back')throw new LegacyMigrationFailure('旧版迁移未完成，请重新确认迁移清单。',true);
  if(plan.phase!=='committed'){
   if(plan.phase!=='planned')throw new LegacyMigrationFailure('迁移结果尚未确认，请先恢复当前操作。');
   target=targetMatches(await invoke<IntegrationObservation|null>('emby_status'));active();reviewed(target);
   try{targetMatches(await invoke<IntegrationObservation>('emby_apply',{id:plan.id,fingerprint:plan.fingerprint}));}
   catch(error){const observed=await receipt();if(observed?.phase!=='committed')throw error;}
   active();
   plan=await receipt();active();
   if(plan?.phase!=='committed')throw new LegacyMigrationFailure('迁移结果尚未确认，请先恢复当前操作。');
  }
  target=targetMatches(await invoke<IntegrationObservation|null>('emby_status'));active();
  if(!target.installed||!target.healthy)throw new LegacyMigrationFailure('维护操作未通过结果复核，请查看故障处理信息。');
  return await changeService(invoke,true,attempt.id+'-start');
 }catch(error){
  if(error instanceof ServiceControlFailure)throw new LegacyMigrationFailure(message(error),false,error.observed);
  if(error instanceof LegacyMigrationFailure)throw error;
  // A changed review can be replaced only after the native serialized receipt
  // query proves no transaction is in flight or partially applied.
  if(code(error)==='emby-legacy-review-changed'){
   try{const plan=await receipt();if(!plan||plan.phase==='planned'||plan.phase==='rolled-back')throw new LegacyMigrationFailure(message(error),true);}
   catch(readError){if(readError instanceof LegacyMigrationFailure&&readError.releaseAttempt)throw readError;}
  }
  throw new LegacyMigrationFailure(message(error));
 }
}
