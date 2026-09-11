<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AppError, CatalogPage, Configuration, MediaItem, Space, Task, TaskState, LibraryView, UiState, UiReceipt, TvPage, TvRow } from './contracts';

const configuration = ref<Configuration>({ revision: 0, locale: 'zh-CN', roots: [] });
const space = ref<Space>('movie');
const emptyView=():LibraryView=>({search:'',errors:false,roots:[],selected:[],expanded:[],offset:0,sort:'title',descending:false});
const views=reactive<{movie:LibraryView;tv:LibraryView}>({movie:emptyView(),tv:emptyView()});
let savedRevision=0, stateReady=false, saving=false;
let queuedState:UiState|null=null;
let failedSave:{id:string;value:UiState}|null=null;
const saveError=ref('');
async function flushState(){
 if(saving||failedSave)return;saving=true;
 try{while(queuedState){const value=queuedState;queuedState=null;value.revision=savedRevision;const id=crypto.randomUUID();
  try{const receipt=await invoke<UiReceipt>('save_ui_state',{id,value});savedRevision=receipt.revision;saveError.value='';}
  catch(e){failedSave={id,value};saveError.value=typeof e==='object'?JSON.stringify(e):String(e);break;}
 }}finally{saving=false;}
}
async function retryState(){if(!failedSave)return;try{const receipt=await invoke<UiReceipt>('save_ui_state',failedSave);savedRevision=receipt.revision;failedSave=null;saveError.value='';await flushState();}catch(e){saveError.value=JSON.stringify(e);}}
watch(()=>({space:space.value,movie:views.movie,tv:views.tv}),()=>{
 if(!stateReady)return;
 queuedState=JSON.parse(JSON.stringify({revision:savedRevision,active_space:space.value,movie:views.movie,tv:views.tv}));void flushState();
},{deep:true});

