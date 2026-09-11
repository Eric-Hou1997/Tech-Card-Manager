<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { InstallationIdentity, UpdateProgress } from './contracts';
const identity=ref<InstallationIdentity|null>(null),progress=ref<UpdateProgress|null>(null),busy=ref(false),error=ref('');
let unlisten:UnlistenFn|undefined;let alive=true;
async function check(){if(busy.value)return;busy.value=true;error.value='';try{progress.value=await invoke<UpdateProgress>('update_check',{id:crypto.randomUUID()});}catch(e){error.value=JSON.stringify(e);}finally{busy.value=false;}}
async function install(){if(!progress.value||busy.value)return;busy.value=true;error.value='';try{progress.value=await invoke<UpdateProgress>('update_install',{id:progress.value.operation_id});}catch(e){error.value=JSON.stringify(e);}finally{busy.value=false;}}
onMounted(async()=>{try{identity.value=await invoke<InstallationIdentity>('update_identity');progress.value=await invoke<UpdateProgress|null>('update_status');const stop=await listen<UpdateProgress>('update-progress',event=>{if(alive)progress.value=event.payload;});if(alive)unlisten=stop;else stop();}catch(e){error.value=JSON.stringify(e);}});
onUnmounted(()=>{alive=false;unlisten?.();});
const labels:Record<string,string>={'checking':'检查更新','available':'可更新','up-to-date':'已是最新版本','downloading':'正在下载','verifying':'正在验签','installing':'正在安装','installed-awaiting-health':'等待新版启动验证','verified':'更新已验证','failed':'更新失败','cancelled':'已取消','interrupted':'上次更新已中断','recovery-required':'需要恢复检查'};
</script>
<template><section aria-labelledby="update-title"><h2 id="update-title">应用更新</h2>
<p v-if="identity">{{ identity.product }} · {{ identity.os }} / {{ identity.arch }} · {{ identity.channel }}</p>
<p v-if="identity && ['deb','rpm'].includes(identity.channel)">此安装由系统包管理器维护，请从已配置的软件源升级。</p>
<p v-if="progress" role="status">{{ labels[progress.phase] || progress.phase }}<template v-if="progress.version"> · {{ progress.version }}</template><template v-if="progress.phase==='downloading'"> · {{ progress.downloaded }} / {{ progress.total || '?' }} B</template></p>
<div class="actions"><button :disabled="busy" @click="check">检查更新</button><button v-if="progress?.phase==='available'" :disabled="busy" @click="install">更新并自动重启</button><button v-if="progress?.phase==='downloading'" @click="invoke('update_cancel')">取消下载</button></div>
<p v-if="error || progress?.error" role="alert">{{ error || progress?.error?.message }}</p>
</section></template>
