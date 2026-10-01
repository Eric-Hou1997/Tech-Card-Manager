// Presentation states and strings from the v4.1.0 Manager's renderService.
import type {AppError,IntegrationStatus,ServiceStatus} from './contracts';
export type ServicePhase='starting'|'stopping'|'migrating'|'exiting'|'running'|'stopped'|'stop-error'|'error'|'legacy-blocked';
// The UI also owns a short-lived unverified observation before a backend
// response exists; every field received from Rust still comes from ServiceStatus.
export type ServiceObservation=Omit<ServiceStatus,'last_started_at'|'lease'|'error'>&{last_started_at?:ServiceStatus['last_started_at'];lease?:ServiceStatus['lease'];error?:Pick<AppError,'message'>|null};
export type IntegrationObservation=IntegrationStatus&{index_current?:boolean};
export function servicePhase(service:ServiceObservation,pending:'start'|'stop'|null,legacy:boolean):ServicePhase {
 if(pending)return pending==='start'?'starting':'stopping';
 if(service.phase==='running')return 'running';
 if(service.phase==='checking'||service.phase==='starting')return 'starting';
 if(service.phase==='unverified'||service.phase==='failed')return 'stop-error';
 if(service.phase==='stopped')return legacy?'legacy-blocked':'stopped';
 return service.phase==='error'?'error':'stop-error';
}
export function servicePresentation(state:ServicePhase) {
 const busy=['starting','stopping','migrating','exiting'].includes(state);
 const stop=['running','stopping','stop-error','exiting'].includes(state);
 const values:Record<ServicePhase,[string,string,string]>={
  running:['Emby 卡片显示服务已启动','ok','服务运行中'],
  starting:['Emby 卡片显示服务正在启动','warn','正在启动'],
  stopping:['Emby 卡片显示服务正在关闭','warn','正在关闭'],
  exiting:['Emby 卡片显示服务正在关闭','warn','正在关闭'],
  migrating:['Emby 卡片显示服务正在迁移旧版','warn','正在迁移'],
  'stop-error':['Emby 卡片显示服务关闭异常','bad','撤卡待重试'],
  error:['Emby 卡片显示服务启动失败','bad','服务异常'],
  stopped:['Emby 卡片显示服务已关闭','','服务已关闭'],
  'legacy-blocked':['Emby 卡片显示服务已关闭','warn','服务已关闭'],
 };
 const [text,pillClass,pill]=values[state];
 return {text,pillClass,pill,busy,stop,label:stop?'停止':'启动',next:busy?'':stop?'service-stop':'service-start',error:state==='stop-error'||state==='error'};
}
export function serviceStartedAt(value:string|null|undefined,locale:string):string {
 if(!value)return '暂无';const date=new Date(value);if(Number.isNaN(date.getTime()))return '暂无';
 return new Intl.DateTimeFormat(locale,{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit',hour12:false}).format(date);
}
