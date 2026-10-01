<script setup lang="ts">
import {reactive,onMounted,onUnmounted,watch} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import {listen,type UnlistenFn} from '@tauri-apps/api/event';
import type {AppError} from './contracts';
import {LifecycleSettings} from './lifecycle-settings';
import LanguagePicker from './LanguagePicker.vue';
const model=reactive(new LifecycleSettings(invoke));
const emit=defineEmits<{notify:[message:string]}>();
let off:UnlistenFn|undefined,offClosing:UnlistenFn|undefined,offConfiguration:UnlistenFn|undefined;
let reconnecting=false;
watch(()=>model.error||model.status?.error?.message,value=>{if(value)emit('notify',value);});
async function save(){await model.apply();if(!model.error&&!model.status?.error&&model.verified)emit('notify','应用设置已保存');}
function releaseListeners(){off?.();offClosing?.();offConfiguration?.();off=offClosing=offConfiguration=undefined;}
async function reconnect(){
 if(model.busy||reconnecting||!model.alive)return;reconnecting=true;
 releaseListeners();let subscriptionError:unknown;
 try {
  // Subscribe before reading so a completed migration cannot fall into the
  // gap between an old snapshot and listener registration.
  const changed=await listen('configuration-changed',()=>{void model.refresh();});
  if(!model.alive){changed();return;}offConfiguration=changed;
  const a=await listen<AppError>('lifecycle-error',event=>{if(model.alive){model.report(event.payload);model.closing=false;}});
  if(!model.alive){a();return;}off=a;
  const b=await listen<boolean>('lifecycle-closing',event=>{if(model.alive)model.closing=event.payload;});
  if(!model.alive){b();return;}offClosing=b;
 }catch(e){releaseListeners();subscriptionError=e;}finally{reconnecting=false;}
 await model.read();if(subscriptionError)model.report(subscriptionError);
}
onMounted(reconnect);
onUnmounted(()=>{model.dispose();releaseListeners();});
</script>
<template><div class="appSettingsGrid">
<LanguagePicker />
<div class="key">登录启动</div><label><input id="autoStart" v-model="model.settings.launch_at_login" type="checkbox" :disabled="model.busy||!model.baseline" @change="save"> 登录后启动 Tech Card Manager</label>
<div class="key">静默启动</div><label><input id="silentStart" v-model="model.settings.start_hidden" type="checkbox" :disabled="model.busy||!model.baseline||!model.settings.launch_at_login" @change="save"> 仅在登录启动时最小化到托盘</label>
</div></template>
