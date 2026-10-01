import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {ref,nextTick} from 'vue';
import {component,renderer} from './support/mount-vue.ts';
import * as maintenance from '../src/maintenance.ts';
import {withConfirmation} from '../src/confirmation.ts';
async function fixture(restored=false){
 const file=new URL('../src/LibraryPanel.vue',import.meta.url),source=await readFile(file,'utf8'),deps:Record<string,unknown>={};
 for(const path of source.matchAll(/from '([^']+\.(?:vue|png))'/g))deps[path[1]]={default:path[1].endsWith('.png')?'fixture.png':{render:()=>null}};
 const target=ref<any>({target:'/web',installed:true,healthy:true,requires_permission:false,issues:[]});
 let saved:any=restored?{id:'restored-remove',target:'/web',action:'remove',phase:'planned',fingerprint:'exact'}:null;
 const plan=ref<any>(saved),error=ref(''),calls:Array<[string,any]>=[];
 const control={confirm:true,defer:false,replyLoss:!restored,confirmError:false};let answer:(yes:boolean)=>void=()=>{},lostReceipt=false;
 const config={revision:0,locale:'zh-CN',roots:[]},view=()=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
 deps['@tauri-apps/api/core']={invoke:async(name:string,args:any)=>{
  calls.push([name,args]);
  if(name==='configuration')return config;if(name==='ui_state')return {revision:0,active_space:'movie',movie:view(),tv:view()};
  if(name==='manager_catalog'||name==='task_history')return [];if(name==='catalog_summary')return {roots_configured:true,total:0};
  if(name==='confirm_product_action'){if(control.confirmError)throw {code:'confirmation-dialog',message:'无法打开确认窗口'};if(control.defer)return await new Promise(resolve=>answer=resolve);return control.confirm;}
  if(name==='emby_status')return {...target.value};
  if(name==='emby_operation'){if(lostReceipt){lostReceipt=false;throw Error('receipt unavailable');}return saved?{...saved}:null;}
  if(name==='emby_repair'){saved={id:args.id,target:'/web',action:'repair-web',phase:'committed',fingerprint:'exact'};if(control.replyLoss){control.replyLoss=false;lostReceipt=true;throw Error('reply lost');}return {...target.value};}
  if(name==='emby_stop')return {phase:'stopped'};
  if(name==='emby_apply'){assert.equal(args.id,'restored-remove');assert.equal(args.fingerprint,'exact');saved.phase='committed';target.value={...target.value,installed:false};return {...target.value};}
  assert.fail('Unexpected IPC '+name);
 }};
 deps['@tauri-apps/api/event']={listen:async()=>()=>{}};
 for(const module of ['console','catalog','window-state'])deps['./'+module]=await import('../src/'+module+'.ts');
 deps['./baseline-layout']={installBaselineLayout:()=>()=>{}};deps['./maintenance']=maintenance;deps['./confirmation']={withConfirmation};deps['./legacy-migration']=await import('../src/legacy-migration.ts');
 const busy=ref(false);
 deps['./useEmby']={embyKey:Symbol(),useEmby:()=>({target,plan,legacyComponents:ref(null),legacyPlan:ref(null),service:ref({phase:'stopped'}),busy,error,ready:ref(true),pending:ref(null),observedAt:ref(''),refresh:async()=>{plan.value=saved?{...saved}:null;},run:async(work:()=>Promise<void>)=>{busy.value=true;error.value='';try{await work();}catch(e:any){error.value=e.message;}finally{busy.value=false;}}})};
 const Panel=await component(file,deps),{host,render}=renderer();let nodes:any[]=[];
 function collect(value:any){if(!value)return;if(Array.isArray(value)){for(const child of value)collect(child);return;}if(typeof value!=='object')return;nodes.push(value);if(Array.isArray(value.children))collect(value.children);else if(typeof value.children?.default==='function')collect(value.children.default());}
 const app=render.createApp({setup(){const draw=Panel.setup({},{expose(){},emit(){}}),cache:unknown[]=[];return ()=>{nodes=[];collect(draw({},cache));return null;};}});
 const settle=async()=>{for(let i=0;i<20;i++)await nextTick();};
 app.mount(host);await settle();nodes.find(n=>n.props?.id==='openSettings').props.onClick();await settle();
 return {app,settle,calls,control,error,answer:(yes:boolean)=>answer(yes),button:(action:string)=>nodes.find(n=>n.props?.['data-action']===action),toast:()=>nodes.find(n=>n.props?.id==='toast')?.children,writes:()=>calls.filter(([name])=>['emby_repair','emby_apply','emby_plan'].includes(name))};
}
test('maintenance confirmation failures show the structured native message',async()=>{
 const f=await fixture();
 try{
  f.control.confirmError=true;await f.button('rebuild-index').props.onClick();await f.settle();
  assert.equal(f.toast(),'无法打开确认窗口');assert.equal(f.writes().length,0);
 }finally{f.app.unmount();}
});
test('original maintenance buttons retain the failed operation across cancellation and retry',async()=>{
 const f=await fixture();let mounted=true;
 try{
  f.control.defer=true;const first=f.button('repair-web').props.onClick();await f.settle();
  await f.button('repair-web').props.onClick();assert.equal(f.calls.filter(([n])=>n==='confirm_product_action').length,1);assert.equal(f.writes().length,0);
  f.answer(true);await first;await f.settle();assert.equal(f.error.value,'receipt unavailable');assert.equal(f.writes().length,1);const id=f.writes()[0][1].id;
  f.control.defer=false;f.control.confirm=false;await f.button('repair-web').props.onClick();assert.equal(f.writes().length,1);
  f.control.confirm=true;await f.button('repair-web').props.onClick();await f.settle();assert.equal(f.error.value,'');assert.equal(f.writes().length,1);
  assert.ok(f.calls.filter(([n])=>n==='emby_operation').every(([,args])=>args.id===id));assert.equal(f.button('repair-web').props.disabled,false);
  f.control.defer=true;const late=f.button('repair-web').props.onClick();await f.settle();f.app.unmount();mounted=false;const before=f.calls.length;f.answer(true);await late;assert.equal(f.calls.length,before);
 }finally{if(mounted)f.app.unmount();}
});
test('a restored unfinished action cannot be replaced by another maintenance button',async()=>{
 const f=await fixture(true);
 try{
  await f.button('repair-web').props.onClick();await f.settle();assert.equal(f.error.value,'已有维护操作尚未确认，请先完成原操作。');assert.equal(f.writes().length,0);
  await f.button('disable-integration').props.onClick();await f.settle();assert.equal(f.error.value,'');assert.equal(f.writes().length,1);assert.equal(f.writes()[0][0],'emby_apply');assert.equal(f.writes()[0][1].id,'restored-remove');
 }finally{f.app.unmount();}
});
