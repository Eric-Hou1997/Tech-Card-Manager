<script setup lang="ts">
import { ref,reactive,provide,onMounted,onUnmounted,watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {listen,type UnlistenFn} from '@tauri-apps/api/event';
import LibraryPanel from './LibraryPanel.vue';
import {Languages,languageKey,translateDocument} from './baseline-language';
const library=ref<InstanceType<typeof LibraryPanel>|null>(null);
const languages=reactive(new Languages(invoke,message=>library.value?.notify(message))) as Languages;
provide(languageKey,languages);
let translation:ReturnType<typeof translateDocument>|undefined;
let languageEvents:UnlistenFn|undefined,packEvents:UnlistenFn|undefined;
onMounted(async()=>{translation=translateDocument(document,languages);translation.refresh();
 try{const off=await listen('configuration-changed',()=>{if(languages.alive){void languages.read();void languages.restore();}});if(languages.alive)languageEvents=off;else off();
  if(languages.alive){const off=await listen('language-state-changed',()=>{if(languages.alive)void languages.read();});if(languages.alive)packEvents=off;else off();}
 }
 catch(error){if(languages.alive)library.value?.notify((error as {message?:string})?.message||String(error));}
 if(languages.alive){await languages.read();if(languages.alive)void languages.restore();}
});
watch(()=>languages.snapshot,()=>translation?.refresh(),{flush:'post'});
onUnmounted(()=>{languages.dispose();translation?.dispose();languageEvents?.();packEvents?.();});
import './styles/baseline.css';
import './styles/product.css';
let ready = false, confirming = false;
async function frontendReady() {
 if(ready || confirming)return;
 confirming=true;
 try { await invoke('frontend_ready'); ready=true; }
 catch(error){library.value?.notify((error as {message?:string})?.message||String(error));}
 finally {confirming=false;}
}
</script>
<template>
 <LibraryPanel ref="library" @ready="frontendReady" />
</template>
