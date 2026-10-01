import data from './assets/baseline-languages.json' with {type:'json'};
import type {LanguageOption,LanguageSnapshot,Locale} from './contracts';
export const languageData=data as {english:Record<string,string>;v5_english:Record<string,string>;v5_traditional:Record<string,string>;traditional:string[][];names:Record<string,Record<string,string>>;options:Omit<LanguageOption,'error'>[];flags:Record<string,string>};
export const languageKey='tcm-baseline-language';
type Invoke=<T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
export function stableMessageID(value:string){let hash=14695981039346656037n;for(const byte of new TextEncoder().encode(value.trim())){hash^=BigInt(byte);hash=BigInt.asUintN(64,hash*1099511628211n);}return 'legacy.'+hash.toString(16).padStart(16,'0');}
export function message(source:string,locale:string,external:Record<string,string>={}):string {
 const current='v5.0.0',baseline='v4.1.0',versioned=source.includes(current);
 if(versioned)source=source.replaceAll(current,baseline);
 const platform=/^(macOS|Linux) 只读索引媒体库 NFO 的$/.exec(source)?.[1];
 if(platform)source=source.replace(/^(macOS|Linux)/,'Windows');
 const finish=(value:string)=>{
  if(versioned)value=value.replaceAll(baseline,current);
  return platform?value.replace(/^Windows/,platform):value;
 };
 if(locale==='zh-CN')return finish(source);
 if(locale==='zh-Hant'){
  const exact=languageData.v5_traditional[source];if(exact)return finish(exact);
  for(const [from,to] of Object.entries(languageData.v5_traditional).sort((a,b)=>b[0].length-a[0].length))if(from&&source.includes(from))source=source.split(from).join(to);
  for(const [from,to] of languageData.traditional)source=source.replaceAll(from,to);return finish(source);
 }
 const english=languageData.english[source]||languageData.v5_english[source]||source,translated=external[stableMessageID(english)];if(translated)return finish(translated);
 if(languageData.english[source]||languageData.v5_english[source])return finish(english);
 for(const [from,to] of Object.entries({...languageData.english,...languageData.v5_english}).sort((a,b)=>b[0].length-a[0].length))if(from&&source.includes(from))source=source.split(from).join(to);
 return finish(source.replace(/第\s*([^\s]+)\s*季/g,'Season $1').replace(/(^|\s)(\d+)\s*项(?=$|\s)/g,'$1$2 items'));
}
export class Languages {
 snapshot:LanguageSnapshot={locale:'zh-CN',options:languageData.options.map(o=>({...o,error:null})),web_messages:{},native_messages:{}};
 busy=false;downloading='';ready=false;alive=true;
 private generation=0;
 private restoring=false;
 private invoke:Invoke;private notify:(message:string)=>void;
 constructor(invoke:Invoke,notify:(message:string)=>void){this.invoke=invoke;this.notify=notify;}
 text(source:string){return message(source,this.snapshot.locale,this.snapshot.web_messages as Record<string,string>);}
 async read(){if(!this.alive)return;const generation=++this.generation;try{const value=await this.invoke<LanguageSnapshot>('language_status');if(this.alive&&generation===this.generation){this.snapshot=value;this.ready=true;}}catch(error){if(this.alive&&generation===this.generation)this.notify((error as {message?:string})?.message||String(error));}}
 async restore(){
  if(!this.alive||this.restoring)return;this.restoring=true;
  try{await this.invoke('restore_language_packs');if(this.alive)await this.read();}
  catch(error){if(this.alive)this.notify((error as {message?:string})?.message||String(error));}
  finally{this.restoring=false;}
 }
 async choose(locale:Locale):Promise<boolean>{
  const option=this.snapshot.options.find(o=>o.code===locale);if(!this.alive||this.busy||!option||!option.installed&&!option.built_in&&!option.downloadable)return false;
  ++this.generation;this.busy=true;this.downloading=!option.installed&&!option.built_in?locale:'';
  try{const value=await this.invoke<LanguageSnapshot>('choose_language',{locale});if(!this.alive)return false;++this.generation;this.snapshot=value;this.notify(this.text('应用设置已保存'));return true;}
  catch(error){if(this.alive){this.notify((error as {message?:string})?.message||String(error));await this.read();}return false;}
  finally{if(this.alive){this.busy=false;this.downloading='';}}
 }
 dispose(){this.alive=false;}
}
/** Same presentation boundaries as 4.1.0: values, paths and user text are excluded.
 * Vue owns product state; this adapter only translates text and presentation attributes. */
export function translateDocument(doc:Document,languages:Languages){
 let previousLocale=languages.snapshot.locale,previousMessages=languages.snapshot.web_messages as Record<string,string>;
 const sources=new WeakMap<Text,string>(),attributes=new WeakMap<Element,Record<string,string>>();
 const skip='#joblog,code,[data-i18n-user],.path,.specValues,.tagList,.catalogItem strong,#catalogPreview .rowHead strong,#rootEditor .rowHead strong';
 const skipped=(node:Node)=>{const parent=node.nodeType===3?node.parentElement:node as Element;return !parent||!!parent.closest(skip)||['SCRIPT','STYLE'].includes(parent.tagName);};
 function text(node:Text){if(skipped(node))return;let source=sources.get(node);if(source===undefined||node.data!==languages.text(source)){source=node.data;sources.set(node,source);}const next=languages.text(source);if(node.data!==next)node.data=next;}
 function element(node:Element){if(skipped(node))return;let saved=attributes.get(node);if(!saved){saved={};attributes.set(node,saved);}for(const attr of ['placeholder','title','aria-label']){if(!node.hasAttribute(attr))continue;const value=node.getAttribute(attr)!;if(!(attr in saved)||value!==languages.text(saved[attr]))saved[attr]=value;const next=languages.text(saved[attr]);if(value!==next)node.setAttribute(attr,next);}for(const child of Array.from(node.childNodes)){if(child.nodeType===3)text(child as Text);else if(child.nodeType===1)element(child as Element);}}
 const observer=new MutationObserver(records=>{for(const record of records){if(record.type==='characterData')text(record.target as Text);else if(record.type==='attributes')element(record.target as Element);else for(const node of Array.from(record.addedNodes)){if(node.nodeType===3)text(node as Text);else if(node.nodeType===1)element(node as Element);}}});
 observer.observe(doc.body,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['placeholder','title','aria-label']});
 return {refresh(){
  // Reapply the original source rather than treating text from the prior language as new input.
  function restore(node:Element){if(skipped(node))return;const attrs=attributes.get(node);if(attrs)for(const [key,value] of Object.entries(attrs)){if(node.getAttribute(key)===message(value,previousLocale,previousMessages))node.setAttribute(key,value);else if(node.hasAttribute(key))attrs[key]=node.getAttribute(key)!;}for(const child of Array.from(node.childNodes)){if(child.nodeType===3){const source=sources.get(child as Text);if(source!==undefined&&(child as Text).data===message(source,previousLocale,previousMessages))(child as Text).data=source;else sources.set(child as Text,(child as Text).data);}else if(child.nodeType===1)restore(child as Element);}}
  observer.takeRecords();restore(doc.body);previousLocale=languages.snapshot.locale;previousMessages=languages.snapshot.web_messages as Record<string,string>;doc.documentElement.lang=languages.snapshot.locale;element(doc.body);observer.takeRecords();doc.defaultView?.dispatchEvent(new Event('resize'));
 },dispose(){observer.disconnect();}};
}
