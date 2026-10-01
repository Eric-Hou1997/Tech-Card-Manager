<script setup lang="ts">
defineProps<{
  environment: Array<[string, string]>;
  errors: Array<{id:string;path:string;error:string}>;
  jobLine: string;
  jobLog: string;
}>();
defineEmits<{reveal:[id:string]}>();
</script>
<template>
 <div class="kv" id="env" style="margin-top:14px"><template v-for="[key,value] in environment" :key="key"><div class="key">{{key}}</div><div>{{value}}</div></template></div>
 <div id="errors" class="muted" style="margin-top:12px"><template v-if="errors.length"><div v-for="item in errors.slice(0,30)" :key="item.id" class="errorRow"><div class="path">{{item.path}}</div><div>{{item.error}}</div><button class="btn" style="margin-top:8px" :data-error-open="item.path" @click="$emit('reveal',item.id)">打开所在目录</button></div></template><div v-else class="muted">暂无 XML 读取异常。</div></div>
 <div class="muted" id="jobline" style="margin-top:12px">{{jobLine}}</div>
 <pre class="log scrollSurface" id="joblog" tabindex="0">{{jobLog}}</pre>
</template>
