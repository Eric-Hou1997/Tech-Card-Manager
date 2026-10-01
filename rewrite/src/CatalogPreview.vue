<script setup lang="ts">
import {computed} from 'vue';
import type {MediaItem} from './contracts';
import {displayTitle,stateLabel,orderedSpecs,fieldLabel} from './catalog';
const props=defineProps<{item:MediaItem|null;locale:string;busy:boolean;unavailable?:boolean}>();
defineEmits<{reveal:[item:MediaItem];copy:[]}>();
const specs=computed(()=>props.item?orderedSpecs(props.item.specs):[]);
const external=computed(()=>props.item?.tags.filter(tag=>tag.ownership!=='generated')||[]);
const generated=computed(()=>props.item?.tags.filter(tag=>tag.ownership==='generated')||[]);
</script>
<template>
 <div class="catalogPreview" id="catalogPreview">
  <div v-if="unavailable" class="catalogEmpty">请在设置的“维护与故障处理”中查看详细状态。</div>
  <div v-else-if="!item" class="catalogEmpty">选择一个 NFO 查看技术规格和根级标签。</div>
  <template v-else>
   <div class="row"><div class="rowHead"><div><strong>{{displayTitle(item)}}</strong><div class="muted">{{[item.kind,item.year,item.imdb].filter(Boolean).join(' · ')}}</div></div><span class="badge" :class="item.error?'bad':Object.keys(item.specs).length?'ok':'warn'"><span class="dot"></span>{{stateLabel(item)}}</span></div><div class="path">{{item.path}}</div><div class="actions" style="margin-top:10px"><button class="btn" :disabled="busy" :data-catalog-open="item.path" @click="$emit('reveal',item)">打开所在文件夹</button><button class="btn" :disabled="busy" :data-copy-path="item.path" @click="$emit('copy')">复制路径</button></div></div>
   <div v-if="item.error" class="errorRow"><strong>读取失败</strong><div>{{item.error.message}}</div></div>
   <div class="row"><strong>技术规格</strong><div class="catalogSpecs"><template v-for="[key,values] in specs" :key="key"><div class="key">{{fieldLabel(key,locale)}}</div><div class="specValues"><div v-for="(value,index) in values" :key="index">{{value}}</div></div></template><div v-if="!specs.length" class="muted">当前 NFO 没有有效的 &lt;technicalspecs&gt;。条目仍保留在只读目录中。</div></div></div>
   <div class="row"><strong>根级标签</strong><div class="tagList"><span v-for="(tag,index) in external" :key="index" class="badge">{{tag.value}}</span><div v-if="generated.length" class="generatedTagRow"><span v-for="(tag,index) in generated" :key="index" class="badge managerGenerated" title="本程序生成（来自 NFO ownership 清单）">{{tag.value}}</span></div><span v-if="!item.tags.length" class="muted">没有根级标签</span></div></div>
  </template>
 </div>
</template>
