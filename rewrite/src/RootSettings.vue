<script setup lang="ts">
import {ref,watch,onMounted,onUnmounted} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import type {Configuration,MediaFolder,FolderSettings,FolderReceipt,DiscoveredLibrary,OperationResult} from './contracts';
import {withConfirmation} from './confirmation';
const props=defineProps<{configuration:Configuration;active?:boolean;blocked?:boolean;afterSave:(revision:number)=>Promise<void>}>();
const emit=defineEmits<{changed:[configuration:Configuration];notify:[message:string];busy:[value:boolean]}>();
const clone=<T,>(value:T):T=>JSON.parse(JSON.stringify(value));
const baseline=ref<FolderSettings|null>(null),draft=ref<FolderSettings>({revision:0,folders:[]}),online=ref<Record<string,boolean>>({});
const path=ref(''),busy=ref(false),error=ref(''),message=ref(''),confirm=ref(false);
let alive=true,reading=false,loadPending=false,lastDiscoveryKey='',pending:{id:string;settings:FolderSettings}|null=null;
let poll:ReturnType<typeof setTimeout>|undefined;
function report(e:unknown){error.value=e&&typeof e==='object'&&'message' in e?[String(e.message),'path' in e?e.path:''].filter(Boolean).join(' · '):String(e);emit('notify',error.value);}
async function run(work:()=>Promise<void>){if(busy.value||!alive)return;busy.value=true;error.value='';try{await work();}catch(e){if(alive)report(e);}finally{busy.value=false;void drainLoad();}}
function discoveredRows(libraries:DiscoveredLibrary[]){return libraries.filter(row=>row.spaces.length&&(row.local_path||row.server_path));}
function discoveryKey(libraries:DiscoveredLibrary[]){return discoveredRows(libraries).map(row=>key(row.local_path||row.server_path)).sort().join('|');}
function mergeDiscovery(libraries:DiscoveredLibrary[],initial=false){
 const next=discoveryKey(libraries);if(!initial&&(!next||next===lastDiscoveryKey))return;
 for(const library of discoveredRows(libraries)){const value=library.local_path||library.server_path;if(!draft.value.folders.some(row=>key(row.path)===key(value)))add(value,'auto',library.name,library.spaces.length>1?'mixed':library.spaces[0]==='tv'?'tv':'movies');}
 lastDiscoveryKey=next;message.value=initial?'':'发现结果已加入，请确认后保存';
}
async function read(){
 const result=await invoke<{settings:FolderSettings;online:Record<string,boolean>;discovered?:DiscoveredLibrary[];discovery_error?:{message:string;path?:string}|null;roots_configured?:boolean}>('folder_settings');if(!alive)return;
 // A migration notification can arrive while the initial snapshot is in flight.
 // Let the queued read initialize the editor from the committed configuration.
 if(loadPending&&result.settings.revision<props.configuration.revision)return;
 if(baseline.value&&result.settings.revision<baseline.value.revision)return;
 if(result.discovery_error)report(result.discovery_error);
 online.value=result.online;const discovered=result.discovered||[];
 if(!baseline.value){draft.value=clone(result.settings);baseline.value=clone(result.settings);lastDiscoveryKey=discoveryKey(discovered);if(!(result.roots_configured??result.settings.folders.length>0))mergeDiscovery(discovered,true);return;}
 draft.value.revision=result.settings.revision;baseline.value=clone(result.settings);mergeDiscovery(discovered);
}
async function load(){if(!alive)return;loadPending=true;await drainLoad();}
async function drainLoad(){if(!loadPending||reading||busy.value||!alive)return;loadPending=false;reading=true;try{await read();}catch(e){if(alive)report(e);}finally{reading=false;void drainLoad();}}
function schedulePoll(){
 clearTimeout(poll);poll=undefined;
 if(alive&&props.active)poll=setTimeout(async()=>{if(!alive||!props.active)return;await load();schedulePoll();},2000);
}
watch(()=>props.active,value=>{if(value)void load();schedulePoll();});
onMounted(()=>{void load();schedulePoll();});onUnmounted(()=>{alive=false;loadPending=false;clearTimeout(poll);emit('busy',false);});
watch(()=>props.configuration.revision,value=>{if(value!==baseline.value?.revision)void load();});
function key(value:string){const path=value.replace(/[\\/]+$/,'');return /Win/i.test(navigator.platform)?path.toLocaleLowerCase():path;}
function add(value=path.value,source:MediaFolder['source']='manual',name='',kind:MediaFolder['kind']='auto'){
 value=value.trim().replace(/^"|"$/g,'');if(!value){report('请输入媒体目录路径');return;}
 if(draft.value.folders.some(row=>key(row.path)===key(value))){report('这个目录已经在列表中');return;}
 draft.value.folders.push({id:crypto.randomUUID(),path:value,name:name||value.split(/[\\/]/).filter(Boolean).pop()||value,kind,source,enabled:true});message.value='有未保存的修改';
}
function addManual(){add();path.value='';}
async function choose(){await run(async()=>{const value=await invoke<string|null>('choose_library_root');if(value&&alive)add(value);});}
function remove(id:string){draft.value.folders=draft.value.folders.filter(row=>row.id!==id);message.value='有未保存的修改';}
async function discover(){if(busy.value||!alive)return;emit('busy',true);try{await run(async()=>{const libraries=await invoke<DiscoveredLibrary[]>('emby_libraries');if(alive)mergeDiscovery(libraries);});}finally{if(alive)emit('busy',false);}}
async function review(){if(busy.value||confirm.value)return;if(!baseline.value){report('正在读取媒体目录…');void load();return;}if(!draft.value.folders.some(row=>row.enabled)){report('至少启用一个媒体目录');return;}confirm.value=true;try{await withConfirmation(invoke,'save-roots',save);}catch(e){report(e);}finally{confirm.value=false;}}
function errorCode(error:unknown){return error&&typeof error==='object'&&'code' in error?String(error.code):'';}
const sameRows=(left:FolderSettings,right:FolderSettings)=>JSON.stringify(left.folders)===JSON.stringify(right.folders);
async function submit(submitted:{id:string;settings:FolderSettings}):Promise<FolderReceipt>{
 try{return await invoke<FolderReceipt>('save_media_folders',submitted);}
 catch(error){
  // A returned store error means the worker finished. Query its transaction
  // before releasing the ID; transport/worker failures remain ambiguous.
  if(alive&&errorCode(error)&&errorCode(error)!=='folders-worker'){
   try{const receipt=await invoke<OperationResult>('operation_result',{id:submitted.id});
    if(receipt.kind==='folders')return receipt.result;
   }catch(readError){if(alive&&errorCode(readError)==='operation-not-found')pending=null;}
   if(errorCode(error)==='configuration-conflict')loadPending=true;
  }
  throw error;
 }
}
function acceptSave(result:FolderReceipt,submitted:FolderSettings){
 const revision=Math.max(draft.value.revision,result.settings.revision);
 if(sameRows(draft.value,submitted)&&revision===result.settings.revision)draft.value=clone(result.settings);
 else draft.value.revision=revision;
 if(!baseline.value||result.settings.revision>=baseline.value.revision)baseline.value=clone(result.settings);
 pending=null;
}
async function save(){await run(async()=>{
 // Freeze only this confirmation. Edits made while a reply is in flight remain
 // unsaved and must not be silently included in the follow-up service action.
 const requested=clone(draft.value);let result:FolderReceipt|null=null;
 if(pending){
  const previous=pending;
  try{result=await submit(previous);if(!alive)return;acceptSave(result,previous.settings);}
  catch(error){if(!alive)return;if(pending||sameRows(requested,previous.settings)||errorCode(error)==='configuration-conflict')throw error;}
  if(result&&(!sameRows(requested,previous.settings)||requested.revision>result.settings.revision))result=null;
  requested.revision=draft.value.revision;
 }
 if(!result){
  pending={id:crypto.randomUUID(),settings:requested};const submitted=pending;
  result=await submit(submitted);if(!alive)return;acceptSave(result,submitted.settings);
 }
 message.value=sameRows(draft.value,result.settings)?'目录设置已保存':'有未保存的修改';
 emit('notify','媒体目录已保存');emit('changed',result.configuration);
 await props.afterSave(result.configuration.revision);if(alive)await read();
});}
async function scan(row:MediaFolder){await run(async()=>{await invoke('scan_media_folder',{id:crypto.randomUUID(),revision:baseline.value?.revision,folderId:row.id});message.value='已提交目录级检查';});}
const kinds={auto:'自动识别',movies:'电影',tv:'电视剧',mixed:'电影/电视剧'};
</script>
<template>
 <div class="rootInput"><input id="manualRootPath" v-model="path" placeholder="例如 D:\Movies 或 \\NAS\TV" @keydown.enter.prevent="addManual"><button id="chooseLibraryRoot" class="btn" @click="choose">选择文件夹</button><button id="addLibraryRoot" class="btn" @click="addManual">添加</button></div>
 <div id="rootEditor" class="rootEditor"><div v-if="!baseline&&!draft.folders.length" class="muted">正在读取媒体目录…</div><div v-else-if="!draft.folders.length" class="catalogEmpty">尚未选择媒体目录。添加电影或电视剧目录后才能建立只读索引。</div>
 <div v-for="(row,index) in draft.folders" :key="row.id" class="row"><div class="rowHead"><label style="display:flex;align-items:center;gap:9px"><input v-model="row.enabled" type="checkbox" :data-root-toggle="index" @change="message='有未保存的修改'"><strong>{{row.name||'媒体目录'}}</strong></label><div class="actions"><span class="badge" :class="online[row.id]===false?'warn':online[row.id]===true?'ok':''"><span class="dot"></span>{{online[row.id]===false?'离线/不可达':online[row.id]===true?'在线':'等待检测'}}</span><button class="btn" :disabled="!row.enabled||online[row.id]===false" :data-scan-root="row.path" @click="scan(row)">检查此目录</button><button class="btn" :data-root-remove="index" @click="remove(row.id)">移除</button></div></div><div class="path">{{row.path}}</div><div class="muted">{{row.source==='auto'?'Emby 发现':'手动添加'}} · {{kinds[row.kind]}} · {{row.enabled?'参与索引':'已停用'}}</div></div></div>
 <div class="actions" style="margin-top:10px"><button class="btn" :disabled="busy||blocked" data-action="discover-roots" @click="discover">从 Emby 发现目录</button><button id="saveLibraryRoots" class="btn primary" @click="review">保存目录</button><span id="rootEditorState" class="muted">{{message}}</span></div>
</template>
