import test,{after} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {createServer} from 'vite';
import vue from '@vitejs/plugin-vue';
import {createSSRApp,h} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {baseParse} from '@vue/compiler-dom';
import {matches,displayTitle,orderedSpecs,emptyMessage} from '../src/catalog.ts';
import type {CatalogItem} from '../src/catalog.ts';

const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const context:any={uiLanguage:'zh-CN',DEFAULT_UI_LANGUAGE:'zh-CN',catalogSelected:'',catalogStripe:0,catalogViewState:{tv:{expandedShows:new Set(),expandedSeasons:new Set()}},requestAnimationFrame:()=>0,syncCatalogListHeight:()=>{}};
vm.createContext(context);
for(const prefix of ['const FIELD_ORDER=','const FIELD_LABELS=','function fieldLabel(','function esc(','function catalogKey(','function catalogMatches(','function catalogItemHtml(','function renderTvTree(','function orderedSpecs(','function catalogTagHTML(','function renderTagList(','function renderCatalogPreview(']){
 const line=source.split('\n').find(line=>line.startsWith(prefix));assert.ok(line,prefix);vm.runInContext(line,context);
}
const server=await createServer({configFile:false,root:new URL('..',import.meta.url).pathname,plugins:[vue()],server:{middlewareMode:true,watch:null,hmr:false,ws:false},optimizeDeps:{noDiscovery:true,entries:[]},appType:'custom'});
after(()=>server.close());
const List=(await server.ssrLoadModule('/src/CatalogList.vue')).default;
const Preview=(await server.ssrLoadModule('/src/CatalogPreview.vue')).default;
function row(id:string,kind='Movie',extra:Partial<CatalogItem>={}):CatalogItem{return {id,root_id:'root',space:kind==='Movie'?'movie':'tv',path:'/media/'+id+'.nfo',source_hash:'',parser_revision:3,title:id,original_title:'',show_title:'',series_title:'',year:'2026',imdb:'',kind,season:'',episode:'',specs:{},tags:[],error:null,...extra};}
function legacy(row:CatalogItem){return {...row,type:row.kind,originalTitle:row.original_title,showTitle:row.show_title,seriesTitle:row.series_title,hasTechnicalSpecs:Object.keys(row.specs).length>0,libraryKind:row.space};}
function normalized(html:string):unknown {
 const walk=(node:any):unknown=>{
  if(node.type===3)return null;
  if(node.type===2)return node.content.trim()?node.content.trim():null;
  if(node.type===0)return node.children.map(walk).filter(Boolean);
  return [node.tag,Object.fromEntries(node.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='style'?(p.value?.content||'').replace(/;$/,''):p.name==='class'?(p.value?.content||'').split(/\s+/).sort().join(' '):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),node.children.map(walk).filter(Boolean)];
 };
 return walk(baseParse(html));
}
const tv=[row('节目','Series',{title:'节目 & 一',series_title:'节目 & 一'}),row('season','Season',{series_title:'节目 & 一',season:'1'}),row('episode10','Episode',{series_title:'节目 & 一',season:'1',episode:'10'}),row('episode2','Episode',{series_title:'节目 & 一',season:'1',episode:'2'}),row('unknown','Episode'),row('same-name','Series',{title:'节目 & 一',series_title:'节目 & 一',root_id:'other'})];
test('Vue TV DOM matches original grouping, counts, nesting, stripes and escaped titles',async()=>{
 for(const search of ['', '节目']){
  context.catalogSelected=tv[2].path;context.catalogStripe=0;
  const expected='<div class="catalogList scrollSurface" id="catalogList">'+context.renderTvTree(tv.map(legacy),Boolean(search))+'</div>';
  const actual=await renderToString(createSSRApp({render:()=>h(List,{rows:tv,space:'tv',locale:'zh-CN',selected:tv[2].id,expanded:[],search,loading:false,empty:''})}));
  assert.deepEqual(normalized(actual),normalized(expected));
 }
});
test('movie list retains all records without introducing paging or selection controls',async()=>{
 const rows=Array.from({length:127},(_,i)=>row('电影 '+i));context.catalogSelected=rows[0].path;context.catalogStripe=0;
 const expected='<div class="catalogList scrollSurface" id="catalogList">'+[...rows].sort((a,b)=>a.title.localeCompare(b.title,'zh-CN')).map(row=>context.catalogItemHtml(legacy(row))).join('')+'</div>';
 const actual=await renderToString(createSSRApp({render:()=>h(List,{rows,space:'movie',locale:'zh-CN',selected:rows[0].id,expanded:[],search:'',loading:false,empty:''})}));
 assert.deepEqual(normalized(actual),normalized(expected));
});
test('preview DOM matches original spec order, names, empty states and ownership badges',async()=>{
 const items=[null,row('无规格'),row('完整','Movie',{specs:{Camera:['ARRI'],Runtime:['120 min'],'Sound mix':['Dolby']},tags:[{value:'Generated',ownership:'generated',engine:'local'},{value:'Manual',ownership:'manual',engine:''},{value:'External & Tag',ownership:'external',engine:''}]})];
 for(const item of items){const host={innerHTML:''};context.$=()=>host;context.renderCatalogPreview(item?legacy(item):null);
  const actual=await renderToString(createSSRApp({render:()=>h(Preview,{item,locale:'zh-CN',busy:false})}));
  assert.deepEqual(normalized(actual),normalized('<div class="catalogPreview" id="catalogPreview">'+host.innerHTML+'</div>'));
 }
});
test('search and fallback titles retain the original title, show and series fields',()=>{
 const items=[row('a','Movie',{original_title:'Original Film'}),row('b','Episode',{show_title:'节目名',series_title:'系列名'}),row('c','Movie',{title:'',original_title:'Fallback'}),row('bad','Movie',{error:{code:'invalid-xml',message:'Broken',path:null},specs:{Camera:['ARRI']}})];
 for(const item of items){assert.equal(displayTitle(item),context.nfoDisplayTitle(legacy(item)));assert.deepEqual(orderedSpecs(item.specs),JSON.parse(JSON.stringify(context.orderedSpecs(item.specs))));
  for(const space of ['movie','tv'] as const)for(const query of [' original ','节目名','系列名','missing',''])for(const filter of ['all','ready','missing','error']){
   context.catalogSpace=space==='movie'?'movies':'tv';context.$=(id:string)=>({value:id==='#catalogSearch'?query:filter});assert.equal(matches(item,space,query,filter),context.catalogMatches(legacy(item)));
  }
 }
});
test('catalog read errors reproduce original inline list and preview markup',async()=>{
 const hosts:Record<string,{innerHTML:string}>={'#catalogList':{innerHTML:''},'#catalogPreview':{innerHTML:''}};
 const error='index <unavailable> & retry';
 const failed=vm.createContext({catalogLoadInFlight:false,catalogLoadedAt:0,$:(id:string)=>hosts[id],api:async()=>{throw Error(error);}});
 vm.runInContext(source.split('\n').find(line=>line.startsWith('function esc('))!,failed);
 vm.runInContext(source.split('\n').find(line=>line.startsWith('async function loadCatalog('))!,failed);
 await vm.runInContext('loadCatalog(true)',failed);
 const previous=row('stale');
 const list=await renderToString(createSSRApp({render:()=>h(List,{rows:[previous],space:'movie',locale:'zh-CN',selected:previous.id,expanded:[],search:'',loading:false,empty:'',error})}));
 const preview=await renderToString(createSSRApp({render:()=>h(Preview,{item:previous,locale:'zh-CN',busy:false,unavailable:true})}));
 assert.deepEqual(normalized(list),normalized('<div class="catalogList scrollSurface" id="catalogList">'+hosts['#catalogList'].innerHTML+'</div>'));
 assert.deepEqual(normalized(preview),normalized('<div class="catalogPreview" id="catalogPreview">'+hosts['#catalogPreview'].innerHTML+'</div>'));
});
test('an unavailable completed index keeps the original empty-state priority',()=>{
 assert.equal(emptyMessage([], 'movie', '', '摘要损坏'),'索引尚不可用：摘要损坏');
 assert.equal(emptyMessage([], 'movie', 'query', '摘要损坏'),'没有找到匹配的 NFO。');
 assert.equal(emptyMessage([row('tv','Series')], 'movie', '', '摘要损坏'),'索引中尚无电影 NFO。');
 assert.equal(emptyMessage([row('movie')], 'movie', '', '摘要损坏'),'当前状态筛选没有结果。');
});
