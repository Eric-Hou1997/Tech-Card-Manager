<script setup lang="ts">
defineProps<{initialized:boolean;embyDetected:boolean;rootsConfigured:boolean;webConfigured:boolean}>();
</script>
<template>
 <div class="setupFlowIntro">配置媒体目录和检查周期</div><div class="setupFlow" id="setupFlow"><div class="setupStep" id="setupStepEmby" :class="{done:initialized&&embyDetected}" :data-complete="initialized?String(embyDetected):undefined"><strong>1　检测 Emby</strong><span id="setupEmby">{{initialized&&embyDetected?'已完成':'等待检测'}}</span></div><div class="setupStep" id="setupStepRoots" :class="{done:initialized&&rootsConfigured}" :data-complete="initialized?String(rootsConfigured):undefined"><strong>2　选择媒体目录</strong><span id="setupRoots">{{initialized?(rootsConfigured?'已完成':'请选择目录'):'尚未完成'}}</span></div><div class="setupStep" id="setupStepWeb" :class="{done:initialized&&webConfigured}" :data-complete="initialized?String(webConfigured):undefined"><strong>3　设置网页卡片</strong><span id="setupWeb">{{initialized?(webConfigured?'已完成':'点击下方按钮完成'):'尚未完成'}}</span></div></div>
</template>
