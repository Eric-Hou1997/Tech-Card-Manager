import test from 'node:test';
import assert from 'node:assert/strict';
import {reactive,nextTick} from 'vue';
import {component,renderer} from './support/mount-vue.ts';
import * as presentation from '../src/job-presentation.ts';
test('diagnostic results come from durable job state and export never replaces that state',async()=>{
 let job:any={running:false,action:'discover-roots',message:'完成',exit_code:0,log:'原任务日志'};
 let release:()=>void=()=>{};let exportError=false;
 const calls:any[]=[],notices:any[]=[],Output={render:()=>null};
 const Panel=await component(new URL('../src/DiagnosticsPanel.vue',import.meta.url),{
  './MaintenanceOutput.vue':{default:Output},'./job-presentation':presentation,
  '@tauri-apps/api/core':{invoke:async(name:string,args:any)=>{
   calls.push([name,args]);
   if(name==='export_diagnostics'){if(exportError)throw Error('export denied');return '/temporary/diagnostic.zip';}
   assert.equal(name,'diagnose');
   if(args?.exportFile===false){await new Promise<void>(resolve=>release=resolve);job={running:false,action:'diagnose',message:'完成',exit_code:0,log:'来自后端的持久化诊断输出'};}
   return {id:'snapshot',report:{version:'5.0.0',library:{roots:[],errors:[]},runtime:{emby:{state:'checked',value:null},service:{state:'checked',value:{phase:'stopped'}},job:{state:'checked',value:job}}}};
  }}
 });
 const props=reactive({active:true,blocked:false,tasks:[],errors:[]});
 const {host,render}=renderer();let vnode:any;
 const app=render.createApp({setup(){const draw=Panel.setup(props,{expose(){},emit:(...values:any[])=>notices.push(values)}),cache:unknown[]=[];return ()=>{vnode=draw({$slots:{}},cache);return null;};}});
 const settle=async()=>{for(let n=0;n<15;n++)await nextTick();};
 function find(match:(node:any)=>boolean,value:any=vnode):any{if(match(value))return value;for(const child of Array.isArray(value?.children)?value.children:[]){const result=find(match,child);if(result)return result;}}
 const output=()=>find(node=>node?.type===Output).props;
 const click=(action:string)=>find(node=>node?.props?.['data-action']===action).props.onClick();
 try{
  app.mount(host);await settle();
  await click('export-diagnostics');await settle();
  assert.equal(output()['job-log'],'原任务日志');assert.equal(output()['job-line'],presentation.jobLine(job));
  assert.ok(notices.some(value=>value[0]==='notify'&&value[1]==='操作已提交'));
  exportError=true;await click('export-diagnostics');await settle();
  assert.equal(output()['job-log'],'原任务日志');assert.equal(output()['job-line'],presentation.jobLine(job));
  assert.ok(notices.some(value=>value[0]==='notify'&&value[1]==='export denied'));
  const run=click('diagnose');await settle();await click('diagnose');
  assert.equal(calls.filter(([,args])=>args?.exportFile===false).length,1);
  assert.equal(output()['job-line'],'运行中 · 运行诊断');release();await run;await settle();
  assert.equal(output()['job-log'],job.log);assert.equal(output()['job-line'],presentation.jobLine(job));
  props.active=false;await settle();props.active=true;await settle();
  assert.equal(output()['job-log'],job.log);
 }finally{app.unmount();}
});
test('actual diagnostics panel passes persisted discovery output to the original log region',async()=>{
 let job:any={running:true,action:'discover-roots',message:'运行中',started_at:'first',log:'正在读取\n/media/电影'},error:any=null;
 const notices:any[]=[];
 const Output={render:()=>null};
 const Panel=await component(new URL('../src/DiagnosticsPanel.vue',import.meta.url),{
  './MaintenanceOutput.vue':{default:Output},'./job-presentation':presentation,
  '@tauri-apps/api/core':{invoke:async(name:string)=>{assert.equal(name,'diagnose');return {id:'snapshot',report:{version:'5.0.0',library:{},runtime:{emby:{state:'checked',value:null},service:{state:'checked',value:{phase:'running'}},index_current:false,job:error?{state:'failed',error}:{state:'checked',value:job}}}};}}
 });
 const props=reactive({active:true,blocked:false,tasks:[],errors:[]});let vnode:any;
 const {host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup(props,{expose(){},emit:(...args:any[])=>notices.push(args)}),cache:unknown[]=[];return ()=>{vnode=draw({$slots:{}},cache);return null;};}});
 function output(value:any=vnode):any{if(value?.type===Output)return value;for(const child of Array.isArray(value?.children)?value.children:[]){const found=output(child);if(found)return found;}}
 const settle=async()=>{for(let n=0;n<15;n++)await nextTick();};
 try{
  app.mount(host);
  await settle();assert.equal(output().props['job-line'],'运行中 · 发现媒体目录 · 运行中');assert.equal(output().props['job-log'],job.log);
  job={...job,running:false,exit_code:1,message:'任务已取消',log:job.log+'\n错误：任务已取消'};
  props.active=false;await settle();props.active=true;await settle();assert.equal(output().props['job-line'],presentation.jobLine(job));assert.equal(output().props['job-log'],job.log);
  job={...job,action:'',message:'',exit_code:0};props.active=false;await settle();props.active=true;await settle();assert.equal(output().props['job-line'],'任务空闲');assert.equal(output().props['job-log'],job.log);
  job={running:false,action:'',log:'\ufeff過去の記録\r\n中断在此'};props.active=false;await settle();props.active=true;await settle();
  assert.equal(output().props['job-line'],'任务空闲');assert.equal(output().props['job-log'],job.log);
  error={code:'history-integrity',message:'Historical diagnostic log failed its original checksum',path:'logs/job.log'};
  const reopen=async()=>{props.active=false;await settle();props.active=true;await settle();};
  await reopen();assert.equal(output().props['job-log'],'history-integrity · '+error.message+' · logs/job.log');
  assert.equal(notices.filter(value=>value[0]==='notify').length,1);await reopen();assert.equal(notices.filter(value=>value[0]==='notify').length,1);
  error=null;await reopen();assert.equal(output().props['job-log'],job.log);
 }finally{app.unmount();}
});

