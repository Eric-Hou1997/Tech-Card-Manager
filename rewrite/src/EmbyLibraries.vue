<script setup lang="ts">
import { ref,onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { DiscoveredLibrary,MappingSettings,Space } from './contracts';
const settings=ref<MappingSettings>({revision:0,mappings:[]}),libraries=ref<DiscoveredLibrary[]>([]),busy=ref(false),error=ref(''),message=ref('');
let pending:{id:string;value:MappingSettings}|null=null;
async function run(work:()=>Promise<void>){if(busy.value)return;busy.value=true;error.value='';try{await work();}catch(e){error.value=typeof e==='object'?JSON.stringify(e):String(e);}finally{busy.value=false;}}
async function reload(){await run(async()=>{await load();pending=null;message.value='已读取当前保存的映射。';});}
async function load(){settings.value=await invoke<MappingSettings>('emby_path_mappings');}
async function save(){await run(async()=>{pending??={id:crypto.randomUUID(),value:JSON.parse(JSON.stringify(settings.value))};settings.value=await invoke<MappingSettings>('emby_save_mappings',pending);pending=null;message.value='路径映射已保存。重新发现目录以检查映射结果。';});}
async function discover(){await run(async()=>{libraries.value=await invoke<DiscoveredLibrary[]>('emby_libraries');message.value='发现 '+libraries.value.length+' 个物理根目录；请逐项确认空间后添加，扫描范围仍需单独勾选。';});}
async function add(library:DiscoveredLibrary,space:Space){if(!library.local_path)return;await run(async()=>{await invoke('emby_add_library',{id:crypto.randomUUID(),path:library.local_path,space});message.value=library.name+' 已添加到 '+(space==='movie'?'Movie':'TV')+'。请在媒体库勾选后扫描。';});}
onMounted(()=>run(load));
</script>
<template>
<article class="emby-libraries" aria-labelledby="emby-libraries-title">
<h3 id="emby-libraries-title">Emby 媒体目录与路径映射</h3>
<p>验证 Emby 版本并选择数据目录后，从实际数据库读取物理目录及 Movie／TV 证据。服务器路径与本机挂载路径不同的情况，请先配置映射。</p>
<fieldset :disabled="busy||!!pending"><legend>路径映射</legend>
<div v-for="(mapping,index) in settings.mappings" :key="index" class="mapping-row"><label>服务器路径前缀<input v-model="mapping.server_prefix" placeholder="/media/电影 或 D:\Movies"></label><label>本机目录<input v-model="mapping.local_root" placeholder="本机绝对路径"></label><button @click="settings.mappings.splice(index,1)">移除此映射</button></div>
<button @click="settings.mappings.push({server_prefix:'',local_root:''})">添加映射</button>
</fieldset>
<div class="actions"><button :disabled="busy" @click="save">{{pending?'重试保存同一操作':'保存路径映射'}}</button><button :disabled="busy" @click="reload">放弃未保存修改并重新读取</button><button :disabled="busy||!!pending" @click="discover">从 Emby 发现媒体目录</button></div>
<p v-if="busy" role="status">正在读取目录信息…</p><p v-if="message" role="status">{{message}}</p><p v-if="error" role="alert">{{error}}</p>
<ul><li v-for="library in libraries" :key="library.id"><h4>{{library.name||library.server_path}}</h4><p class="path">{{library.server_path}} → {{library.local_path||'未找到可访问的本机目录'}}</p><p>{{library.state}} · IMDb 电影 {{library.movie_evidence}} · 节目 {{library.series_evidence}} · 剧集 {{library.episode_evidence}}</p><p v-if="library.issues.length">{{library.issues.join('；')}}</p>
<p v-if="library.spaces.length>1">此根目录混合了 Movie 和 TV。可分别添加到两个空间；每次扫描仅处理所属空间的 NFO。</p>
<button v-for="space in library.spaces" :key="space" :disabled="busy||!library.local_path" @click="add(library,space)">添加到 {{space==='movie'?'Movie':'TV'}}</button></li></ul>
</article>
</template>
<style scoped>.emby-libraries{margin-top:24px}.mapping-row{display:flex;gap:12px;align-items:end;flex-wrap:wrap;margin-bottom:12px}label{display:grid;gap:4px;flex:1;min-width:200px}input{font:inherit;min-width:0;padding:8px}.actions{margin-top:12px}.path{overflow-wrap:anywhere}fieldset{border:1px solid #8aa4c2;border-radius:8px}li{margin-top:16px}</style>
