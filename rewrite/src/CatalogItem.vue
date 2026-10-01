<script setup lang="ts">
import type {MediaItem} from './contracts';
import {displayTitle,stateLabel} from './catalog';
defineProps<{item:MediaItem;active:boolean;stripe:boolean;depth:number}>();
defineEmits<{inspect:[item:MediaItem]}>();
</script>
<template>
 <button class="catalogItem" :class="{active,stripe}" :data-catalog-key="item.path" :style="{paddingLeft:13+depth*18+'px'}" @click="$emit('inspect',item)"><strong>{{displayTitle(item)}}</strong><div class="meta">{{[item.year,item.imdb,stateLabel(item)].filter(Boolean).join(' · ')}}</div></button>
</template>
