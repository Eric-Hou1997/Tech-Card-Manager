<script setup lang="ts">
import {computed,ref,shallowRef,onUnmounted,watch} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import type {LibraryDiagnostics,Task} from './contracts';
import MaintenanceOutput from './MaintenanceOutput.vue';
import {jobLine as originalJobLine,type ManagerJob} from './job-presentation';
interface Check<T> {state:string;error?:{code:string;message:string;path?:string};value?:T}
interface Integration {installed:boolean;healthy:boolean;target:string;details?:{script_exists:boolean;script_matches:boolean;script_version:string|null;data_exists:boolean;data_valid:boolean;runtime_valid:boolean}|null}
interface Service {phase:string;error?:{message:string}|null}
interface Snapshot {id:string;report:{version:string;library:LibraryDiagnostics;runtime:{emby:Check<Integration|null>;index_current:boolean;service:Check<Service>;job?:Check<ManagerJob|null>}}}
const props=defineProps<{active:boolean;blocked:boolean;taskRunning?:boolean;tasks:Task[];errors:Array<{id:string;path:string;error:string}>}>();
const emit=defineEmits<{busy:[value:boolean];notify:[message:string]}>();
const snapshot=shallowRef<Snapshot|null>(null),liveJob=shallowRef<ManagerJob|null>(null),busy=ref(false),diagnosing=ref(false);
let disposed=false,statusLoad:Promise<void>|undefined,jobLoad:Promise<void>|undefined,pollTimer:ReturnType<typeof setTimeout>|undefined;
function message(e:unknown){if(e&&typeof e==='object'&&'message' in e){const value=e as {code?:string;message:string;path?:string};return [value.code,value.message,value.path].filter(Boolean).join(' · ');}return String(e);}
let lastJobError='';
function acceptSnapshot(value:Snapshot){snapshot.value=value;liveJob.value=null;const check=value.report.runtime.job;const error=check?.state==='failed'&&check.error?message(check.error):'';if(error&&error!==lastJobError)emit('notify',error);lastJobError=error;}
async function run(exportFile=false){if(busy.value||props.blocked)return;busy.value=true;diagnosing.value=!exportFile;emit('busy',true);
 try {await statusLoad;if(disposed)return;const value=await invoke<Snapshot>('diagnose',{exportFile});if(disposed)return;acceptSnapshot(value);
 if(exportFile){const path=await invoke<string|null>('export_diagnostics',{id:value.id});if(!disposed&&path)emit('notify','操作已提交');}}
 catch(e){if(!disposed)emit('notify',message(e));}finally{busy.value=false;diagnosing.value=false;emit('busy',false);}
}
function loadStatus():Promise<void>{
 if(statusLoad)return statusLoad;
 statusLoad=(async()=>{try{const value=await invoke<Snapshot>('diagnose');if(!disposed&&!busy.value)acceptSnapshot(value);}catch(e){if(!disposed&&!busy.value)emit('notify',message(e));}finally{statusLoad=undefined;}})();
 return statusLoad;
}
function loadJob():Promise<void>{
 if(jobLoad)return jobLoad;
 jobLoad=(async()=>{try{const value=await invoke<ManagerJob|null>('manager_job');if(!disposed&&!busy.value)liveJob.value=value;}catch(e){if(!disposed&&!busy.value)emit('notify',message(e));}finally{jobLoad=undefined;}})();
 return jobLoad;
}
let pollGeneration=0;
async function pollStatus(generation:number){
 if(disposed||!props.active||generation!==pollGeneration)return;
 if(!busy.value){if(props.taskRunning)await loadJob();else if(!props.blocked)await loadStatus();}
 if(!disposed&&props.active&&generation===pollGeneration)pollTimer=setTimeout(()=>{void pollStatus(generation);},2000);
}
async function reveal(id:string){try{await invoke('reveal_item',{id});}catch(e){if(!disposed)emit('notify',message(e));}}
const environment=computed<Array<[string,string]>>(()=>{
 const report=snapshot.value?.report;if(!report)return [];
 const integration=report.runtime.emby,service=report.runtime.service;
 const details=integration.value?.details,web=integration.value?.target||'';
 const separator=web.includes('\\')?'\\':'/',clean=web.replace(/[\\/]$/,'');
 const root=/[\\/](?:system[\\/])?dashboard-ui$/i.test(clean)?clean.replace(/[\\/](?:system[\\/])?dashboard-ui$/i,''):'—';
 const absent=integration.state==='checked'&&!integration.value;
 const script=details?(details.script_exists?(details.script_matches?'正常 ':'内容异常 ')+(details.script_version||'未知'):'文件缺失'):absent?'文件缺失':'—';
 const index=details?(report.runtime.index_current?'有效':details.data_valid?'等待按当前目录重建':details.data_exists?'损坏':'缺失'):absent?'缺失':'—';
 return [['程序版本','v'+report.version],['服务状态',service.error?.message||service.value?.error?.message||({running:'服务运行中',stopped:'服务已关闭',failed:'服务异常'} as Record<string,string>)[service.value?.phase||'']||'—'],['网页入口注入',integration.state==='checked'?(integration.value?.installed?'正常':'缺失/版本不符'):'—'],['卡片脚本',script],['运行许可',details?(details.runtime_valid?'有效':'未启用/已过期'):absent?'未启用/已过期':'—'],['技术规格索引',index],['Emby 服务目录',root],['网页入口文件',web?clean+separator+'index.html':'—']];
});
const activeTask=computed(()=>props.tasks.find(task=>!['completed','failed','cancelled'].includes(task.state)));
const task=computed(()=>activeTask.value||props.tasks[0]);
const managerJob=computed(()=>liveJob.value??snapshot.value?.report.runtime.job?.value);
const jobLine=computed(()=>{
 if(diagnosing.value)return '运行中 · 运行诊断';
 if(managerJob.value)return originalJobLine(managerJob.value);
 if(!task.value)return '任务空闲';
 const value=task.value,name=value.force_parse?'完整重建只读索引':value.id.startsWith('refresh-')?'刷新媒体库':value.id.startsWith('scan-root-')?'目录级检查':value.id.startsWith('scan-space-')?'刷新当前媒体库':value.service_session?'自动增量检查':'刷新当前媒体库';
 const state=({requested:'运行中',running:'运行中',paused:'已暂停',interrupted:'执行中断',completed:'完成',failed:'失败',cancelled:'已取消'} as const)[value.state];
 return state+' · '+name+(value.failure?' · '+value.failure.message:'');
});
const jobLog=computed(()=>{
 const check=snapshot.value?.report.runtime.job;
 if(check?.state==='failed'&&check.error)return message(check.error);
 return managerJob.value?.log|| (task.value?[task.value.current_path,task.value.processed+' 项 · '+task.value.errors+' 个异常',task.value.failure?message(task.value.failure):''].filter(Boolean).join('\n'):'等待任务…');
});
watch(()=>props.active,active=>{const generation=++pollGeneration;clearTimeout(pollTimer);if(active)void pollStatus(generation);},{immediate:true});
onUnmounted(()=>{disposed=true;++pollGeneration;clearTimeout(pollTimer);emit('busy',false);});
</script>
<template>
 <div class="actions"><button class="btn" data-action="diagnose" :disabled="busy||blocked" @click="run()">运行诊断</button><button class="btn" data-action="export-diagnostics" :disabled="busy||blocked" @click="run(true)">导出诊断包</button><slot /></div>
 <MaintenanceOutput :environment="environment" :errors="errors" :job-line="jobLine" :job-log="jobLog" @reveal="reveal" />
</template>