test('task polling reads only the lightweight persistent log and resumes diagnostics after the gate clears',async()=>{
 const calls:string[]=[];const Output={render:()=>null};
 const Panel=await component(new URL('../src/DiagnosticsPanel.vue',import.meta.url),{
  './MaintenanceOutput.vue':{default:Output},'./job-presentation':presentation,
  '@tauri-apps/api/core':{invoke:async(name:string)=>{calls.push(name);if(name==='manager_job')return {running:true,action:'scan-space',message:'运行中',log:'只读任务日志'};assert.equal(name,'diagnose');return {id:'snapshot',report:{version:'5.0.0',library:{},runtime:{emby:{state:'checked',value:null},service:{state:'checked',value:{phase:'stopped'}},index_current:false}}};}},
 });
 const props=reactive({active:false,blocked:true,taskRunning:true,tasks:[],errors:[]}),{host,render}=renderer();
 const app=render.createApp({setup(){const draw=Panel.setup(props,{expose(){},emit(){}}),cache:unknown[]=[];return ()=>draw({$slots:{}},cache);}});
 const settle=async()=>{for(let n=0;n<12;n++)await nextTick();};
 try{app.mount(host);props.active=true;await settle();assert.deepEqual(calls,['manager_job']);
  props.active=false;props.taskRunning=false;await settle();props.active=true;await settle();assert.deepEqual(calls,['manager_job']);
  props.active=false;await settle();props.blocked=false;props.active=true;await settle();assert.deepEqual(calls,['manager_job','diagnose']);
 }finally{app.unmount();}
});
