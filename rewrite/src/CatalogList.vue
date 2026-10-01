<script setup lang="ts">
import {computed} from 'vue';
import type {MediaItem,Space} from './contracts';
import type {CatalogItem as Row} from './catalog';
import {tvGroups} from './catalog';
import CatalogItem from './CatalogItem.vue';
const props=defineProps<{rows:Row[];space:Space;locale:string;selected:string|null;expanded:string[];search:string;loading:boolean;empty:string;error?:string}>();
const emit=defineEmits<{inspect:[item:MediaItem];expanded:[value:string[]]}>();
const groups=computed(()=>tvGroups(props.rows,props.locale));
const movies=computed(()=>[...props.rows].sort((a,b)=>a.title.localeCompare(b.title,props.locale)));
const autoOpen=computed(()=>Boolean(props.search.trim()));
function toggle(event:Event,id:string){
 if(autoOpen.value||!(event.target instanceof HTMLDetailsElement))return;
 const open=event.target.open;if(props.expanded.includes(id)===open)return;
 emit('expanded',open?[...props.expanded,id]:props.expanded.filter(value=>value!==id));
}
</script>
<template>
 <div class="catalogList scrollSurface" id="catalogList">
  <div v-if="error" class="catalogEmpty">NFO 索引读取失败：{{error}}</div>
  <div v-else-if="loading" class="catalogEmpty">正在读取只读索引…</div>
  <div v-else-if="!rows.length" class="catalogEmpty">{{empty}}</div>
  <template v-else-if="space==='movie'"><CatalogItem v-for="(item,index) in movies" :key="item.id" :item="item" :active="selected===item.id" :stripe="Boolean(index%2)" :depth="0" @inspect="emit('inspect',$event)" /></template>
  <template v-else><details v-for="group in groups" :key="group.id" class="catalogTree" :data-tv-show="group.id" :data-tv-auto="autoOpen?'1':undefined" :open="autoOpen||expanded.includes('show:'+group.id)" @toggle.stop="toggle($event,'show:'+group.id)">
   <summary class="catalogGroup" data-i18n-user>{{group.name}} · {{group.count}}</summary>
   <CatalogItem v-for="row in group.rows" :key="row.item.id" v-bind="row" :active="selected===row.item.id" @inspect="emit('inspect',$event)" />
   <details v-for="season in group.seasons" :key="season.id" class="catalogSeason" :data-tv-season="season.id" :data-tv-auto="autoOpen?'1':undefined" :open="autoOpen||expanded.includes('season:'+season.id)" @toggle.stop="toggle($event,'season:'+season.id)">
    <summary class="catalogGroup">{{season.label}} · {{season.rows.length}}</summary>
    <CatalogItem v-for="row in season.rows" :key="row.item.id" v-bind="row" :active="selected===row.item.id" @inspect="emit('inspect',$event)" />
   </details>
  </details></template>
 </div>
</template>
