// Real Emby API + its shipped web client. No mocked ApiClient or fabricated DOM.
import { chromium } from '../../rewrite/node_modules/playwright/index.mjs';
import { spawn, execFileSync } from 'node:child_process';
import { mkdir, writeFile, readFile, stat } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';
import path from 'node:path';
const [web, temporary, driver, reportDirectory] = process.argv.slice(2);
if (!reportDirectory) throw Error('usage: WEB_ROOT TEMPORARY_ROOT RUST_DRIVER REPORT_DIRECTORY');
const base='http://127.0.0.1:18096';
const report={scope:'real-emby-rust-integration-and-web-client',checks:[],status:'running'};
await mkdir(reportDirectory,{recursive:true});
let token='';
async function api(route,body){
 const response=await fetch(base+'/emby/'+route,{method:body===undefined?'GET':'POST',headers:{'Content-Type':'application/json','X-Emby-Authorization':'MediaBrowser Client="TCM acceptance", Device="Isolated CI", DeviceId="tcm-ci", Version="4.1.0"',...(token?{'X-Emby-Token':token}:{})},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000)});
 if(!response.ok)throw Error(`Emby ${route.split('?')[0]}: HTTP ${response.status}`);
 const text=await response.text();return text?JSON.parse(text):null;
}
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
let rust,browser,lines;const waiting=[],queued=[];
let driverError=null,driverStderr='';
function nextLine(){
 return new Promise((resolve,reject)=>{
  if(queued.length){try{resolve(JSON.parse(queued.shift()));}catch(e){reject(e);}return;}
  if(driverError){reject(driverError);return;}
  const receive=line=>{clearTimeout(timeout);try{resolve(JSON.parse(line));}catch(e){reject(e);}};
  receive.reject=error=>{clearTimeout(timeout);const index=waiting.indexOf(receive);if(index>=0)waiting.splice(index,1);reject(error);};
  const timeout=setTimeout(()=>receive.reject(Error('Rust driver response timeout')),20000);
  waiting.push(receive);
 });
}
async function command(text){const response=nextLine();rust.stdin.write(text+'\n');return response;}
try {
 const info=await api('System/Info/Public');report.version=info.Version;
 if(info.Version!=='4.9.5.0')throw Error('Unexpected Emby version');
 const configuration=await api('Startup/Configuration');await api('Startup/Configuration',{...configuration,UICulture:'en-US'});
 const password=process.env.TCM_ACCEPTANCE_PASSWORD||randomUUID();await api('Startup/User',{Name:'TCM Acceptance',Password:password});
 await api('Startup/RemoteAccess',{EnableAutomaticPortMapping:false});await api('Startup/Complete',{});
 const auth=await api('Users/AuthenticateByName',{Username:'TCM Acceptance',Pw:password});token=auth.AccessToken;
 const movieRoot=path.join(temporary,'movies'),movie=path.join(movieRoot,'TCM Acceptance (1967)');await mkdir(movie,{recursive:true});
 const nfo=path.join(movie,'movie.nfo');
 await writeFile(nfo,'\uFEFF<movie>\r\n<title>TCM Acceptance</title><year>1967</year><uniqueid type="imdb" default="true">tt0061452</uniqueid><technicalspecs source="IMDb" imdbid="tt0061452"><section name="Camera"><item>TCM ACCEPTANCE CAMERA</item></section><section name="Sound mix"><item>Mono</item></section></technicalspecs>\r\n</movie>');
 execFileSync('ffmpeg',['-loglevel','error','-f','lavfi','-i','color=c=black:s=640x360:d=2','-c:v','libx264','-pix_fmt','yuv420p',path.join(movie,'TCM Acceptance (1967).mp4')],{timeout:30000});
 const before={sha256:hash(await readFile(nfo)),mtime:(await stat(nfo,{bigint:true})).mtimeNs.toString()};
 await api('Library/VirtualFolders',{Name:'TCM Acceptance Library',CollectionType:'movies',RefreshLibrary:true,Paths:[movieRoot],LibraryOptions:{ContentType:'movies',PathInfos:[{Path:movieRoot}],EnableRealtimeMonitor:false,SaveLocalMetadata:false,MetadataSavers:[],TypeOptions:[{Type:'Movie',MetadataFetchers:[],ImageFetchers:[]}]}});
 let item;const deadline=Date.now()+90000;
 while(Date.now()<deadline){const found=await api(`Users/${auth.User.Id}/Items?Recursive=true&IncludeItemTypes=Movie&Fields=ProviderIds,MediaStreams`);item=found.Items?.find(i=>i.ProviderIds?.Imdb==='tt0061452');if(item)break;await new Promise(r=>setTimeout(r,1000));}
 if(!item)throw Error('Emby did not index the real NFO and video');
 report.checks.push('emby-library-read-real-nfo-and-video');
 const indexBefore=hash(await readFile(path.join(web,'index.html')));
 rust=spawn(driver,[web,path.join(temporary,'tcm-private'),movieRoot,path.join(temporary,'programdata')],{stdio:['pipe','pipe','pipe']});
 rust.stderr.on('data',bytes=>{driverStderr=(driverStderr+bytes.toString()).slice(-4000);});
 const driverFailed=error=>{driverError=error;for(const receive of [...waiting])receive.reject(error);};
 rust.once('error',driverFailed);rust.once('close',(code,signal)=>driverFailed(Error(`Rust driver exited (${code??signal}): ${driverStderr}`)));
 lines=createInterface({input:rust.stdout});lines.on('line',line=>{const receive=waiting.shift();if(receive)receive(line);else queued.push(line);});
 const ready=await nextLine();if(ready.phase!=='running'||ready.items!==1)throw Error('Rust business chain did not start');
 if(!ready.physical_root_discovery)throw Error('Rust did not discover the real Emby physical root');
 report.checks.push('readonly-real-emby-database-physical-root-discovery');
 for(const file of ['technical-specs-card.js','technical-specs-data.json','technical-specs-runtime.json']){
  const response=await fetch(base+'/web/'+file);if(!response.ok)throw Error('Emby did not serve '+file);
  const served=Buffer.from(await response.arrayBuffer());if(hash(served)!==hash(await readFile(path.join(web,file))))throw Error('Served bytes differ: '+file);
 }
 report.checks.push('rust-index-assets-published-and-served-byte-identical');
 browser=await chromium.launch({headless:true});const context=await browser.newContext({viewport:{width:1440,height:1000}});
 const credentials={Servers:[{Id:info.Id,Name:info.ServerName,ManualAddress:base,LocalAddress:base,LastConnectionMode:2,UserId:auth.User.Id,Users:[{UserId:auth.User.Id,AccessToken:token}],DateLastAccessed:Date.now()}]};
 await context.addInitScript(value=>{localStorage.setItem('servercredentials3',JSON.stringify(value));},credentials);
 const page=await context.newPage();
 await page.goto(`${base}/web/index.html#!/item?id=${item.Id}&serverId=${info.Id}`,{waitUntil:'domcontentloaded'});
 try{await page.waitForFunction(()=>window.__technicalSpecsDebug?.rendered===true,{},{timeout:60000});}
 catch(error){report.debug=await page.evaluate(()=>({url:location.pathname+location.hash,stage:window.__technicalSpecsDebug?.stage,reason:window.__technicalSpecsDebug?.retryReason,loaded:window.__technicalSpecsDebug?.loaded,body:document.body.innerText.slice(0,1200)}));throw error;}
 if(!(await page.getByText('TCM ACCEPTANCE CAMERA',{exact:false}).first().isVisible()))throw Error('Card value is not visible');
 await page.waitForFunction(() => {
  const cell=Array.from(document.querySelectorAll('span,div')).find(el=>el.textContent==='TCM ACCEPTANCE CAMERA');
  if(!cell)return false;
  cell.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'});
  const box=cell.getBoundingClientRect();
  return box.width>0 && box.height>0 && box.top>=0 && box.bottom<=innerHeight;
 },{}, {timeout:15000});
 // A card rendered during Emby's loading screen is insufficient evidence.
 await page.getByRole('heading',{name:'TCM Acceptance',exact:true}).waitFor({state:'visible',timeout:30000});
 await page.waitForFunction(()=>Array.from(document.querySelectorAll("[data-tech-spec-card='1']")).some(card=>{
  const row=card.parentElement;
  return card.textContent.includes('TCM ACCEPTANCE CAMERA') && row && Array.from(row.children).some(sibling=>sibling!==card&&sibling.textContent.includes('H264')) && card.getBoundingClientRect().height>0;
 }),{}, {timeout:30000});
 report.checks.push('loaded-item-native-video-and-tech-card-share-media-row');
 await page.getByText('TCM ACCEPTANCE CAMERA',{exact:false}).first().scrollIntoViewIfNeeded();
 await page.screenshot({path:path.join(reportDirectory,'card-rendered.png'),fullPage:true});report.checks.push('real-emby-item-page-rendered-card-value');
 const localeTitles={'zh-CN':'技术规格','zh-Hant':'技術規格','en-US':'Technical Specs'};
 for(const locale of ['fr-FR','ru-RU','ja-JP','es-ES','th-TH']) {
  const messages=JSON.parse(await readFile(new URL(`../../language-packs/${locale}/r1/translations.json`,import.meta.url),'utf8'));
  localeTitles[locale]=messages['web-card']['Technical Specs'];
 }
 for(const [locale,title] of Object.entries(localeTitles)) {
  await page.evaluate(locale=>{document.documentElement.lang=locale;},locale);
  await page.waitForFunction(title=>Array.from(document.querySelectorAll("[data-tech-spec-card='1']")).some(card=>card.textContent.includes(title)&&card.textContent.includes('TCM ACCEPTANCE CAMERA')),title,{timeout:20000});
 }
 report.checks.push('eight-card-locales-change-without-changing-spec-values');
 await command('stop');await page.waitForFunction(()=>!document.body.innerText.includes('TCM ACCEPTANCE CAMERA'),{},{timeout:15000});
 report.checks.push('service-stop-removes-visible-card');await page.screenshot({path:path.join(reportDirectory,'card-stopped.png'),fullPage:true});
 await command('start');await page.waitForFunction(()=>document.body.innerText.includes('TCM ACCEPTANCE CAMERA'),{},{timeout:20000});report.checks.push('service-restart-renders-card');
 await command('remove');await page.waitForFunction(()=>!document.body.innerText.includes('TCM ACCEPTANCE CAMERA'),{},{timeout:15000});
 if(hash(await readFile(path.join(web,'index.html')))!==indexBefore)throw Error('Uninstall did not restore exact Emby HTML');
 const after={sha256:hash(await readFile(nfo)),mtime:(await stat(nfo,{bigint:true})).mtimeNs.toString()};
 if(JSON.stringify(before)!==JSON.stringify(after))throw Error('NFO bytes or mtime changed');
 report.checks.push('uninstall-restores-html-and-preserves-nfo-bytes-and-mtime');report.status='passed';
} catch(error){report.status='failed';report.error=String(error);throw error;}
finally{
 if(browser)await browser.close();
 if(rust&&rust.exitCode===null){rust.stdin.end();await Promise.race([new Promise(resolve=>rust.once('exit',resolve)),new Promise(resolve=>setTimeout(resolve,5000))]);if(rust.exitCode===null)rust.kill('SIGKILL');}
 lines?.close();await writeFile(path.join(reportDirectory,'card-acceptance.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
}
