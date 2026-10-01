<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch, nextTick, provide } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { CatalogSummary, AppError, Configuration, MediaItem, Space, Task, LibraryView, ManagerRow } from './contracts';

import logo from './assets/TCM_logo_letter_only.png';
import ConsolePanel from './ConsolePanel.vue';
import {servicePhase,servicePresentation} from './console';
const platform=/Win/i.test(navigator.platform)?'Windows':/Mac/i.test(navigator.platform)?'macOS':'Linux';
const updatedAt=ref('');
import CatalogList from './CatalogList.vue';
import {installBaselineLayout} from './baseline-layout';
const shell=ref<HTMLElement|null>(null);let releaseLayout:(()=>void)|undefined;
onMounted(()=>{if(shell.value)releaseLayout=installBaselineLayout(shell.value);});
onUnmounted(()=>releaseLayout?.());
import CatalogPreview from './CatalogPreview.vue';
import {catalogItems,matches,emptyMessage,type CatalogItem} from './catalog';
import SettingsDialog from './SettingsDialog.vue';
import SetupFlow from './SetupFlow.vue';
import LegacyDialog from './LegacyDialog.vue';
import {migrateLegacyCard,migrateLegacySystem,LegacyMigrationFailure,type LegacyAttempt,type LegacySystemAttempt} from './legacy-migration';
import {withConfirmation} from './confirmation';
import IncrementalPanel from './IncrementalPanel.vue';
import {maintainCard,restoredMaintenance,MaintenanceFailure,type MaintenanceAction,type MaintenanceAttempt} from './maintenance';
let maintenanceAttempt:MaintenanceAttempt|null=null;
let releasedMaintenance:string|null=null;
const confirmationPending=ref(false);
const maintenanceRunning=ref(false);
const toastText=ref(''),toastVisible=ref(false);let toastTimer:ReturnType<typeof setTimeout>|undefined;
const errorMessage=(error:unknown)=>error&&typeof error==='object'&&'message' in error?String(error.message):String(error);
function notify(message:string){toastText.value=message;toastVisible.value=true;clearTimeout(toastTimer);toastTimer=setTimeout(()=>{toastVisible.value=false;},4200);}
defineExpose({notify});
onUnmounted(()=>clearTimeout(toastTimer));
async function requestMaintenance(operation:MaintenanceAction){
 if(confirmationPending.value||maintenanceRunning.value||embyBusy.value||discoveryBusy.value||taskRunning.value)return;
 confirmationPending.value=true;
 try{await withConfirmation(invoke,operation,()=>confirmMaintenance(operation));}catch(e){if(!disposed)notify(errorMessage(e));}finally{confirmationPending.value=false;}
}
async function confirmMaintenance(operation:MaintenanceAction){
 if(disposed)return;
 maintenanceRunning.value=true;
 try{await emby.run(async()=>{
  if(operation==='rebuild-index')await invoke('rebuild_index',{id:crypto.randomUUID(),revision:configuration.value.revision});
  else{
   if(!maintenanceAttempt){
    const previous=emby.plan.value;
    maintenanceAttempt=(previous?.id!==releasedMaintenance?restoredMaintenance(previous):null)
     ??{id:crypto.randomUUID(),action:operation,target:target.value?.target??null};
   }
   if(maintenanceAttempt.action!==operation)throw new MaintenanceFailure('已有维护操作尚未确认，请先完成原操作。');
   const attempt=maintenanceAttempt;
   try{await maintainCard(invoke,attempt,()=>!disposed);if(disposed)return;maintenanceAttempt=null;}
   catch(error){if(error instanceof MaintenanceFailure&&error.releaseAttempt){releasedMaintenance=attempt.id;maintenanceAttempt=null;}throw error;}
  }
  if(disposed)return;
  notify(operation==='rebuild-index'?'操作已提交':'操作已完成并通过结果复核');
  await emby.refresh();if(!disposed)await refresh();
 });}finally{maintenanceRunning.value=false;}
}
import RootSettings from './RootSettings.vue';
function closeSettings(){settingsOpen.value=false;}

