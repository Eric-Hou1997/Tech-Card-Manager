<script setup lang="ts">
import {ref,onMounted,onUnmounted} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import {listen,type UnlistenFn} from '@tauri-apps/api/event';
import type {AppError,Settings,SettingsOperation} from './contracts';
interface Status {settings:Settings;native_autostart:boolean|null;background_mode:string;tray_available:boolean;closing:boolean;error:AppError|null}
const status=ref<Status|null>(null),settings=ref<Settings>({revision:0,close_action:'quit',launch_at_login:false,start_hidden:true}),busy=ref(false),error=ref(''),closing=ref(false);
let off:UnlistenFn|undefined,offClosing:UnlistenFn|undefined,alive=true;
async function refresh(){status.value=await invoke<Status>('lifecycle_status');settings.value={...status.value.settings};}
async function apply(){if(busy.value)return;busy.value=true;error.value='';try{await invoke<SettingsOperation>('lifecycle_apply',{id:crypto.randomUUID(),settings:{...settings.value}});await refresh();}catch(e){error.value=JSON.stringify(e);}finally{busy.value=false;}}
onMounted(async()=>{try{await refresh();const a=await listen<AppError>('lifecycle-error',event=>{if(alive){error.value=event.payload.message;closing.value=false;}});const b=await listen<boolean>('lifecycle-closing',event=>{if(alive)closing.value=event.payload;});if(alive){off=a;offClosing=b;}else{a();b();}}catch(e){error.value=JSON.stringify(e);}});
onUnmounted(()=>{alive=false;off?.();offClosing?.();});
</script>
<template><section aria-labelledby="lifecycle-title"><h2 id="lifecycle-title">窗口与登录启动</h2>
<label>关闭窗口时 <select v-model="settings.close_action" :disabled="busy"><option value="quit">停止任务与服务并退出</option><option value="background">继续在后台运行</option></select></label>
<p><label><input v-model="settings.launch_at_login" type="checkbox" :disabled="busy">登录系统时启动</label> <label><input v-model="settings.start_hidden" type="checkbox" :disabled="busy">登录启动时不显示主窗口</label></p>
<p v-if="status?.background_mode==='minimize'">此 Linux 环境以最小化窗口保留恢复入口；可以从任务栏或再次启动应用恢复。</p>
<p v-else>后台运行时可以从托盘、Dock 或再次启动应用恢复。菜单“退出”会停止任务与服务。</p>
<p v-if="status">系统登录项：{{ status.native_autostart===null?'无法核实':status.native_autostart?'已启用':'未启用' }} · 托盘：{{ status.tray_available?'已创建':'不可用，保留窗口入口' }}</p>
<div class="actions"><button :disabled="busy" @click="apply">保存设置</button><button :disabled="busy" @click="invoke('background_window')">转入后台</button><button :disabled="busy||closing" @click="invoke('quit_probe')">完全退出</button></div>
<p v-if="closing" role="status">正在停止任务与服务，完成清理后退出…</p><p v-if="error||status?.error" role="alert">{{ error||status?.error?.message }}</p>
</section></template>
