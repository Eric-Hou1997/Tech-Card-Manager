<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
interface Status { target: string; installed: boolean; healthy: boolean; phase: string; issues: string[] }
interface Plan { id: string; fingerprint: string; action: string; target: string; files: string[]; legacy_patch: boolean }
interface Service { phase: string; error: { message: string } | null }
const target = ref<Status | null>(null), plan = ref<Plan | null>(null), service = ref<Service>({phase:'stopped',error:null});
const busy = ref(false), error = ref('');
let poll: ReturnType<typeof setInterval> | undefined;
let alive = true;
async function run(work: () => Promise<void>) { if(busy.value)return; busy.value=true;error.value='';try{await work();}catch(e){error.value=typeof e==='object'?JSON.stringify(e):String(e);}finally{busy.value=false;} }
async function select(){await run(async()=>{const selected=await invoke<Status|null>('emby_select');if(selected){target.value=selected;plan.value=null;}});}
async function prepare(action:string){await run(async()=>{plan.value=await invoke<Plan>('emby_plan',{id:crypto.randomUUID(),action});});}
async function apply(){if(!plan.value)return;const reviewed=plan.value;await run(async()=>{target.value=await invoke<Status>('emby_apply',{id:reviewed.id,fingerprint:reviewed.fingerprint});plan.value=null;});}
async function control(start:boolean){await run(async()=>{service.value=await invoke<Service>(start?'emby_start':'emby_stop',start?{id:crypto.randomUUID()}:{});});}
onMounted(()=>{poll=setInterval(async()=>{if(busy.value)return;try{const next=await invoke<Service>('emby_service_status');if(alive)service.value=next;}catch(e){if(alive)error.value=String(e);}},2000);});
onUnmounted(()=>{alive=false;if(poll)clearInterval(poll);});
</script>
<template>
<section aria-labelledby="emby-title">
  <h2 id="emby-title">Emby Web Card</h2>
  <p>选择 Emby 网页目录，检查维护计划后安装卡片。媒体 NFO 始终只读。</p>
  <button :disabled="busy || service.phase === 'running'" @click="select">选择 Emby 网页目录</button>
  <template v-if="target">
    <p class="path">{{ target.target }}</p>
    <p role="status">资源：{{ target.healthy ? '磁盘就绪' : target.installed ? '需要检查或修复' : '尚未安装' }} · 服务：{{ service.phase }}</p>
    <p>网页是否已加载及实际显示卡片，需要在 Emby 页面中确认。</p>
    <ul v-if="target.issues.length"><li v-for="issue in target.issues" :key="issue">{{ issue }}</li></ul>
    <div class="actions">
      <button :disabled="busy || service.phase === 'running'" @click="prepare('install')">安装</button>
      <button :disabled="busy || service.phase === 'running'" @click="prepare('update')">更新卡片与数据</button>
      <button :disabled="busy || service.phase === 'running'" @click="prepare('repair')">检查并修复</button>
      <button :disabled="busy || service.phase === 'running'" @click="prepare('remove')">移除</button>
      <button :disabled="busy || !target.healthy || service.phase === 'running'" @click="control(true)">启动服务</button>
      <button :disabled="busy" @click="control(false)">停止并禁用卡片</button>
    </div>
  </template>
  <article v-if="plan" class="maintenance" aria-labelledby="maintenance-title">
    <h3 id="maintenance-title">确认维护计划：{{ plan.action }}</h3>
    <p class="path">{{ plan.target }}</p>
    <p v-if="plan.legacy_patch">检测到历史注入标记；仅处理经过验证的标记内容。</p>
    <p>以下文件将备份后更新或移除；未列出的文件保持原状。</p>
    <ul><li v-for="file in plan.files" :key="file">{{ file }}</li></ul>
    <button :disabled="busy" @click="apply">确认执行此计划</button>
    <button :disabled="busy" @click="plan=null">取消</button>
  </article>
  <p v-if="error || service.error" role="alert">{{ error || service.error?.message }}</p>
</section>
</template>
<style scoped>.path{overflow-wrap:anywhere}.maintenance{border:1px solid #8aa4c2;border-radius:10px;padding:16px;margin-top:16px}</style>