import LifecyclePanel from './LifecyclePanel.vue';
import DiagnosticsPanel from './DiagnosticsPanel.vue';
const diagnosticsBusy=ref(false),discoveryBusy=ref(false);
import AboutPanel from './AboutPanel.vue';
import {useEmby,embyKey} from './useEmby';
const emit=defineEmits<{ready:[]}>();
const emby=useEmby();provide(embyKey,emby);
const {target,service,busy:embyBusy,error:embyError,ready:embyReady}=emby;
const settingsOpen=ref(false),settingsMounted=ref(false),loading=ref(true);
watch(settingsOpen,value=>{if(value)settingsMounted.value=true;});
let setupShown=false;
function maybeOpenSetup(){if(!setupShown&&stateReady&&embyReady.value&&!legacyBlocked.value&&!summary.value.roots_configured){setupShown=true;settingsOpen.value=true;}}
watch(embyReady,maybeOpenSetup);
let initializing=false;let initializationRetry:ReturnType<typeof setTimeout>|undefined;
const summary=ref<CatalogSummary>({total:0,movie:0,tv:0,errors:0,displayable:0,web_eligible:0,episodes_excluded:0,generated_at:null,roots_configured:false,index_error:null});
const specFilter=computed({get:()=>view.value.spec_filter ?? (view.value.errors?'error':'all'),set:value=>{view.value.spec_filter=value;view.value.errors=false;view.value.offset=0;renderCatalog();}});
const taskRunning=computed(()=>tasks.value.some(task=>['running','requested'].includes(task.state)));
const legacyPatch=computed(()=>['stopped','error'].includes(service.value.phase)?target.value?.legacy_patch??null:null);
const systemLegacy=computed(()=>{
 if(!['stopped','error'].includes(service.value.phase))return null;
 const value=emby.legacyComponents.value;
 if(value&&(value.items.length||value.errors.length))return value;
 const previous=emby.legacyPlan.value;
 return previous&&previous.id!==releasedSystemAttempt&&['planned','running'].includes(previous.phase)
  ?{fingerprint:previous.reviewed,items:previous.items??['旧组件迁移未完成'],errors:[]}:null;
});
const legacyReview=computed(()=>{
 const patch=legacyPatch.value,system=systemLegacy.value;
 if(!patch&&!system)return null;
 return {fingerprint:system?JSON.stringify([patch?.fingerprint??null,system.fingerprint]):patch!.fingerprint,
  items:[...(system?.items??[]),...(system?.errors.map(error=>error.path?`${error.message}：${error.path}`:error.message)??[]),...(patch?.items??[])],unsafe_patch:patch?.unsafe_patch??false};
});
const unsafeLegacy=computed(()=>legacyPatch.value?.unsafe_patch?legacyPatch.value:null);
const legacyMigrating=ref(false);
let legacyAttempt:LegacyAttempt|null=null;
let systemAttempt:LegacySystemAttempt|null=null;
let releasedSystemAttempt:string|null=null;
let releasedLegacyAttempt:string|null=null;
const dismissedLegacy=ref<string|null>(null);
const legacyPromptVisible=computed(()=>!legacyMigrating.value&&!!legacyReview.value&&legacyReview.value.fingerprint!==dismissedLegacy.value);
function cancelLegacy(){if(legacyMigrating.value)return;dismissedLegacy.value=legacyReview.value?.fingerprint??null;notify(legacyAttempt||systemAttempt||['running','failed'].includes(emby.legacyPlan.value?.phase??'')?'旧版迁移未完成，请查看故障处理信息。':'已取消；旧版保持不变，新版服务未启动');}
function bannerAction(){if(legacyReview.value)dismissedLegacy.value=null;else settingsOpen.value=true;}
const legacyBlocked=computed(()=>!!legacyReview.value||(target.value?.issues.includes('legacy-patch-requires-plan')??false));
const phase=computed(()=>legacyMigrating.value?'migrating':servicePhase(service.value,emby.pending.value,legacyBlocked.value));
async function confirmLegacy(){
 if(disposed||legacyMigrating.value||embyBusy.value||unsafeLegacy.value)return;
 if(systemLegacy.value||systemAttempt){
  if(!systemAttempt){
   const previous=emby.legacyPlan.value;
   systemAttempt=previous&&previous.id!==releasedSystemAttempt&&['planned','running'].includes(previous.phase)
    ?{id:previous.id,reviewed:previous.reviewed,target:target.value?.target??null}
    :{id:crypto.randomUUID(),reviewed:systemLegacy.value!.fingerprint,target:target.value?.target??null};
  }
  const attempt=systemAttempt;let completed=false;legacyMigrating.value=true;
  try{await emby.run(async()=>{
   try{await migrateLegacySystem(invoke,attempt,()=>!disposed);if(disposed)return;systemAttempt=null;completed=true;dismissedLegacy.value=null;}
   catch(error){if(error instanceof LegacyMigrationFailure&&error.releaseAttempt){releasedSystemAttempt=attempt.id;systemAttempt=null;}throw error;}
   finally{if(!disposed){await emby.refresh();await emby.refreshLegacy();}}
  });}finally{legacyMigrating.value=false;}
  if(!disposed&&completed&&!legacyReview.value){
   if(target.value?.target===attempt.target&&target.value?.healthy&&!target.value.requires_permission)await emby.control(true);
   else settingsOpen.value=true;
  }
  return;
 }
 if(!legacyAttempt){
  if(!legacyPatch.value||!target.value)return;
  const previous=emby.plan.value;
  legacyAttempt=previous?.action==='adopt'&&previous.id!==releasedLegacyAttempt&&previous.target===target.value.target&&previous.legacy_review&&['planned','prepared'].includes(previous.phase)
   ?{id:previous.id,target:previous.target,reviewed:previous.legacy_review}
   :{id:crypto.randomUUID(),target:target.value.target,reviewed:legacyPatch.value.fingerprint};
 }
 const attempt=legacyAttempt;legacyMigrating.value=true;
 try{await emby.run(async()=>{
  try{const next=await migrateLegacyCard(invoke,attempt,()=>!disposed);if(disposed)return;emby.acceptService(next);legacyAttempt=null;dismissedLegacy.value=null;}
  catch(error){if(error instanceof LegacyMigrationFailure){if(error.releaseAttempt){releasedLegacyAttempt=attempt.id;legacyAttempt=null;}if(!disposed&&error.service)emby.acceptService(error.service);}throw error;}
  finally{if(!disposed){await emby.refresh();await emby.refreshLegacy();}}
 });}finally{legacyMigrating.value=false;}
}
const webHealthy=computed(()=>!!target.value?.healthy&&target.value.index_current===true);
const serviceView=computed(()=>servicePresentation(phase.value));
const serviceStop=computed(()=>serviceView.value.stop);
async function toggleService() {
 if(embyBusy.value||serviceView.value.busy)return;
 if(serviceStop.value){await emby.control(false);return;}
 let checked=false;
 await emby.run(async()=>{await emby.refreshLegacy();if(legacyReview.value)await emby.refresh();checked=true;});
 if(disposed||!checked)return;
 if(legacyReview.value){dismissedLegacy.value=null;return;}
 if(systemAttempt){await confirmLegacy();return;}
 if(legacyAttempt){await confirmLegacy();return;}
 if(!configuration.value.roots.length||!target.value||target.value.requires_permission){settingsOpen.value=true;return;}
 await emby.control(true);
}
async function copyPath() {
 const path=detail.value?.path;
 if(!path||disposed)return;
 await action(async()=>{
  try{await navigator.clipboard.writeText(path);if(!disposed)notify('路径已复制');}
  catch{if(!disposed)notify('无法复制路径');}
 });
}
const configuration = ref<Configuration>({ revision: 0, locale: 'zh-CN', roots: [] });
const space = ref<Space>('movie');
const emptyView=():LibraryView=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
const views=reactive<{movie:LibraryView;tv:LibraryView}>({movie:emptyView(),tv:emptyView()});
let stateReady=false;

