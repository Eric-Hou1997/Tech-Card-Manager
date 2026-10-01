import type {ReleaseCheck} from './contracts';
type Invoke=<T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
export interface CardUpdateState {busy:boolean;prompt:boolean;label:string;button:string;result:ReleaseCheck|null}
export function initialCardUpdate():CardUpdateState{return {busy:false,prompt:false,label:'尚未检查更新',button:'检查更新',result:null};}
export function retryLabel(data:{retry_at?:string|null}|undefined,locale:string):string {
 if(!data?.retry_at)return '';const when=new Date(data.retry_at);if(Number.isNaN(when.getTime()))return '';
 return (locale==='en-US'?' · Retry after ':' · ')+when.toLocaleString(locale)+(locale==='en-US'?'':' 后可重试');
}
export function cacheLabel(result:ReleaseCheck,locale:string):string {
 if(!result.source.includes('cache')||!result.checked_at)return '';const when=new Date(result.checked_at);if(Number.isNaN(when.getTime()))return '';
 return ' · '+(locale==='en-US'?'Last checked ':'上次检查于 ')+when.toLocaleString(locale);
}
export function createCardUpdate(invoke:Invoke,state:CardUpdateState,notify:(message:string)=>void,locale=()=> 'zh-CN') {
 let alive=true,opening=false;
 async function check(force=false){
  if(!alive||state.busy)return;
  state.busy=true;state.prompt=false;state.result=null;state.button='正在检查…';state.label='正在检查 GitHub 正式发布…';
  try{
   const result=await invoke<ReleaseCheck>('check_card_update',{force});if(!alive)return;
   state.result=result;
   const cached=cacheLabel(result,locale()),warning=result.warning?' · '+(locale()==='en-US'?'Using the last successful result: ':'使用上次成功结果：')+result.warning+retryLabel(result,locale()):'';
   state.label=(result.available?'最新版本 '+result.latest_version+' 可用':'已是最新版本 '+result.current_version)+cached+warning;
   state.button=result.available?'下载新版':'已是最新版本';
  }catch(error){if(!alive)return;const failure=error as {message?:string;retry_at?:string|null};state.result=null;state.label='检查失败：'+(failure?.message||String(error))+retryLabel(failure,locale());state.button='重试检查';}
  finally{if(alive)state.busy=false;}
 }
 async function activate(){if(!alive||state.busy)return;if(state.result?.available)state.prompt=true;else await check(true);}
 async function download(){
  if(!alive||opening)return;
  if(!state.result?.available||!state.result.candidate_id){notify('安装包地址尚未确认，请重新检查更新');return;}
  opening=true;try{await invoke('open_card_update',{candidateId:state.result.candidate_id});}catch(error){if(alive)notify((error as {message?:string})?.message||String(error));}finally{opening=false;}
 }
 return {check,activate,download,cancel(){state.prompt=false;},dispose(){alive=false;}};
}
