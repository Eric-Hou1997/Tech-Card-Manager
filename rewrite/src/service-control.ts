import type {ServiceObservation} from './console';
type Invoke=<T>(name:string,args?:Record<string,unknown>)=>Promise<T>;
function message(error:unknown):string{return error&&typeof error==='object'&&'message' in error?String(error.message):String(error);}
export class ServiceControlFailure extends Error {
 readonly observed:ServiceObservation;
 constructor(error:unknown,observed:ServiceObservation){super(message(error));this.observed=observed;}
}
export async function changeService(invoke:Invoke,start:boolean,id:string):Promise<ServiceObservation>{
 const expected=(phase:string)=>start?phase==='running'||phase==='starting':phase==='stopped';
 let observed:ServiceObservation;
 try {observed=await invoke<ServiceObservation>(start?'emby_start':'emby_stop',start?{id}:{});}
 catch(error){
  // Resolve an uncertain reply with a read; never repeat a start/stop mutation.
  try {observed=await invoke<ServiceObservation>('emby_service_status');}
  catch{throw new ServiceControlFailure(error,{phase:'unverified',error:{message:message(error)}});}
  if(!expected(observed.phase))throw new ServiceControlFailure(error,observed.phase==='stopped'&&start?{...observed,phase:'error'}:observed);
 }
 if(!expected(observed.phase)||observed.error)throw new ServiceControlFailure(observed.error?.message||'服务操作未通过结果复核。',observed);
 return observed;
}

export async function refreshAfterRootsSaved(invoke:Invoke,revision:number,id:string,options:{alive?:()=>boolean;onStart?:()=>void}={}):Promise<ServiceObservation>{
 const observed=await invoke<ServiceObservation>('emby_service_status');
 if(options.alive&&!options.alive())throw Error('操作已取消');
 if(observed.phase==='stopped'){
  options.onStart?.();
  return changeService(invoke,true,id);
 }
 if(observed.phase!=='running')throw new ServiceControlFailure('服务状态尚未确认，请稍后重试',observed);
 try{await invoke('refresh_libraries',{revision});}
 catch(error){throw new ServiceControlFailure(error,observed);}
 return observed;
}
