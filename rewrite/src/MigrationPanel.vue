<script setup lang="ts">
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { MigrationPlan, MigrationReceipt } from './contracts';
const props=defineProps<{product:'ITM'|'TCM'}>();
const plan=ref<MigrationPlan|null>(null),receipt=ref<MigrationReceipt|null>(null),busy=ref(false),error=ref('');
async function inspect(kind:string){if(busy.value)return;busy.value=true;error.value='';try{plan.value=await invoke<MigrationPlan|null>('migration_plan',{id:crypto.randomUUID(),sourceKind:kind});receipt.value=null;}catch(e){error.value=JSON.stringify(e);}finally{busy.value=false;}}
async function apply(){if(!plan.value||busy.value)return;busy.value=true;error.value='';try{receipt.value=await invoke<MigrationReceipt>('migration_apply',{id:plan.value.id,fingerprint:plan.value.fingerprint});plan.value=null;}catch(e){error.value=JSON.stringify(e);}finally{busy.value=false;}}
</script>
<template>
<section aria-labelledby="migration-title">
<h2 id="migration-title">旧版数据迁移</h2>
<p>先检查所选目录和迁移范围，再导入。原数据保持原状，未完成的旧任务需重新预演和确认。</p>
<div class="actions">
<template v-if="props.product==='ITM'"><button :disabled="busy" @click="inspect('itm-manager')">选择旧 Manager 数据</button><button :disabled="busy" @click="inspect('itm-engine')">选择旧业务数据</button></template>
<template v-else><button :disabled="busy" @click="inspect('tcm-portable')">选择旧便携版目录</button><button :disabled="busy" @click="inspect('tcm-state')">选择旧 Emby 索引数据</button></template>
</div>
<p v-if="busy" role="status">正在核对或导入数据，请稍候…</p>
<article v-if="plan">
<h3>核对迁移计划</h3><p class="path">{{ plan.source }}</p><p>{{ plan.files.length }} 个文件，{{ plan.roots.length }} 个媒体根目录</p>
<ul><li v-for="root in plan.roots" :key="root.path" class="path">{{ root.path }} · {{ root.space || '待指定类型' }} · {{ root.state }}</li></ul>
<details><summary>文件清单</summary><ul><li v-for="file in plan.files" :key="file.relative">{{ file.relative }} · {{ file.bytes }} B</li></ul></details>
<ul><li v-for="warning in plan.warnings" :key="warning">{{ warning }}</li></ul>
<button :disabled="busy" @click="apply">确认导入这份快照</button><button :disabled="busy" @click="plan=null">取消</button>
</article>
<article v-if="receipt" role="status"><h3>快照已导入</h3><p>已保留 {{ receipt.imported_files }} 个文件的原始字节。业务适配与使用结果仍需逐项核对。</p><p v-if="receipt.pending_roots.length">{{ receipt.pending_roots.length }} 个目录需要恢复连接、配置映射或指定类型。</p><p>刷新媒体工作台以读取导入后的目录配置。</p></article>
<p v-if="error" role="alert">{{ error }}</p>
</section>
</template>
<style scoped>.path{overflow-wrap:anywhere}</style>