const view = computed(() => views[space.value]);
const roots = computed(() => configuration.value.roots.filter(r => r.space === space.value));
const page = ref<CatalogPage>({ total: 0, items: [] });
const tvPage=ref<TvPage>({total:0,rows:[]});
const tasks = ref<Task[]>([]);
const detail = ref<MediaItem | null>(null);
const error = ref('');
const busy = ref(false);
let unlisten: UnlistenFn | undefined;
let failures: UnlistenFn | undefined;
let disposed = false;
let refreshToken = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
function report(e: unknown) {
  if (e && typeof e === 'object' && 'code' in e) {
    const failure = e as AppError;
    error.value = `${failure.code}：${failure.message}${failure.path ? '\n' + failure.path : ''}`;
  } else error.value = String(e);
}
async function refresh() {
  const token = ++refreshToken;
  try {
    if(space.value==='tv') {
      const result=await invoke<TvPage>('tv_catalog',{view:JSON.parse(JSON.stringify(view.value))});
      if(!disposed&&token===refreshToken){tvPage.value=result;page.value={total:result.total,items:[]};}
    }else{
      const result=await invoke<CatalogPage>('browse',{space:space.value,view:JSON.parse(JSON.stringify(view.value))});
      if(!disposed&&token===refreshToken)page.value=result;
    }
    const history = await invoke<Task[]>('task_history');
    if (!disposed && token === refreshToken) tasks.value = history;
  } catch (e) { if (!disposed) report(e); }
}
function scheduleRefresh() { clearTimeout(timer); timer = setTimeout(() => { void refresh(); }, 150); }
async function action(work: () => Promise<void>) {
  if (busy.value) return;
  busy.value = true; error.value = '';
  try { await work(); } catch (e) { report(e); } finally { busy.value = false; }
}
async function addRoot() {
  await action(async () => {
    const result = await invoke<Configuration | null>('add_library_root', { space: space.value, operationId: crypto.randomUUID() });
    if (result) configuration.value = result;
  });
}
async function scan() {
  await action(async () => {
    await invoke<Task>('scan_library', { request: { operation_id: crypto.randomUUID(), space: space.value, root_ids: [...view.value.roots] } });
    await refresh();
  });
}
async function control(task: Task, state: TaskState) {
  await action(async () => {
    await invoke<Task>('task_control', { request: { operation_id: crypto.randomUUID(), task_id: task.id, state } });
    await refresh();
  });
}
async function inspect(item: MediaItem) { await action(async () => { detail.value = await invoke<MediaItem>('inspector', { id: item.id }); }); }
function toggleExpanded(id:string){const values=new Set(views.tv.expanded);if(values.has(id))values.delete(id);else values.add(id);views.tv.expanded=[...values];void refresh();}
async function toggleTv(row:TvRow){await action(async()=>{const ids=await invoke<string[]>('tv_members',{id:row.id});const selected=new Set(views.tv.selected);const remove=ids.every(id=>selected.has(id));for(const id of ids){if(remove)selected.delete(id);else selected.add(id);}views.tv.selected=[...selected];await refresh();});}
function tvName(row:TvRow){if(row.kind==='orphan')return '未归属节目';if(row.kind==='season')return row.name==='unknown'?'未标注季':'第 '+row.name+' 季';return row.name||row.item?.path||'异常条目';}
function switchSpace(next: Space) { space.value = next; detail.value = null; void refresh(); }
onMounted(async () => {
  try {
    configuration.value = await invoke<Configuration>('configuration');
    const state=await invoke<UiState>('ui_state');savedRevision=state.revision;space.value=state.active_space;
    Object.assign(views.movie,emptyView(),state.movie);Object.assign(views.tv,emptyView(),state.tv);
    await nextTick();stateReady=true;
    const off = await listen<Task>('task-changed', scheduleRefresh);
    if (disposed) off(); else unlisten = off;
    const offFailure = await listen<AppError>('worker-failed', e => report(e.payload));
    if (disposed) offFailure(); else failures = offFailure;
    if (!disposed) await refresh();
  } catch (e) { if (!disposed) report(e); }
});
onUnmounted(() => { disposed = true; ++refreshToken; clearTimeout(timer); unlisten?.(); failures?.(); });
</script>
<template>
  <section class="library-panel">
    <h2>媒体库 · 只读迁移工作台</h2>
    <p>选择根目录并明确勾选本次扫描范围。索引写入本应用独立数据目录，媒体 NFO 保持只读。</p>
    <div class="actions" aria-label="媒体空间">
      <button :aria-pressed="space === 'movie'" @click="switchSpace('movie')">Movie</button>
      <button :aria-pressed="space === 'tv'" @click="switchSpace('tv')">TV</button>
      <button :disabled="busy" @click="addRoot">添加根目录</button>
    </div>
    <p v-if="!roots.length">当前空间还没有配置根目录。</p>
    <label v-for="root in roots" :key="root.id" class="root"><input v-model="view.roots" type="checkbox" :value="root.id" />{{ root.path }}</label>
    <button :disabled="busy || !view.roots.length" @click="scan">扫描勾选的 {{ view.roots.length }} 个根目录</button>
    <p v-if="busy" role="status">正在处理…</p>
    <pre v-if="error" role="alert">{{ error }}</pre>
    <p v-if="saveError" role="alert">界面状态尚未保存：{{ saveError }} <button @click="retryState">重试保存</button></p>
    <div class="search-row">
      <input v-model="view.search" aria-label="搜索标题、年份、IMDb ID 或路径" placeholder="搜索标题、年份、IMDb ID 或路径" @input="view.offset = 0; scheduleRefresh()" />
      <label><input v-model="view.errors" type="checkbox" @change="view.offset = 0; refresh()" />仅异常</label>
    </div>
    <div class="actions"><label>排序 <select v-model="view.sort" @change="view.offset=0;refresh()"><option value="title">名称</option><option value="year">年份</option><option value="path">路径</option><option value="status">状态</option></select></label><label><input v-model="view.descending" type="checkbox" @change="view.offset=0;refresh()">降序</label><button :disabled="!view.selected.length" @click="view.selected=[]">清除选择</button></div>
    <p>{{ page.total }} 个条目 · 已选 {{ view.selected.length }} 项</p>
    <div v-if="space==='movie'" class="table-scroll"><table><thead><tr><th>选择</th><th>名称</th><th>年份</th><th>类型</th><th>IMDb</th><th>状态</th></tr></thead><tbody>
      <tr v-for="item in page.items" :key="item.id"><td><input v-model="view.selected" type="checkbox" :value="item.id" :aria-label="'选择 ' + (item.title || item.path)" /></td><td><button class="item-name" @click="inspect(item)">{{ item.title || item.path }}</button></td><td>{{ item.year }}</td><td>{{ item.kind }}</td><td>{{ item.imdb }}</td><td>{{ item.error ? '读取异常' : '已索引' }}</td></tr>
    </tbody></table></div>
    <div v-else class="table-scroll"><table><thead><tr><th>节目／季／集</th><th>年份</th><th>IMDb</th><th>数量</th></tr></thead><tbody>
      <tr v-for="row in tvPage.rows" :key="row.id"><td><div class="tv-node" :style="{paddingInlineStart:(row.depth*20)+'px'}">
        <button v-if="row.expandable" class="expander" :aria-expanded="view.expanded.includes(row.id)" :aria-label="(view.expanded.includes(row.id)?'折叠 ':'展开 ')+tvName(row)" @click="toggleExpanded(row.id)">{{ view.expanded.includes(row.id)?'▾':'▸' }}</button><span v-else class="expander-space"></span>
        <span class="badge">{{ row.item?.error?'异常':row.kind==='season'?'季':row.kind==='series'?'节目':row.kind==='orphan'?'待归属':'集' }}</span>
        <input type="checkbox" :checked="row.member_count>0&&row.selected_count===row.member_count" :indeterminate="row.selected_count>0&&row.selected_count<row.member_count" :disabled="busy||!row.member_count" :aria-label="'选择 '+tvName(row)" @change="toggleTv(row)">
        <button v-if="row.item" class="item-name" @click="inspect(row.item)">{{ tvName(row) }}</button><span v-else>{{ tvName(row) }}</span>
      </div></td><td>{{ row.item?.year }}</td><td>{{ row.item?.imdb }}</td><td>{{ row.selected_count }} / {{ row.member_count }}</td></tr>
    </tbody></table></div>
    <p v-if="space==='tv'">季复选框选择该季全部已索引剧集，包括筛选或分页未显示的剧集。展开、选择和当前分页分别保存。</p>
    <p v-if="!page.total">没有符合条件的已索引条目。</p>
    <div class="actions"><button :disabled="view.offset === 0" @click="view.offset = Math.max(0, view.offset - 100); refresh()">上一页</button><button :disabled="view.offset + 100 >= page.total" @click="view.offset += 100; refresh()">下一页</button></div>
    <aside v-if="detail" class="inspector"><h3>{{ detail.title || '异常条目' }} · Inspector</h3><p>{{ detail.year }} · {{ detail.imdb }} · {{ detail.kind }}</p><pre>{{ detail.path }}</pre><pre v-if="detail.error" role="alert">{{ detail.error.code }}：{{ detail.error.message }}</pre>
      <button :disabled="busy" @click="action(async () => { await invoke('reveal_item', { id: detail!.id }); })">在文件管理器中定位</button>
      <h4>Technical Specs</h4><dl><template v-for="(values, field) in detail.specs" :key="field"><dt>{{ field }}</dt><dd v-for="(value, index) in values" :key="index">{{ value }}</dd></template></dl>
      <h4>根标签与归属</h4><ul><li v-for="(tag, index) in detail.tags" :key="index">{{ tag.value }} · {{ tag.ownership }}<span v-if="tag.engine"> · {{ tag.engine }}</span></li></ul>
      <button @click="detail = null">关闭检查器</button>
    </aside>
    <h3>当前空间任务</h3><article v-for="task in tasks.filter(t => t.space === space)" :key="task.id"><p>{{ task.state }} · {{ task.processed }} 项 · {{ task.errors }} 个异常</p><p class="task-id">{{ task.id }} · {{ task.locale }}</p><pre v-if="task.current_path">{{ task.current_path }}</pre><pre v-if="task.failure" role="alert">{{ task.failure.code }}：{{ task.failure.message }}\n{{ task.failure.path }}</pre><button v-if="task.errors" @click="view.errors=true;view.offset=0;refresh()">查看异常条目</button>
      <div class="actions"><button v-if="['requested', 'running'].includes(task.state)" :disabled="busy" @click="control(task, 'paused')">暂停</button><button v-if="['paused', 'interrupted'].includes(task.state)" :disabled="busy" @click="control(task, 'requested')">恢复</button><button v-if="!['completed', 'failed', 'cancelled'].includes(task.state)" :disabled="busy" @click="control(task, 'cancelled')">取消</button></div>
    </article>
  </section>
</template>
<style scoped>
.tv-node{display:flex;align-items:center;gap:8px;white-space:nowrap}.expander{padding:2px;width:28px;min-width:28px}.expander-space{width:28px;min-width:28px}.badge{font-size:12px;font-weight:600;border:1px solid #65768b66;border-radius:4px;padding:2px 5px}.root{display:flex;gap:8px;margin:12px 0;overflow-wrap:anywhere}.search-row{display:flex;gap:12px;align-items:center;margin-top:20px}.search-row>input{min-width:0;flex:1}.search-row label{white-space:nowrap}input{font:inherit;padding:8px}.table-scroll{overflow:auto}table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:8px;border-bottom:1px solid #65768b44}.item-name{text-align:left;min-width:120px;max-width:300px;overflow-wrap:anywhere}.inspector{padding:16px;margin-top:20px;border:1px solid #65768b66;border-radius:10px}.task-id{font-size:12px;overflow-wrap:anywhere}dt{font-weight:600}dd{margin:4px 0 8px 16px}article{border-top:1px solid #65768b44;margin-top:12px}button[aria-pressed=true]{background:#174e9c;color:white}
</style>
