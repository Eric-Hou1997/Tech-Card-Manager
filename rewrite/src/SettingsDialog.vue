<script setup lang="ts">
defineProps<{visible:boolean}>();
defineEmits<{close:[]}>();
</script>
<template>
 <Teleport to="body">
  <div class="modalBackdrop" id="settingsBackdrop" :class="{show:visible}" role="dialog" aria-modal="true" aria-labelledby="settingsTitle">
   <div class="modal">
    <div class="modalHeader"><h2 id="settingsTitle">设置</h2><button class="btn iconButton" data-close-modal="settingsBackdrop" aria-label="关闭" @click="$emit('close')">×</button></div>
    <div class="settingsBody scrollSurface" id="settingsContent"><slot /></div>
   </div>
  </div>
 </Teleport>
</template>