watch(()=>configuration.value.roots,values=>{
 const ids=new Set(values.map(root=>root.id));
 for(const current of [views.movie,views.tv])current.roots=current.roots.filter(id=>ids.has(id));
},{deep:true});
const view = computed(() => views[space.value]);
const roots = computed(() => configuration.value.roots.filter(r => r.space === space.value));
const catalogRows=ref<CatalogItem[]>([]);
const catalogError=ref(''),renderedSearch=ref('');
let searchTimer:ReturnType<typeof setTimeout>|undefined;
function renderCatalog(){renderedSearch.value=view.value.search;restoreCurrent();}
function searchChanged(){clearTimeout(searchTimer);searchTimer=setTimeout(()=>{if(!disposed)renderCatalog();},200);}
const matchedRows=computed(()=>catalogRows.value.filter(row=>matches(row,space.value,renderedSearch.value,specFilter.value)));
const catalogEmpty=computed(()=>{
 return emptyMessage(catalogRows.value,space.value,renderedSearch.value,summary.value.index_error?.message);
});
const tasks = ref<Task[]>([]);
const detail = ref<MediaItem | null>(null);
const selectedPath=reactive<Record<Space,string>>({movie:'',tv:''});
const error = ref('');
watch([error,embyError],values=>{const message=values.find(Boolean);if(message)notify(message);});
const busy = ref(false);
let unlisten: UnlistenFn | undefined;
let configEvents:UnlistenFn|undefined;
let disposed = false;
let refreshToken = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let catalogPoll:ReturnType<typeof setTimeout>|undefined;
let refreshes=0,catalogLoadedAt=0;
function pollCatalog(){
 if(disposed)return;
 // v4.1.0 refreshes an idle catalog on the job poll, at most once per five
 // seconds. Task events still request an immediate refresh after changes.
 if(stateReady&&!taskRunning.value&&!refreshes&&Date.now()-catalogLoadedAt>=5000)void refresh();
 catalogPoll=setTimeout(pollCatalog,1800);
}
function report(e: unknown) {
  if (e && typeof e === 'object' && 'message' in e) {
    const failure = e as Partial<AppError>;
    error.value = `${failure.code ? failure.code+'：' : ''}${failure.message}${failure.path ? '\n' + failure.path : ''}`;
  } else error.value = String(e);
}
async function refresh(strict = false) {
  const token = ++refreshToken;
  ++refreshes;
  try {
    let result:ManagerRow[];
    try{result=await invoke<ManagerRow[]>('manager_catalog');}
    catch(e){
      if(!disposed&&token===refreshToken){
        catalogError.value=e&&typeof e==='object'&&'message' in e?String(e.message):String(e);
        detail.value=null;
      }
      throw e;
    }
    if(disposed||token!==refreshToken)return;
    catalogRows.value=catalogItems(result);catalogError.value='';catalogLoadedAt=Date.now();renderCatalog();
    const history = await invoke<Task[]>('task_history');
    const counts=await invoke<CatalogSummary>('catalog_summary');
    if (!disposed && token === refreshToken) {tasks.value=history;summary.value=counts;updatedAt.value=new Date().toLocaleTimeString();}
  } catch (e) {if(strict)throw e;if(!disposed && token===refreshToken)report(e);}
  finally{--refreshes;}
}
function scheduleRefresh() { clearTimeout(timer); timer = setTimeout(() => { void refresh(); }, 150); }
async function action(work: () => Promise<void>) {
  if (busy.value) return;
  busy.value = true; error.value = '';
  try { await work(); } catch (e) { report(e); } finally { busy.value = false; }
}
async function scan() {
  await action(async () => {
    await invoke<Task[]>('scan_library', {id:crypto.randomUUID(),revision:configuration.value.revision,space:space.value});
    await refresh();
  });
}
function inspect(item: MediaItem) {
 if(disposed||item.space!==space.value)return;
 // The original preview uses the same catalog snapshot as the selected row.
 detail.value=item;
 selectedPath[space.value]=item.path;
}
function restoreCurrent() {
 if(catalogError.value)return;
 const visible=matchedRows.value;
 const item=visible.find(item=>item.path===selectedPath[space.value])??visible[0];
 if(item)inspect(item);else detail.value=null;
}
function switchSpace(next: Space) {
 if(next===space.value)return;
 clearTimeout(searchTimer);space.value=next;renderCatalog();
}
function releaseListeners(){unlisten?.();configEvents?.();unlisten=configEvents=undefined;}
function acceptConfiguration(value:Configuration,refreshChanged=true){
 if(value.revision<configuration.value.revision)return;
 const rootsChanged=JSON.stringify(configuration.value.roots)!==JSON.stringify(value.roots);
 configuration.value=value;
 if(refreshChanged&&rootsChanged){detail.value=null;scheduleRefresh();}
}
async function initialize(){
 if(initializing || disposed)return;
 clearTimeout(initializationRetry);initializing=true;loading.value=true;stateReady=false;error.value='';releaseListeners();
 try{
  const offConfig=await listen<Configuration>('configuration-changed',e=>{if(!disposed)acceptConfiguration(e.payload);});if(disposed){offConfig();return;}configEvents=offConfig;
  const config=await invoke<Configuration>('configuration');
  if(disposed)return;
  acceptConfiguration(config,false);
  await nextTick();if(disposed)return;
  const off=await listen<Task>('task-changed',()=>{if(!disposed)scheduleRefresh();});if(disposed){off();return;}unlisten=off;
  await refresh(true);if(disposed)return;
  stateReady=true;loading.value=false;maybeOpenSetup();await nextTick();if(!disposed)emit('ready');
 }catch(e){releaseListeners();clearTimeout(timer);++refreshToken;if(!disposed){report(e);initializationRetry=setTimeout(()=>{void initialize();},2000);}}
 finally{initializing=false;}
}
onMounted(()=>{void initialize();pollCatalog();});
onUnmounted(()=>{disposed=true;stateReady=false;++refreshToken;clearTimeout(searchTimer);clearTimeout(timer);clearTimeout(catalogPoll);clearTimeout(initializationRetry);releaseListeners();});
</script>
<template>
 <div class="shell" ref="shell">
  <header class="top">
   <div class="brand"><img class="logo" alt="Tech Card Manager" :src="logo"><div class="brandText"><div class="titleLine"><h1>Tech Card Manager</h1><span class="version" id="version">v5.0.0</span></div><div class="sub">Emby Server 技术规格卡片</div></div></div>
   <div class="topActions"><div class="statusActions"><span class="statusPill" id="embyPill" :class="!embyReady?'':target?.installed||target?.details?.script_exists||configuration.roots.length?'ok':'warn'"><span class="dot"></span><span>{{!embyReady?'正在检测 Emby':target?.installed||target?.details?.script_exists||configuration.roots.length?'Emby 已检测':'Emby 待设置'}}</span></span><span class="statusPill" id="servicePill" :class="embyReady?serviceView.pillClass:''"><span class="dot"></span><span>{{serviceView.pill}}</span></span></div><div class="commandActions"><button class="btn" id="refreshLibrary" :disabled="loading||busy||taskRunning||discoveryBusy||!roots.length" @click="scan">刷新当前媒体库</button><button class="btn" id="openSettings" @click="settingsOpen=true">设置</button></div></div>
  </header>
  <div class="banner warn" id="productBanner" :class="{show:!loading&&(!!legacyReview||!summary.roots_configured||!webHealthy)}"><div id="productBannerText">{{legacyReview?'检测到旧版组件。新版服务保持停止，只有用户确认后才会迁移。':!summary.roots_configured?'尚未选择媒体目录。完成设置后才能建立 NFO 只读索引。':target?.details?.data_valid&&target.index_current===false&&!target.details.runtime_valid?'媒体目录与卡片索引不一致，程序正在重建；完成前不会显示过期卡片。':'Emby 网页卡片尚未设置或需要维护；NFO 只读索引不受影响。'}}</div><button class="btn" id="bannerAction" :data-target="legacyReview?'legacy':'settings'" @click="bannerAction">{{legacyReview?'查看迁移提示':!summary.roots_configured?'选择媒体目录':'打开设置'}}</button></div>
  <ConsolePanel :phase="phase" :last-started-at="service.last_started_at" :locale="configuration.locale" :loading="loading" :updated-at="emby.observedAt.value||updatedAt" :summary="summary" :web-version="target?.details?.script_version" :web-healthy="webHealthy" :runtime-valid="target?.details?.runtime_valid??false" :web-installed="target?.installed??false" :busy="embyBusy" :platform="platform" @toggle="toggleService" />
  <section class="section card catalogPanel">
   <div class="sectionTitle"><h2>NFO 管理器 <span class="badge">只读</span></h2><span class="muted" id="catalogCount">{{matchedRows.length}} 项</span></div>
   <div class="catalogTools"><div class="catalogTabs"><button class="btn catalogTab" id="catalogMovieTab" :class="{active:space==='movie'}" :aria-pressed="space==='movie'" @click="switchSpace('movie')">电影</button><button class="btn catalogTab" id="catalogTvTab" :class="{active:space==='tv'}" :aria-pressed="space==='tv'" @click="switchSpace('tv')">电视剧</button></div><input v-model="view.search" class="search" id="catalogSearch" aria-label="搜索片名、原标题、年份、IMDb ID 或路径" placeholder="搜索片名、原标题、年份、IMDb ID 或路径" @input="searchChanged"><select id="catalogSpecFilter" v-model="specFilter" class="select" aria-label="NFO 状态"><option value="all">全部状态</option><option value="ready">技术规格正常</option><option value="missing">缺少技术规格</option><option value="error">解析异常</option></select></div>
   <div class="catalogLayout">
    <CatalogList :rows="matchedRows" :space="space" :locale="configuration.locale" :selected="detail?.id??null" :expanded="view.expanded" :search="renderedSearch" :loading="loading" :empty="catalogEmpty" :error="catalogError" @inspect="inspect" @expanded="view.expanded=$event" />
    <CatalogPreview :item="detail" :locale="configuration.locale" :busy="busy" :unavailable="!!catalogError" @reveal="item=>action(async()=>{await invoke('reveal_item',{id:item.id});})" @copy="copyPath" />
   </div>
  </section>
 </div>
 <SettingsDialog v-if="settingsMounted" :visible="settingsOpen" @close="closeSettings">
  <SetupFlow :initialized="!loading&&embyReady" :emby-detected="target!==null" :roots-configured="summary.roots_configured" :web-configured="target?.healthy??false" />
  <div class="settingGroup"><h3>应用</h3><LifecyclePanel @notify="notify" /></div>
  <div class="settingGroup"><h3>设置媒体目录</h3><RootSettings :active="settingsOpen" :configuration="configuration" :blocked="embyBusy||maintenanceRunning||taskRunning" :after-save="emby.afterRootsSaved" @busy="discoveryBusy=$event" @changed="configuration=$event;detail=null;scheduleRefresh()" @notify="notify" /></div>
  <div class="settingGroup"><IncrementalPanel :active="settingsOpen" @notify="notify"><button class="btn primary" data-action="repair-web" :disabled="embyBusy||diagnosticsBusy||maintenanceRunning||discoveryBusy||taskRunning" @click="requestMaintenance('repair-web')">设置/维护网页卡片</button></IncrementalPanel></div>
  <details class="maintenance"><summary>维护与故障处理</summary><div class="muted" style="margin:9px 0">这些操作只在排查问题时使用，不属于日常工作流程。</div><DiagnosticsPanel :active="settingsOpen" :blocked="embyBusy||maintenanceRunning||busy||discoveryBusy||taskRunning" :task-running="taskRunning" :tasks="tasks" :errors="catalogRows.filter(row=>row.error).map(row=>({id:row.id,path:row.path,error:row.error!.message}))" @busy="diagnosticsBusy=$event" @notify="notify"><button class="btn" data-action="rebuild-index" :disabled="embyBusy||diagnosticsBusy||maintenanceRunning||discoveryBusy||taskRunning" @click="requestMaintenance('rebuild-index')">完整重建只读索引</button><button class="btn danger" data-action="disable-integration" :disabled="embyBusy||diagnosticsBusy||maintenanceRunning||discoveryBusy||taskRunning" @click="requestMaintenance('disable-integration')">恢复原生 Emby</button></DiagnosticsPanel></details>
  <AboutPanel :active="settingsOpen" @notify="notify" />
 </SettingsDialog>
 <LegacyDialog v-if="legacyReview" :visible="legacyPromptVisible" :items="legacyReview.items" :unsafe-patch="legacyReview.unsafe_patch" :pending="legacyMigrating" @cancel="cancelLegacy" @confirm="confirmLegacy" />
 <div class="toast" id="toast" :class="{show:toastVisible}">{{toastText}}</div>
</template>
