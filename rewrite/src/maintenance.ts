// Existing v4.1.0 settings actions. Transaction identities remain internal.
import type {IntegrationStatus,MaintenancePlan as NativeMaintenancePlan} from './contracts';
export type MaintenanceAction='repair-web'|'rebuild-index'|'disable-integration';
export type CardMaintenanceAction=Exclude<MaintenanceAction,'rebuild-index'>;
export type MaintenanceStatus=IntegrationStatus;
export interface MaintenanceAttempt {id:string;action:CardMaintenanceAction;target:string|null}
export type MaintenancePlan=NativeMaintenancePlan;
type Invoke=<T>(name:string,args?:Record<string,unknown>)=>Promise<T>;
const message=(error:unknown)=>error&&typeof error==='object'&&'message' in error?String(error.message):String(error);
const code=(error:unknown)=>error&&typeof error==='object'&&'code' in error?String(error.code):'';
export class MaintenanceFailure extends Error {
 readonly releaseAttempt:boolean;
 constructor(message:string,releaseAttempt=false){super(message);this.releaseAttempt=releaseAttempt;}
}
export function restoredMaintenance(plan:MaintenancePlan|null):MaintenanceAttempt|null{
 if(!plan||!['planned','prepared'].includes(plan.phase)||!['repair-web','remove'].includes(plan.action))return null;
 return {id:plan.id,target:plan.target,action:plan.action==='remove'?'disable-integration':'repair-web'};
}
export async function maintainCard(invoke:Invoke,attempt:MaintenanceAttempt,alive=()=>true):Promise<MaintenanceStatus>{
 const active=()=>{if(!alive())throw new MaintenanceFailure('操作已取消');};
 const targetMatches=(value:MaintenanceStatus|null)=>{
  if(!value||!value.target||(attempt.target!==null&&value.target!==attempt.target))throw new MaintenanceFailure('Emby 安装目录已改变，请重新检查。');
  if(attempt.target===null)attempt.target=value.target;
  return value;
 };
 const planMatches=(value:MaintenancePlan)=>{
  if(value.id!==attempt.id||value.target!==attempt.target||value.action!==(attempt.action==='repair-web'?'repair-web':'remove')||!value.fingerprint)
   throw new MaintenanceFailure('维护回执与已确认的操作不一致。');
  return value;
 };
 const receipt=async()=>{
  try{const value=await invoke<MaintenancePlan|null>('emby_operation',{id:attempt.id});active();return value?planMatches(value):null;}
  catch(error){if(code(error)==='operation-not-found')return null;throw error;}
 };
 const verified=async()=>{
  const status=targetMatches(await invoke<MaintenanceStatus|null>('emby_status'));active();
  if(attempt.action==='disable-integration'?status.installed:!status.installed||!status.healthy)
   throw new MaintenanceFailure('维护操作未通过结果复核，请查看故障处理信息。',true);
  return status;
 };
 try{
  active();let target=await invoke<MaintenanceStatus|null>('emby_status');active();
  if(!target){
   // A pending operation may never silently switch to another installation.
   if(attempt.target!==null)throw new MaintenanceFailure('Emby 安装目录已改变，请重新检查。');
   const candidates=await invoke<{web:string;endpoint:string}[]>('emby_discover');active();
   const available=candidates.filter(candidate=>candidate.endpoint);
   if(available.length!==1)throw new MaintenanceFailure(available.length?'检测到多个 Emby 安装目录，无法确认维护目标。':'未检测到 Emby 安装目录。');
   target=await invoke<MaintenanceStatus>('emby_connect',{path:available[0].web});active();
  }
  target=targetMatches(target);
  if(target.requires_permission){target=targetMatches(await invoke<MaintenanceStatus>('emby_authorize'));active();}
  if(target.requires_permission)throw new MaintenanceFailure('尚未取得 Emby 安装目录访问权限。');
  let plan=await receipt();active();
  if(plan?.phase==='committed')return await verified();
  if(plan?.phase==='rolled-back')throw new MaintenanceFailure('维护操作已回滚，请重新确认。',true);
  if(plan&&plan.phase!=='planned')throw new MaintenanceFailure('维护结果尚未确认，请先恢复当前操作。');
  if(attempt.action==='disable-integration'){
   const service=await invoke<{phase:string;error?:{message:string}|null}>('emby_stop');active();
   if(service.phase!=='stopped'||service.error)throw new MaintenanceFailure(service.error?.message||'服务尚未确认停止。');
  }
  let failure:unknown;
  try{
   // Recheck after authorization, receipt reading and stop; none grants consent
   // to maintain a subsequently selected installation.
   targetMatches(await invoke<MaintenanceStatus|null>('emby_status'));active();
   if(attempt.action==='repair-web')targetMatches(await invoke<MaintenanceStatus>('emby_repair',{id:attempt.id}));
   else{
    if(!plan)plan=planMatches(await invoke<MaintenancePlan>('emby_plan',{id:attempt.id,action:'remove'}));
    active();
    if(plan.phase!=='planned')throw new MaintenanceFailure('维护计划尚未就绪。');
    targetMatches(await invoke<MaintenanceStatus>('emby_apply',{id:plan.id,fingerprint:plan.fingerprint}));
   }
  }catch(error){failure=error;}
  active();plan=await receipt();active();
  if(plan?.phase==='rolled-back')throw new MaintenanceFailure(failure?message(failure):'维护操作已回滚，请重新确认。',true);
  if(plan?.phase!=='committed')throw new MaintenanceFailure(failure?message(failure):'维护结果尚未确认，请先恢复当前操作。',plan?.phase==='planned'&&['emby-external-change','operation-conflict'].includes(code(failure)));
  if(failure instanceof MaintenanceFailure)throw failure;
  return await verified();
 }catch(error){if(error instanceof MaintenanceFailure)throw error;throw new MaintenanceFailure(message(error));}
}
