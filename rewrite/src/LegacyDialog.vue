<script setup lang="ts">
defineProps<{visible:boolean;items:string[];unsafePatch:boolean;pending?:boolean}>();
defineEmits<{cancel:[];confirm:[]}>();
</script>
<template>
 <div class="modalBackdrop" id="legacyBackdrop" :class="{show:visible}" role="dialog" aria-modal="true" aria-labelledby="legacyTitle"><div class="modal small scrollSurface"><div class="modalHeader"><div><h2 id="legacyTitle">检测到旧版 Card 软件组件</h2><div class="muted">新版服务尚未启动</div></div></div><p>启动 v5.0.0 前，需要分别处理下列旧程序、后台组件或网页卡片。程序只处理能够严格确认属于旧版 Card 软件的项目；修改 Emby 文件前会建立并验证备份。</p><ul class="legacyList" id="legacyList"><li v-for="(item,index) in items" :key="index">{{item}}</li></ul><div class="readonlyNote">如果取消，新版不会停止、删除或覆盖任何旧版组件。再次点击启动服务时会重新检查。</div><div class="modalFooter"><button class="btn" id="cancelLegacy" @click="$emit('cancel')">取消，保持旧版不变</button><button class="btn primary" id="confirmLegacy" :disabled="unsafePatch||pending" @click="$emit('confirm')">{{unsafePatch?'需要先人工确认网页文件':'迁移所列组件并继续'}}</button></div></div></div>
</template>
