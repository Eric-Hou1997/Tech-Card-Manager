<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AppError, CatalogPage, Configuration, MediaItem, Space, Task, TaskState } from './contracts';

const configuration = ref<Configuration>({ revision: 0, locale: 'zh-CN', roots: [] });
const space = ref<Space>('movie');
const views = reactive({ movie: { search: '', errors: false, roots: [] as string[], selected: [] as string[], offset: 0 }, tv: { search: '', errors: false, roots: [] as string[], selected: [] as string[], offset: 0 } });
const view = computed(() => views[space.value]);
const roots = computed(() => configuration.value.roots.filter(r => r.space === space.value));
const page = ref<CatalogPage>({ total: 0, items: [] });
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
    const result = await invoke<CatalogPage>('catalog', { query: { space: space.value, search: view.value.search, only_errors: view.value.errors, offset: view.value.offset, limit: 100 } });
    if (!disposed && token === refreshToken) page.value = result;
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
function switchSpace(next: Space) { space.value = next; detail.value = null; void refresh(); }
onMounted(async () => {
  try {
    configuration.value = await invoke<Configuration>('configuration');
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
    <div class="search-row">
      <input v-model="view.search" aria-label="搜索标题、年份、IMDb ID 或路径" placeholder="搜索标题、年份、IMDb ID 或路径" @input="view.offset = 0; scheduleRefresh()" />
      <label><input v-model="view.errors" type="checkbox" @change="view.offset = 0; refresh()" />仅异常</label>
    </div>
    <p>{{ page.total }} 个条目 · 已选 {{ view.selected.length }} 项</p>
    <div class="table-scroll"><table><thead><tr><th>选择</th><th>名称</th><th>年份</th><th>类型</th><th>IMDb</th><th>状态</th></tr></thead><tbody>
      <tr v-for="item in page.items" :key="item.id"><td><input v-model="view.selected" type="checkbox" :value="item.id" :aria-label="'选择 ' + (item.title || item.path)" /></td><td><button class="item-name" @click="inspect(item)">{{ item.title || item.path }}</button></td><td>{{ item.year }}</td><td>{{ item.kind }}</td><td>{{ item.imdb }}</td><td>{{ item.error ? '读取异常' : '已索引' }}</td></tr>
    </tbody></table></div>
    <p v-if="!page.items.length">没有符合条件的已索引条目。</p>
    <div class="actions"><button :disabled="view.offset === 0" @click="view.offset = Math.max(0, view.offset - 100); refresh()">上一页</button><button :disabled="view.offset + 100 >= page.total" @click="view.offset += 100; refresh()">下一页</button></div>
    <aside v-if="detail" class="inspector"><h3>{{ detail.title || '异常条目' }} · Inspector</h3><p>{{ detail.year }} · {{ detail.imdb }} · {{ detail.kind }}</p><pre>{{ detail.path }}</pre><pre v-if="detail.error" role="alert">{{ detail.error.code }}：{{ detail.error.message }}</pre>
      <button :disabled="busy" @click="action(async () => { await invoke('reveal_item', { id: detail!.id }); })">在文件管理器中定位</button>
      <h4>Technical Specs</h4><dl><template v-for="(values, field) in detail.specs" :key="field"><dt>{{ field }}</dt><dd v-for="(value, index) in values" :key="index">{{ value }}</dd></template></dl>
      <h4>根标签与归属</h4><ul><li v-for="(tag, index) in detail.tags" :key="index">{{ tag.value }} · {{ tag.ownership }}<span v-if="tag.engine"> · {{ tag.engine }}</span></li></ul>
      <button @click="detail = null">关闭检查器</button>
    </aside>
    <h3>当前空间任务</h3><article v-for="task in tasks.filter(t => t.space === space)" :key="task.id"><p>{{ task.state }} · {{ task.processed }} 项 · {{ task.errors }} 个异常</p><p class="task-id">{{ task.id }} · {{ task.locale }}</p><pre v-if="task.current_path">{{ task.current_path }}</pre>
      <div class="actions"><button v-if="['requested', 'running'].includes(task.state)" :disabled="busy" @click="control(task, 'paused')">暂停</button><button v-if="['paused', 'interrupted'].includes(task.state)" :disabled="busy" @click="control(task, 'requested')">恢复</button><button v-if="!['completed', 'failed', 'cancelled'].includes(task.state)" :disabled="busy" @click="control(task, 'cancelled')">取消</button></div>
    </article>
  </section>
</template>
<style scoped>
.root{display:flex;gap:8px;margin:12px 0;overflow-wrap:anywhere}.search-row{display:flex;gap:12px;align-items:center;margin-top:20px}.search-row>input{min-width:0;flex:1}.search-row label{white-space:nowrap}input{font:inherit;padding:8px}.table-scroll{overflow:auto}table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:8px;border-bottom:1px solid #65768b44}.item-name{text-align:left;min-width:120px;max-width:300px;overflow-wrap:anywhere}.inspector{padding:16px;margin-top:20px;border:1px solid #65768b66;border-radius:10px}.task-id{font-size:12px;overflow-wrap:anywhere}dt{font-weight:600}dd{margin:4px 0 8px 16px}article{border-top:1px solid #65768b44;margin-top:12px}button[aria-pressed=true]{background:#174e9c;color:white}
</style>
