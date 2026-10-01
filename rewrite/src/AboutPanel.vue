<script setup lang="ts">
import logo from './assets/TCM_logo_tiny.png';
import UpdatePanel from './UpdatePanel.vue';
import {computed,onMounted,onUnmounted,ref,inject} from 'vue';
import {languageKey,type Languages} from './baseline-language';
const languages=inject<Languages>(languageKey);
import {invoke} from '@tauri-apps/api/core';
import type {InstallationIdentity} from './contracts';
defineProps<{active:boolean}>();
const emit=defineEmits<{notify:[message:string]}>();
const identity=ref<InstallationIdentity|null>(null);let alive=true;
const metadata=computed(()=>{const i=identity.value;if(!i)return 'v5.0.0';const os=i.os==='darwin'?'macOS':i.os==='windows'?'Windows':'Linux',arch=i.arch==='aarch64'?'ARM64':'x64',channel=({app:'DMG',nsis:'NSIS',appimage:'AppImage',deb:'DEB',rpm:'RPM',portable:'Portable'} as Record<string,string>)[i.channel];return `v5.0.0　${os} · ${arch} ${channel}`;});
const legalEnglish=computed(()=>!['zh-CN','zh-Hant'].includes(languages?.snapshot.locale||'zh-CN'));
const privacyUrl=computed(()=>`https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/${legalEnglish.value?'docs/legal/PRIVACY.en.md':'PRIVACY.md'}`);
const termsUrl=computed(()=>`https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/${legalEnglish.value?'docs/legal/TERMS.en.md':'TERMS.md'}`);
type ProductLink='privacy'|'terms'|'license'|'repository';
async function openLink(kind:ProductLink){
 try{await invoke('open_product_link',{kind,english:legalEnglish.value});}catch(error){if(alive)emit('notify',(error as {message?:string})?.message||String(error));}
}
onMounted(async()=>{try{const value=await invoke<InstallationIdentity>('update_identity');if(alive)identity.value=value;}catch(error){if(alive)emit('notify',(error as {message?:string})?.message||String(error));}});
onUnmounted(()=>{alive=false;});
</script>
<template><section class="aboutCard" aria-label="关于 Tech Card Manager"><div class="aboutHead"><img :src="logo" alt=""><span>关于</span></div><h3 class="aboutTitle">Tech Card Manager</h3><p class="aboutSummary">Emby Server 只读 NFO 索引与技术规格卡片管理器</p><p class="aboutMeta">{{metadata}}</p><div class="aboutRule"></div><UpdatePanel :locale="languages?.snapshot.locale" :active="active" :identity="identity" @notify="emit('notify',$event)" /><div class="aboutRule"></div><p class="aboutDisclaimer">本软件为独立开发工具，与 IMDb.com, Inc. 或 Emby LLC 无隶属、授权或背书关系。相关商标归各自权利人所有。</p><div class="aboutRule"></div><p class="aboutCredits" data-i18n-user>作者 侯雁泽　　© 2026 侯雁泽　　Apache License 2.0</p><nav class="aboutLinks"><a id="privacyLink" :href="privacyUrl" target="_blank" rel="noopener" @click.prevent="openLink('privacy')">隐私政策</a><a id="termsLink" :href="termsUrl" target="_blank" rel="noopener" @click.prevent="openLink('terms')">使用条款</a><a href="https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/LICENSE" target="_blank" rel="noopener" @click.prevent="openLink('license')">开源许可</a><a href="https://github.com/Eric-Hou1997/Tech-Card-Manager" target="_blank" rel="noopener" @click.prevent="openLink('repository')">GitHub 项目</a></nav></section></template>
