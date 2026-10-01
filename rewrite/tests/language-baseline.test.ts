import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {baseParse} from '@vue/compiler-dom';
import {createSSRApp,h} from 'vue';
import {renderToString} from 'vue/server-renderer';
import {component} from './support/mount-vue.ts';
import * as language from '../src/baseline-language.ts';
import type {Locale,LanguageSnapshot} from '../src/contracts';
const {Languages,languageData,languageKey,message,stableMessageID,translateDocument}=language;
const source=await readFile(new URL('../../windows/web/index.html',import.meta.url),'utf8');
const snapshot=(locale:Locale='zh-CN'):LanguageSnapshot=>({locale,options:languageData.options.map(o=>({...o,error:null})),web_messages:{},native_messages:{}});
function normalize(html:string):unknown {
 function walk(node:any):unknown{if(node.type===3)return null;if(node.type===2)return node.content.trim()||null;if(node.type===0)return node.children.map(walk).filter(Boolean);return [node.tag,Object.fromEntries(node.props.filter((p:any)=>p.type===6).map((p:any)=>[p.name,p.name==='class'?(p.value?.content||'').split(/\s+/).filter(Boolean).sort().join(' '):p.value?.content||'']).sort((a:any,b:any)=>a[0].localeCompare(b[0]))),node.children.map(walk).filter(Boolean)];}return walk(baseParse(html));
}
function original(locale:string,options:any[],downloading:string){
 const nodes=new Map<string,any>();const $=(id:string)=>{if(!nodes.has(id))nodes.set(id,{innerHTML:'',textContent:''});return nodes.get(id);};
 const c:any={$,uiLanguage:locale,esc:(v:string)=>v.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;'),uiMessage:(s:string)=>message(s,locale)};vm.createContext(c);
 const start=source.indexOf('const LANGUAGE_NAMES='),end=source.indexOf('async function loadLanguageWebMessages',start);
 vm.runInContext(source.slice(start,end)+`;languageDownloadInFlight=${JSON.stringify(downloading)};`,c);c.renderLanguagePicker(options);return $;
}
test('all eight language rows, flags, native names and four action states match original rendered markup',async()=>{
 const Picker=await component(new URL('../src/LanguagePicker.vue',import.meta.url),{'./baseline-language':language});
 for(const locale of ['zh-CN','zh-Hant','en-US','fr-FR','ru-RU','ja-JP','es-ES','th-TH'] as Locale[])for(const state of ['not-installed','installed','downloading','failed']){
  const model=new Languages(async()=>assert.fail('SSR cannot call native commands'),()=>{});model.snapshot=snapshot(locale);for(const option of model.snapshot.options.filter(o=>!o.built_in)){option.state=state;option.installed=state==='installed';}const old=original(locale,model.snapshot.options,'');
  const html=await renderToString(createSSRApp({setup(){return ()=>h(Picker);}}).provide(languageKey,model));
  const ast:any=baseParse(html);const menu=ast.children.find((n:any)=>n.tag==='div'&&n.props.some((p:any)=>p.name==='id'&&p.value?.content==='languagePicker')).children.find((n:any)=>n.tag==='div');
  assert.deepEqual(normalize(menu.loc.source),normalize('<div class="languageMenu" id="languageMenu" role="listbox" aria-label="选择语言">'+old('#languageMenu').innerHTML+'</div>'));
  assert.ok(html.includes(old('#languageCurrentFlag').innerHTML));assert.ok(html.includes(old('#languageCurrentName').textContent));
 }
});
test('baseline literals and external text lookup retain the original translations and IDs',async()=>{
 const c:any={TextEncoder};vm.createContext(c);vm.runInContext(source.slice(source.indexOf('const UI_LOCALES='),source.indexOf('function uiNodeSkipped'))+';globalThis.translate=(text,locale,external)=>{uiLanguage=locale;EXTERNAL_UI_MESSAGES[locale]=external;return uiMessage(text)};',c);
 for(const locale of ['zh-CN','zh-Hant','en-US','fr-FR','ru-RU','ja-JP','es-ES','th-TH']){
  const external:Record<string,string>={};if(!['zh-CN','zh-Hant','en-US'].includes(locale)){const data=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r1/translations.json`,import.meta.url),'utf8'));for(const [key,value] of Object.entries(data.web))external[stableMessageID(key)]=value as string;}
  for(const text of [...Object.keys(languageData.english),'第 12 季 · 4','27 项','最新版本 v4.1.1 可用','unmapped',...Object.values(languageData.names[locale])])assert.equal(message(text,locale,external),c.translate(text,locale,external));
 }
});
test('current v5 version is shown while unchanged baseline translation IDs remain usable',()=>{
 const source='启动 v5.0.0 前，需要分别处理下列旧程序、后台组件或网页卡片。程序只处理能够严格确认属于旧版 Card 软件的项目；修改 Emby 文件前会建立并验证备份。';
 const baseline='启动 v4.1.0 前，需要分别处理下列旧程序、后台组件或网页卡片。程序只处理能够严格确认属于旧版 Card 软件的项目；修改 Emby 文件前会建立并验证备份。';
 const english=languageData.english[baseline];
 assert.ok(english);
 assert.equal(message(source,'en-US'),english.replace('v4.1.0','v5.0.0'));
 assert.equal(message(source,'fr-FR',{[stableMessageID(english)]:'Avant de démarrer v4.1.0, traiter les composants hérités.'}),'Avant de démarrer v5.0.0, traiter les composants hérités.');
});
test('macOS and Linux console hints reuse the original translated message without retaining Chinese text',async()=>{
 const baseline='Windows 只读索引媒体库 NFO 的';
 for(const locale of ['zh-CN','zh-Hant','en-US','fr-FR','ru-RU','ja-JP','es-ES','th-TH']){
  const external:Record<string,string>={};
  if(!['zh-CN','zh-Hant','en-US'].includes(locale)){
   const data=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r1/translations.json`,import.meta.url),'utf8'));
   for(const [key,value] of Object.entries(data.web))external[stableMessageID(key)]=value as string;
  }
  const translated=message(baseline,locale,external);
  for(const platform of ['macOS','Linux'])assert.equal(message(platform+' 只读索引媒体库 NFO 的',locale,external),translated.replace(/^Windows/,platform));
 }
});
test('platform update instructions are complete in built-in languages and fall back to English for unchanged external packs',async()=>{
 const sources=[
  '。请先从菜单栏完全退出 Tech Card Manager，再打开 DMG 并替换现有应用。',
  '。请先从系统托盘完全退出 Tech Card Manager，再运行安装包完成升级。',
  '。请先从系统托盘完全退出 Tech Card Manager，再替换当前 AppImage 文件。',
  '请保留已有配置、日志、备份及其他用户文件夹。',
 ];
 const traditional=[
  '。請先從選單列完全退出 Tech Card Manager，再開啟 DMG 並替換現有應用程式。',
  '。請先從系統匣完全退出 Tech Card Manager，再執行安裝程式完成升級。',
  '。請先從系統匣完全退出 Tech Card Manager，再替換目前的 AppImage 檔案。',
  '請保留現有設定、日誌、備份及其他使用者資料夾。',
 ];
 assert.deepEqual(sources.map(text=>message(text,'zh-Hant')),traditional);
 assert.deepEqual(sources.map(text=>message(text,'en-US')),Object.values(languageData.v5_english).slice(0,4));
 const path='/Applications/Tech Card Manager.app';
 assert.equal(message('当前程序目录：'+path,'zh-Hant'),'目前應用程式目錄：'+path);
 assert.equal(message('当前程序目录：'+path,'en-US'),'Current application folder: '+path);
 for(const locale of ['fr-FR','ru-RU','ja-JP','es-ES','th-TH']){
  const data=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r1/translations.json`,import.meta.url),'utf8')),external:Record<string,string>={};
  for(const [key,value] of Object.entries(data.web))external[stableMessageID(key)]=value as string;
  for(const text of [...sources,'当前程序目录：'+path])assert.doesNotMatch(message(text,locale,external),/[\u3400-\u9fff]/);
 }
});
test('v5 recovery messages remain complete in built-in locales and use the original external-pack English fallback',async()=>{
 const sources=['已暂停','执行中断','已取消','已有维护操作尚未确认，请先完成原操作。','系统迁移回执与已确认清单不一致。','Emby 安装目录已改变，请重新检查。','尚未取得 Emby 安装目录访问权限。','系统设置结果尚未确认，请重新读取后核对','维护操作已回滚，请重新确认。','服务状态尚未确认，请稍后重试','已有操作正在进行，请稍后重试'];
 const han=/[\u3400-\u9fff]/u;
 for(const source of sources){assert.equal(message(source,'en-US'),languageData.v5_english[source]);assert.equal(message(source,'zh-Hant'),languageData.v5_traditional[source]);}
 for(const locale of ['fr-FR','ru-RU','ja-JP','es-ES','th-TH']){
  const data=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r1/translations.json`,import.meta.url),'utf8')),external:Record<string,string>={};
  for(const [key,value] of Object.entries(data.web))external[stableMessageID(key)]=value as string;
  for(const source of sources)assert.doesNotMatch(message(source,locale,external),han);
 }
});
test('failed downloads keep the prior locale, clear the spinner and read back persisted state',async()=>{
 const calls:any[]=[];const messages:string[]=[];const model=new Languages(async<T>(name,args)=>{calls.push([name,args]);if(name==='choose_language')throw {message:'语言包摘要验证失败'};const value=snapshot();value.options[3].state='failed';return value as T;},message=>messages.push(message));
 assert.equal(await model.choose('fr-FR'),false);assert.equal(model.snapshot.locale,'zh-CN');assert.equal(model.snapshot.options[3].state,'failed');assert.equal(model.downloading,'');assert.equal(model.busy,false);assert.deepEqual(calls.map(c=>c[0]),['choose_language','language_status']);assert.deepEqual(messages,['语言包摘要验证失败']);
});
test('startup restoration is coalesced, allows language selection and reloads the current state',async()=>{
 let complete!:(s:LanguageSnapshot)=>void;let current=snapshot();const calls:string[]=[];
 const model=new Languages(async<T>(name)=>{calls.push(name);if(name==='restore_language_packs')return await new Promise<LanguageSnapshot>(r=>complete=r) as T;if(name==='choose_language'){current=snapshot('en-US');return current as T;}return current as T;},()=>{});
 const restoring=model.restore();await model.restore();assert.equal(model.busy,false);assert.equal(await model.choose('en-US'),true);complete(snapshot('fr-FR'));await restoring;assert.equal(model.snapshot.locale,'en-US');assert.deepEqual(calls,['restore_language_packs','choose_language','language_status']);
 const late=model.restore();model.dispose();complete(snapshot('fr-FR'));await late;assert.equal(calls.at(-1),'restore_language_packs');assert.equal(model.snapshot.locale,'en-US');
});
test('duplicate choices and stale status responses cannot undo a successful language change',async()=>{
 let read!:(s:LanguageSnapshot)=>void,choose!:(s:LanguageSnapshot)=>void;let calls=0;const model=new Languages(async<T>(name)=>{calls++;return await new Promise<LanguageSnapshot>(r=>{if(name==='language_status')read=r;else choose=r;}) as T;},()=>{});
 const pending=model.read(),changed=model.choose('en-US');assert.equal(await model.choose('zh-Hant'),false);choose(snapshot('en-US'));assert.equal(await changed,true);read(snapshot());await pending;assert.equal(model.snapshot.locale,'en-US');assert.equal(calls,2);
 const next=model.choose('fr-FR'),during=model.read();choose(snapshot('fr-FR'));assert.equal(await next,true);read(snapshot('en-US'));await during;assert.equal(model.snapshot.locale,'fr-FR');
 const late=model.read();model.dispose();read(snapshot('zh-Hant'));await late;assert.equal(model.snapshot.locale,'fr-FR');
 const before=calls;await model.read();assert.equal(await model.choose('zh-CN'),false);assert.equal(calls,before);
});
test('translation round trips preserve inputs, user content and new Vue text while releasing the observer',()=>{
 class Text {nodeType=3;parentElement:El|null=null;data:string;constructor(data:string){this.data=data;}}
 class El {nodeType=1;parentElement:El|null=null;childNodes:(El|Text)[]=[];attrs:Record<string,string>={};value='';scrollTop=0;skip=false;tagName:string;constructor(tagName:string){this.tagName=tagName;}append(...nodes:(El|Text)[]){for(const node of nodes){node.parentElement=this;this.childNodes.push(node);}}closest(){return this.skip?this:this.parentElement?.closest()||null;}hasAttribute(k:string){return k in this.attrs;}getAttribute(k:string){return this.attrs[k]??null;}setAttribute(k:string,v:string){this.attrs[k]=v;}}
 const previous=globalThis.MutationObserver;let callback:MutationCallback=()=>{};let disconnected=false;
 globalThis.MutationObserver=class {constructor(fn:MutationCallback){callback=fn;}observe(){}takeRecords(){return [];}disconnect(){disconnected=true;}} as any;
 try{const body=new El('BODY'),button=new El('BUTTON'),text=new Text('设置'),input=new El('INPUT'),user=new El('CODE'),protectedText=new Text('电影 设置 / data.nfo');user.skip=true;user.append(protectedText);button.attrs.title='打开设置';button.append(text);input.value='Casino Royale';input.scrollTop=73;body.append(button,input,user);
  const doc={body,documentElement:{lang:''},defaultView:{dispatchEvent(){}}};const model=new Languages(async()=>assert.fail(),()=>{});const adapter=translateDocument(doc as any,model);adapter.refresh();
  for(const locale of ['en-US','zh-Hant','zh-CN'] as Locale[]){model.snapshot=snapshot(locale);adapter.refresh();assert.equal(text.data,message('设置',locale));assert.equal(button.attrs.title,message('打开设置',locale));assert.equal(protectedText.data,'电影 设置 / data.nfo');assert.equal(input.value,'Casino Royale');assert.equal(input.scrollTop,73);}
  model.snapshot=snapshot('en-US');adapter.refresh();text.data='停止';callback([{type:'characterData',target:text}] as any,{} as any);assert.equal(text.data,'Stop');
  model.snapshot=snapshot('zh-CN');text.data='Vue 新状态';adapter.refresh();assert.equal(text.data,'Vue 新状态');adapter.dispose();assert.ok(disconnected);
 }finally{globalThis.MutationObserver=previous;}
});

test('r2 translates every v5 adaptation message in all five external locales while retaining r1 keys',async()=>{
 for(const locale of ['fr-FR','ru-RU','ja-JP','es-ES','th-TH']){
  const before=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r1/translations.json`,import.meta.url),'utf8'));
  const after=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r2/translations.json`,import.meta.url),'utf8'));
  const external=Object.fromEntries(Object.entries(after.web).map(([key,value])=>[stableMessageID(key),value]));
  for(const [source,english] of Object.entries(languageData.v5_english))assert.equal(message(source,locale,external),after.web[english]);
  for(const section of Object.keys(before))for(const key of Object.keys(before[section]))assert.ok(key in after[section]);
 }
});
