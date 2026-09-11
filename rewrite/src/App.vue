<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import LibraryPanel from './LibraryPanel.vue';
import UpdatePanel from './UpdatePanel.vue';
import MigrationPanel from './MigrationPanel.vue';
import EmbyPanel from './EmbyPanel.vue';
const product = 'TCM';
const status = ref('正在连接桌面核心…');
const busy = ref(false);
const rows = ref<{ name: string; result: string }[]>([]);
async function run(command: string, name: string) {
  if (busy.value) return;
  busy.value = true;
  try { rows.value.push({ name, result: JSON.stringify(await invoke(command), null, 2) }); }
  catch (error) { rows.value.push({ name, result: `未通过：${String(error)}` }); }
  finally { busy.value = false; }
}
onMounted(async () => {
  try {
    const nonce = crypto.randomUUID();
    const r = await invoke<{ product: string; os: string; arch: string; echo: string }>('runtime_probe', { nonce });
    if (r.echo !== nonce || r.product !== product) throw Error('IPC 响应不匹配');
    status.value = `${r.os} / ${r.arch} · Rust 通信已验证`;
    await nextTick();
    await invoke('frontend_ready');
  } catch (e) { status.value = `启动验证失败：${String(e)}`; }
});
</script>
<template>
  <main>
    <header><span class="mark">{{ product }}</span><div><h1>重写工作台</h1><p>读取流程迁移中 · NFO 始终只读</p></div></header>
    <p class="status" role="status">{{ status }}</p>
    <LibraryPanel />
    <UpdatePanel />
    <MigrationPanel :product="product" />
    <EmbyPanel />
    <section><h2>平台能力</h2><p>目录检查只读；凭据测试使用独立的临时条目并清理。网络测试仅发出固定地址的读取请求。</p>
      <p v-if="busy" role="status">正在执行验证，请稍候…</p>
      <div class="actions">
        <button :disabled="busy" @click="run('directory_probe', '目录读取')">选择目录并检查</button>
        <button :disabled="busy" @click="run('storage_probe', '隔离文件读写')">验证测试文件读写</button>
        <button :disabled="busy" @click="run('credential_probe', '系统凭据')">验证系统凭据</button>
        <button :disabled="busy" @click="run('network_probe', '网络请求')">验证网络请求</button>
      </div>
    </section>
    <section><h2>执行记录</h2><p v-if="!rows.length">选择一项能力开始验证。安装、更新、真实媒体业务另行验收。</p><article v-for="(row, i) in rows" :key="i"><h3>{{ row.name }}</h3><pre>{{ row.result }}</pre></article></section>
    <footer><p>菜单提供恢复窗口和退出。此工程不连接正式更新通道。</p><button :disabled="busy" @click="invoke('quit_probe')">退出验证工程</button></footer>
  </main>
</template>
<style>
:root{font-family:system-ui,sans-serif;color:#1b2636;background:#f3f5f9;color-scheme:light dark}body{margin:0}main{max-width:880px;margin:auto;padding:32px}header{display:flex;gap:20px;align-items:center}.mark{font-weight:800;background:#174e9c;color:white;border-radius:16px;padding:20px}h1{font-size:26px;margin:0}h2{font-size:18px}p{line-height:1.6;color:#57677b}section{background:white;border:1px solid #dbe2ec;border-radius:14px;padding:20px;margin:20px 0}.status{padding:12px;background:#e3edf9;border-radius:8px;color:#174e9c}.actions{display:flex;flex-wrap:wrap;gap:12px}button{font:inherit;padding:10px 16px;border:1px solid #abc0da;border-radius:8px;color:inherit;background:transparent;cursor:pointer}button:disabled{opacity:.5;cursor:wait}button:focus-visible{outline:3px solid #4e8adb}pre{white-space:pre-wrap;overflow-wrap:anywhere}footer{display:flex;gap:16px;align-items:center;justify-content:space-between}@media(max-width:600px){main{padding:16px}header{gap:12px}footer{display:block}}@media(prefers-color-scheme:dark){:root{color:#dce5ef;background:#111923}p{color:#a4b3c5}section{background:#1c2735;border-color:#344356}.status{background:#193655;color:#bddbff}}
</style>
