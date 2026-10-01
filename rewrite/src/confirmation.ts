import type {MaintenanceAction} from './maintenance';
type Invoke=<T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
export async function withConfirmation(invoke:Invoke,action:MaintenanceAction|'save-roots',work:()=>Promise<void>):Promise<boolean>{
 const accepted=await invoke<boolean>('confirm_product_action',{action});
 if(accepted!==true)return false;
 await work();return true;
}
