import test,{after} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {createServer} from 'vite';
import vue from '@vitejs/plugin-vue';
import {createSSRApp,h} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {baseParse} from '@vue/compiler-dom';
import {jobLine} from '../src/job-presentation.ts';

const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const server=await createServer({configFile:false,root:new URL('..',import.meta.url).pathname,plugins:[vue()],server:{middlewareMode:true,watch:null,hmr:false,ws:false},optimizeDeps:{noDiscovery:true,entries:[]},appType:'custom'});
after(()=>server.close());
const Output=(await server.ssrLoadModule('/src/MaintenanceOutput.vue')).default;
function normalized(html:string):unknown {
 const walk=(n:any):unknown=>n.type===3?null:n.type===2?n.content.trim()||null:n.type===0?n.children.map(walk).filter(Boolean):[n.tag,Object.fromEntries(n.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='style'?(p.value?.content||'').replace(/;$/,''):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),n.children.map(walk).filter(Boolean)];
 return walk(baseParse(html));
}
function baseline(errors:Array<{path:string;error:string}>,job:any) {
 const nodes=new Map<string,any>();
 const $=(id:string)=>{if(!nodes.has(id))nodes.set(id,{classList:{add(){},remove(){}},dataset:{},textContent:'',innerHTML:'',value:''});return nodes.get(id);};
 const context:any={$,rootsInitialized:true,lastDiscoveryKey:'',renderService(){},setPill(){},renderRootEditor(){},rootKey:(s:string)=>s,lastJobRunning:false,setBusy(){},loadCatalog(){},uiLanguage:'zh-CN'};
 vm.createContext(context);
 for(const prefix of ['function esc(','const actionNames=','function renderStatus(','function renderJob(']){
  const line=source.split('\n').find(line=>line.startsWith(prefix));assert.ok(line,prefix);vm.runInContext(line,context);
 }
 context.renderStatus({app_version:'5.0.0',roots_configured:true,service:{state:'stopped',message:'服务已关闭'},extra:{},xml_error_details:errors});
 context.renderJob(job);
 const env:Array<[string,string]>=[['程序版本','v5.0.0'],['服务状态','服务已关闭'],['网页入口注入','缺失/版本不符'],['卡片脚本','文件缺失'],['运行许可','未启用/已过期'],['技术规格索引','缺失'],['Emby 服务目录','—'],['网页入口文件','—']];
 return {environment:env,jobLine:$('#jobline').textContent,jobLog:job.log||'等待任务…',html:`<div class="kv" id="env" style="margin-top:14px">${$('#env').innerHTML}</div><div id="errors" class="muted" style="margin-top:12px">${$('#errors').innerHTML}</div><div class="muted" id="jobline" style="margin-top:12px">${context.esc($('#jobline').textContent)}</div><pre class="log scrollSurface" id="joblog" tabindex="0">${context.esc(job.log||'等待任务…')}</pre>`};
}
test('maintenance output uses the original environment, empty XML state and idle log DOM',async()=>{
 const expected=baseline([],{});
 const actual=await renderToString(createSSRApp({render:()=>h(Output,{...expected,errors:[]})}));
 assert.deepEqual(normalized(actual),normalized(expected.html));
});
test('maintenance failures retain the original 30-item limit, escaped paths and directory actions',async()=>{
 const errors=Array.from({length:34},(_,i)=>({id:String(i),path:'/媒体/<电影 & '+i+'>.nfo',error:'Invalid <xml> & data'}));
 for(const job of [{running:true,action:'diagnose',message:'检查中',log:'路径\n<xml> & error'},{running:false,action:'diagnose',exit_code:1,message:'读取失败',log:'诊断失败\n/media/a.nfo'}]){
  const expected=baseline(errors,job);
  const actual=await renderToString(createSSRApp({render:()=>h(Output,{environment:expected.environment,jobLine:expected.jobLine,jobLog:expected.jobLog,errors})}));
  assert.deepEqual(normalized(actual),normalized(expected.html));
  assert.equal((actual.match(/data-error-open=/g)||[]).length,30);
 }
});

test('persisted discovery states render the same job line as the original function',()=>{
 for(const job of [{running:true,action:'discover-roots',message:'运行中'},{running:false,action:'discover-roots',exit_code:0,message:'完成'},{running:false,action:'discover-roots',exit_code:1,message:'任务已取消'},{running:false,action:'discover-roots',exit_code:2,message:'没有识别出电影/电视剧物理路径。'}]){
  assert.equal(jobLine(job),baseline([],job).jobLine);
 }
});
