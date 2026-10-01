<script setup lang="ts">
import {reactive,watch,onUnmounted} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import type {InstallationIdentity} from './contracts';
import {initialCardUpdate,createCardUpdate} from './card-update';
const props=defineProps<{active:boolean;identity:InstallationIdentity|null;locale?:string}>();
const emit=defineEmits<{notify:[message:string]}>();
const state=reactive(initialCardUpdate());
const update=createCardUpdate(invoke,state,message=>emit('notify',message),()=>props.locale||'zh-CN');
watch(()=>props.active,active=>{if(active)void update.check(false);},{immediate:true});
onUnmounted(update.dispose);
</script>
<template><div class="aboutUpdate"><div><strong>软件更新</strong><span class="aboutUpdateState" id="cardUpdateState">{{state.label}}</span></div><button class="btn" id="checkCardUpdate" :disabled="state.busy" @click="update.activate">{{state.button}}</button></div>
<div class="aboutInstall" :class="{hide:!state.prompt}" id="cardInstallPrompt"><strong id="cardInstallTitle">{{state.result?.available?'最新版本 '+state.result.latest_version+' 可用':'最新版本可用'}}</strong>
<p v-if="!identity||identity.channel==='portable'">即将打开已确认的 GitHub 安装包，请下载 <code id="cardPackageName">{{state.result?.package_name||'TCM-v5.0.0-Windows-x64-EXE.zip'}}</code>。解压后，请先从系统托盘完全退出 Tech Card Manager，再用其中的 <code>Tech-Card-Manager.exe</code> 替换当前 Portable 程序。</p>
<p v-else>即将打开已确认的 GitHub 安装包，请下载 <code id="cardPackageName">{{state.result?.package_name||'—'}}</code>。请先从{{identity.os==='darwin'?'菜单栏':'系统托盘'}}完全退出 Tech Card Manager，再{{identity.channel==='app'?'打开 DMG 并替换现有应用':identity.channel==='appimage'?'替换当前 AppImage 文件':'运行安装包完成升级'}}。</p>
<p v-if="!identity||identity.channel==='portable'" class="muted">请保留 <code>data</code>、<code>logs</code>、<code>backup</code>、<code>runtime</code>、<code>updates</code> 及其他用户文件夹。</p><p v-else class="muted">请保留已有配置、日志、备份及其他用户文件夹。</p>
<p class="muted" id="cardPortablePath">{{state.result?(identity&&identity.channel!=='portable'?'当前程序目录：':'当前 Portable 程序目录：')+(state.result.portable_directory||'—'):''}}</p>
<div><button class="btn" id="cancelCardInstall" @click="update.cancel">取消</button><button class="btn primary" id="confirmCardInstall" @click="update.download">下载指定安装包</button></div></div></template>
