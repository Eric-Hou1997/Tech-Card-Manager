<script setup lang="ts">
import {computed,onMounted,onUnmounted,ref,watch} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import type {IncrementalSettings,OperationResult} from './contracts';
const props=defineProps<{active:boolean}>();
const emit=defineEmits<{notify:[message:string]}>();
const saved=ref<IncrementalSettings|null>(null),interval=ref(60),busy=ref(false),error=ref('');
const dirty=computed(()=>saved.value!==null&&interval.value!==saved.value.interval_seconds);
const valid=computed(()=>Number.isInteger(interval.value)&&interval.value>=30&&interval.value<=86400);
let alive=true;
onUnmounted(()=>{alive=false;});
let pending:{id:string;value:IncrementalSettings}|null=null;
function report(e:unknown){if(!alive)return;error.value=e&&typeof e==='object'&&'message' in e?String(e.message):String(e);emit('notify',error.value);}
function code(e:unknown){return e&&typeof e==='object'&&'code' in e?String(e.code):'';}
async function reload(){if(busy.value||pending)return;busy.value=true;error.value='';try{const value=await invoke<IncrementalSettings>('incremental_settings');if(!alive)return;saved.value=value;interval.value=saved.value.interval_seconds;pending=null;}catch(e){report(e);}finally{busy.value=false;}}
async function submit(attempt:{id:string;value:IncrementalSettings}){
 try{return await invoke<IncrementalSettings>('save_incremental_settings',attempt);}
 catch(error){
  try{const receipt=await invoke<OperationResult>('operation_result',{id:attempt.id});if(receipt.kind==='incremental-settings')return receipt.result;}
  catch(readError){if(code(readError)==='operation-not-found')pending=null;}
  throw error;
 }
}
function accept(result:IncrementalSettings,attempt:IncrementalSettings){
 saved.value=result;if(interval.value===attempt.interval_seconds)interval.value=result.interval_seconds;pending=null;
}
async function save(){
 if(!alive||busy.value||!saved.value||(!dirty.value&&!pending)||!valid.value)return;
 busy.value=true;error.value='';const requested=interval.value;
 try{
  if(pending){const previous=pending,result=await submit(previous);if(!alive)return;accept(result,previous.value);if(requested===result.interval_seconds){emit('notify','增量检查周期已保存');return;}}
  if(!saved.value)return;
  const attempt={id:crypto.randomUUID(),value:{revision:saved.value.revision,interval_seconds:requested}};pending=attempt;
  const result=await submit(attempt);if(!alive)return;accept(result,attempt.value);emit('notify','增量检查周期已保存');
 }catch(e){report(e);if(alive&&saved.value)interval.value=saved.value.interval_seconds;}finally{busy.value=false;}
}
onMounted(reload);
watch(()=>props.active,active=>{if(active)void reload();});
</script>
<template>
 <h3>增量检查</h3><div class="intervalActions"><label class="muted">检查周期　<select id="interval" v-model.number="interval" class="select" :disabled="busy||!saved" @change="save"><option :value="30">30 秒</option><option :value="60">1 分钟</option><option :value="300">5 分钟</option><option :value="900">15 分钟</option><option :value="3600">1 小时</option></select></label><slot /></div>
 <div class="muted" style="margin-top:8px">服务停止或程序退出后不会继续检查。</div>
</template>
