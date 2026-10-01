import type {AppError,Settings,SettingsOperation,OperationResult} from './contracts';
export interface LifecycleStatus {settings:Settings;native_autostart:boolean|null;background_mode:string;tray_available:boolean;closing:boolean;error:AppError|null}
export type LifecycleInvoke=<T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
const defaults=():Settings=>({revision:0,close_action:'quit',launch_at_login:false,start_hidden:false});
export class LifecycleSettings {
 status:LifecycleStatus|null=null;
 settings=defaults();
 baseline:Settings|null=null;
 busy=false;
 error='';
 closing=false;
 alive=true;
 verified=false;
 pending:{id:string;settings:Settings}|null=null;
 private refreshRequested=false;
 invoke:LifecycleInvoke;
 identity:()=>string;
 constructor(invoke:LifecycleInvoke,identity=()=>crypto.randomUUID()){this.invoke=invoke;this.identity=identity;}
 get dirty(){return this.baseline!==null&&JSON.stringify(this.settings)!==JSON.stringify(this.baseline);}
 report(e:unknown){if(!this.alive)return;this.error=e&&typeof e==='object'&&'message' in e?String(e.message):String(e);}
 async refresh(){if(!this.alive)return;this.refreshRequested=true;await this.refreshWhenIdle();}
 private async refreshWhenIdle(){
  if(!this.refreshRequested||!this.alive||this.busy||this.dirty||this.pending)return;
  this.refreshRequested=false;await this.read();
 }
 async read(){
  if(this.busy||!this.alive)return;this.busy=true;this.error='';this.verified=false;
  try{const value=await this.invoke<LifecycleStatus>('lifecycle_status');if(!this.alive)return;this.status=value;this.baseline={...value.settings};this.settings={...value.settings};this.closing=value.closing;this.pending=null;this.verified=true;}
  catch(e){this.report(e);}finally{this.busy=false;void this.refreshWhenIdle();}
 }
 async apply(){
  if(this.busy||!this.alive||(!this.dirty&&!this.pending))return;this.busy=true;this.error='';this.verified=false;
  let terminalFailure=false;
  try{
   // Resolve the previous operation under the native lifecycle gate first.
   // Even a draft changed back to the old baseline may need a second write
   // after a committed operation whose reply was lost.
   while(this.pending||this.dirty){
    if(!this.pending)this.pending={id:this.identity(),settings:{...this.settings}};
    const result=await this.invoke<SettingsOperation>('lifecycle_apply',this.pending);
    if(!this.alive)return;
    if(result.phase!=='committed'){
     if(['failed','interrupted'].includes(result.phase)){this.pending=null;terminalFailure=true;}
     throw result.error??Error('系统设置结果尚未确认，请重新读取后核对');
    }
    this.baseline={...result.desired};this.settings={...this.settings,revision:result.desired.revision};this.pending=null;
   }
   const status=await this.invoke<LifecycleStatus>('lifecycle_status');if(!this.alive)return;
   this.status=status;this.verified=true;this.closing=status.closing;if(JSON.stringify(status.settings)!==JSON.stringify(this.baseline))this.error='系统设置已在其他位置更改，请重新读取后核对';
  }catch(e){
   // A transport error is ambiguous. Only a persisted terminal failure permits
   // a fresh ID; otherwise preserve the exact input for the next user action.
   const failed=this.pending;
   if(this.alive&&failed){
    try{const receipt=await this.invoke<OperationResult>('operation_result',{id:failed.id});
     if(this.alive&&receipt.kind==='lifecycle'&&['failed','interrupted'].includes(receipt.result.phase)){this.pending=null;terminalFailure=true;}
    }catch{/* Keep an unresolved receipt, including a not-yet-persisted call. */}
   }
   // The original settings error path rereads the effective switches. Keep an
   // ambiguous operation intact, but do not display a rejected login setting.
   if(this.alive&&terminalFailure){
    try{const status=await this.invoke<LifecycleStatus>('lifecycle_status');if(this.alive){this.status=status;this.baseline={...status.settings};this.settings={...status.settings};this.closing=status.closing;}}
    catch{/* The original failure remains actionable if status is also unavailable. */}
   }
   this.report(e);
  }finally{this.busy=false;void this.refreshWhenIdle();}
 }
 async window(command:'background_window'|'quit_probe'){
  if(this.busy||!this.alive||this.dirty||this.closing)return;this.busy=true;this.error='';
  try{await this.invoke(command);}catch(e){this.report(e);}finally{this.busy=false;void this.refreshWhenIdle();}
 }
 dispose(){this.alive=false;this.refreshRequested=false;}
}
