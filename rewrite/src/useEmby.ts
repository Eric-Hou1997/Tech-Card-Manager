
import { ref, onMounted, onUnmounted, type InjectionKey } from 'vue';
import type {EmbyEnvironment,IncrementalStatus,MaintenancePlan} from './contracts';
import { invoke } from '@tauri-apps/api/core';
import type {IntegrationObservation,ServiceObservation} from './console';
import type {LegacyComponentsReview,SystemPlan} from './legacy-migration';
import {changeService,refreshAfterRootsSaved,ServiceControlFailure} from './service-control';
export function useEmby() {
type Status=IntegrationObservation & {phase:string};
type Plan=MaintenancePlan;
type Service=ServiceObservation;
const target = ref<Status | null>(null), plan = ref<Plan | null>(null), service = ref<Service>({phase:'checking',error:null});
const legacyComponents=ref<LegacyComponentsReview|null>(null);
const legacyPlan=ref<SystemPlan|null>(null);
let legacyRead=0;
async function refreshLegacy(){if(!alive)return;const token=++legacyRead;const value=await invoke<LegacyComponentsReview>('emby_legacy_components',{reviewed:null});if(!alive||token!==legacyRead)return;const operation=await invoke<SystemPlan|null>('emby_legacy_operation',{id:null});if(alive&&token===legacyRead){legacyComponents.value=value;legacyPlan.value=operation;}}
type Environment=EmbyEnvironment&{restore_error?:{message:string}};
const environment=ref<Environment|null>(null),candidates=ref<Environment[]>([]),endpoint=ref('http://127.0.0.1:8096');
const observedAt=ref('');
const pending=ref<'start'|'stop'|null>(null);let startFailed=false;
function acceptService(value:Service){service.value=value;startFailed=value.phase==='error';}
const busy = ref(false), error = ref(''), authorizationAvailable=ref(false), ready=ref(false);
const incremental=ref<IncrementalStatus>({phase:'unverified',session:null,last_check:null,completed_cycles:0,last_errors:0,error:null});
let generation=0,initialized=false;
async function authorize(){await run(async()=>{target.value=await invoke<Status>('emby_authorize');await refresh();});}
async function refresh(){if(!alive)return;const authorization=await invoke<boolean>('emby_authorization_available');if(!alive)return;const status=await invoke<Status|null>('emby_status');if(!alive)return;const value=await invoke<Environment>('emby_environment');if(!alive)return;const operation=await invoke<Plan|null>('emby_operation',{id:null});if(!alive)return;authorizationAvailable.value=authorization;target.value=status;environment.value=value.web?value:null;if(value.web)endpoint.value=value.endpoint;if(value.restore_error)error.value=value.restore_error.message;plan.value=operation;}
async function discover(){await run(async()=>{candidates.value=await invoke<Environment[]>('emby_discover');});}
async function connect(path:string){await run(async()=>{target.value=await invoke<Status>('emby_connect',{path});plan.value=null;await refresh();});}
async function checkServer(){await run(async()=>{environment.value=await invoke<Environment>('emby_check_server',{endpoint:endpoint.value});});}
async function chooseData(){await run(async()=>{const value=await invoke<Environment|null>('emby_data_directory');if(value)environment.value=value;});}

let poll: ReturnType<typeof setTimeout> | undefined;
let alive = true;
async function run(work: () => Promise<void>,propagate=false) { if(!alive||busy.value){if(propagate)throw Error('已有操作正在进行，请稍后重试');return;} busy.value=true;++generation;error.value='';try{await work();}catch(e){if(alive)error.value=e&&typeof e==='object'&&'message' in e?String(e.message):String(e);if(propagate)throw e;}finally{busy.value=false;} }
async function select(){await run(async()=>{const selected=await invoke<Status|null>('emby_select');if(selected){target.value=selected;plan.value=null;await refresh();}});}
async function prepare(action:string){await run(async()=>{plan.value=await invoke<Plan>('emby_plan',{id:crypto.randomUUID(),action});});}
async function receipt(){if(!plan.value)return;const id=plan.value.id;await run(async()=>{plan.value=await invoke<Plan|null>('emby_operation',{id});target.value=await invoke<Status|null>('emby_status');});}
async function apply(){if(!plan.value||plan.value.phase!=='planned')return;const reviewed=plan.value;await run(async()=>{try{target.value=await invoke<Status>('emby_apply',{id:reviewed.id,fingerprint:reviewed.fingerprint});}finally{try{plan.value=await invoke<Plan|null>('emby_operation',{id:reviewed.id});}catch{reviewed.phase='unverified';plan.value=reviewed;}}});}
async function control(start:boolean){if(busy.value)return;await run(async()=>{
 pending.value=start?'start':'stop';startFailed=false;
 try{service.value=await changeService(invoke,start,crypto.randomUUID());}
 catch(error){if(error instanceof ServiceControlFailure){service.value={...service.value,...error.observed};startFailed=error.observed.phase==='error';}throw error;}
 finally{pending.value=null;}
});}
async function afterRootsSaved(revision:number){await run(async()=>{
 startFailed=false;
 try{const result=await refreshAfterRootsSaved(invoke,revision,crypto.randomUUID(),{alive:()=>alive,onStart:()=>{pending.value='start';}});if(alive)service.value=result;}
 catch(error){if(alive&&error instanceof ServiceControlFailure){service.value={...service.value,...error.observed};startFailed=error.observed.phase==='error';}throw error;}
 finally{if(alive)pending.value=null;}
},true);}
async function pollStatus() {
 if(!alive)return;
 const token=generation;
 if(!busy.value)try {
  // Retry the original startup snapshot after a read failure. A service-only
  // poll cannot recover missing environment or legacy-component information.
  if(!initialized){await refresh();if(!alive)return;await refreshLegacy();if(!alive)return;}
  const next=await invoke<Service>('emby_service_status');
  if(!alive)return;
  const status=await invoke<Status|null>('emby_status');
  if(!alive)return;
  const checks=await invoke<IncrementalStatus>('incremental_status');
  if(alive && !busy.value && token===generation){initialized=true;service.value=startFailed&&next.phase==='stopped'?{...next,phase:'error'}:next;target.value=status;incremental.value=checks;if(checks.error)error.value=checks.error.message;ready.value=true;observedAt.value=new Date().toLocaleTimeString();}
 }catch(e){if(alive && token===generation){incremental.value={...incremental.value,phase:'unverified'};error.value='状态读取失败：'+(e&&typeof e==='object'&&'message' in e?String(e.message):String(e));}}
 if(alive)poll=setTimeout(pollStatus,2000);
}
onMounted(async()=>{
 await run(async()=>{await refresh();if(!alive)return;await refreshLegacy();if(!alive)return;const status=await invoke<Service>('emby_service_status');if(alive){initialized=true;service.value=status;ready.value=true;}});
 if(alive)void pollStatus();
});
onUnmounted(()=>{alive=false;++generation;if(poll)clearTimeout(poll);});
return {target,plan,service,pending,observedAt,incremental,environment,candidates,endpoint,busy,error,authorizationAvailable,ready,
 legacyComponents,legacyPlan,refreshLegacy,authorize,refresh,discover,connect,checkServer,chooseData,run,select,prepare,receipt,apply,control,afterRootsSaved,acceptService};
}
export const embyKey: InjectionKey<ReturnType<typeof useEmby>> = Symbol('emby-controller');
