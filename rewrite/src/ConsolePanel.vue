<script setup lang="ts">
import {computed} from 'vue';
import type {CatalogSummary} from './contracts';
import {servicePresentation,serviceStartedAt,type ServicePhase} from './console';
const props=defineProps<{phase:ServicePhase;lastStartedAt?:string|null;locale:string;loading:boolean;updatedAt:string;summary:CatalogSummary;webVersion?:string|null;webHealthy:boolean;runtimeValid:boolean;webInstalled:boolean;busy:boolean;platform:string}>();
defineEmits<{toggle:[]}>();
const service=computed(()=>servicePresentation(props.phase));
</script>
<template>
 <section class="card consolePanel">
  <div class="sectionTitle"><h2>控制台</h2><span class="muted" id="lastUpdated">{{loading?'正在读取状态…':'状态更新 '+updatedAt}}</span></div>
  <div class="console"><div class="serviceControl"><div class="serviceCard" id="serviceCard" :class="{error:service.error}"><div class="serviceCopy"><div class="serviceStateText" id="serviceStateText">{{service.text}}</div><div class="serviceStartedAt" id="serviceStartedAt">上次启动时间：{{serviceStartedAt(lastStartedAt,locale)}}</div></div><button class="btn serviceButton" id="serviceButton" :class="{running:phase==='running'}" :disabled="busy||(!loading&&service.busy)" :data-next="loading?undefined:service.next" @click="$emit('toggle')">{{service.label}}</button></div></div><div class="metrics"><div class="metric"><div class="k">电影/节目可展示</div><div class="v" id="indexed">{{summary.displayable}}</div><div class="s" id="eligible">{{loading?'等待索引':'可展示 '+summary.web_eligible+' · 排除单集 '+summary.episodes_excluded}}</div></div><div class="metric"><div class="k">NFO 总数</div><div class="v" id="nfos">{{summary.total}}</div><div class="s" id="nfoState">{{loading?'等待索引':summary.generated_at?'更新于 '+summary.generated_at:'索引尚未生成'}}</div></div><div class="metric"><div class="k">解析异常</div><div class="v" id="xmlCount">{{summary.errors}}</div><div class="s" id="xml">{{loading?'暂无异常':summary.errors?'请在设置中查看':'暂无 XML 异常'}}</div></div><div class="metric"><div class="k">网页卡片</div><div class="v" id="webver">{{webVersion?'v'+webVersion:'—'}}</div><div class="s" id="webpatch">{{loading?'正在检测':webHealthy?(runtimeValid?'卡片服务已就绪':'集成正常 · 当前停用'):(webInstalled?'网页集成需要维护':'尚未设置网页卡片')}}</div></div></div></div>
  <div class="readonlyNote consoleHint">{{platform}} 只读索引媒体库 NFO 的 <code>&lt;technicalspecs&gt;</code>，最小化后继续运行；关闭窗口将停止服务并撤下 Emby 技术规格卡片。</div>
 </section>
</template>
