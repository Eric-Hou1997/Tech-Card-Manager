// v4.1.0 intrinsic layout and auto-hide scrollbar behavior, owned by the Vue root.
export function installBaselineLayout(root:HTMLElement):()=>void {
 let frame=0,disposed=false;
 const canvas=document.createElement('canvas'),context=canvas.getContext('2d');
 const one=(selector:string)=>root.querySelector<HTMLElement>(selector);
 const all=(selector:string)=>Array.from(root.querySelectorAll<HTMLElement>(selector));
 const length=(value:string)=>Number.parseFloat(value)||0;
 function textWidth(element:HTMLElement|null):number{
  if(!element)return 0;if(!context)return element.scrollWidth;
  context.font=getComputedStyle(element).font;
  const text=element instanceof HTMLInputElement?element.placeholder||element.value:element instanceof HTMLSelectElement?element.selectedOptions[0]?.textContent:element.textContent;
  return Math.ceil(context.measureText((text||'').trim()).width);
 }
 function box(element:HTMLElement){const style=getComputedStyle(element);return textWidth(element)+length(style.paddingLeft)+length(style.paddingRight)+length(style.borderLeftWidth)+length(style.borderRightWidth);}
 function update(){
  frame=0;if(disposed)return;
  const top=one('.top'),actions=one('.topActions');
  if(top&&actions){
   top.classList.remove('layoutStatusBelow','layoutStacked');actions.classList.remove('layoutSplit');
   const logo=one('.logo'),heading=one('.titleLine h1'),version=one('.titleLine .version'),subtitle=one('.sub');
   if(logo&&heading&&version&&subtitle){
    const brand=length(getComputedStyle(logo).width)+15+Math.max(box(heading)+box(version)+10,box(subtitle));
    const pills=all('.statusPill'),commands=all('.commandActions .btn');
    const status=pills.reduce((total,item)=>total+box(item)+15,0)+Math.max(0,pills.length-1)*8;
    const command=commands.reduce((total,item)=>total+box(item),0)+Math.max(0,commands.length-1)*8;
    const mode=brand+status+command+34<=top.clientWidth+1?'wide':brand+command+22<=top.clientWidth+1?'status-below':'stacked';
    top.classList.toggle('layoutStatusBelow',mode==='status-below');top.classList.toggle('layoutStacked',mode==='stacked');
    if(mode==='stacked')actions.classList.toggle('layoutSplit',status+command+12>top.clientWidth+1);
   }
  }
  const consoleGrid=one('.console'),service=one('.serviceCard'),metrics=one('.metrics');
  if(consoleGrid&&service&&metrics){
   consoleGrid.classList.remove('layoutFiveCards');metrics.classList.remove('layoutTwoColumns');
   const metricWidth=Math.max(135,...all('.metric .k,.metric .s').map(label=>textWidth(label)+22));
   const serviceCopy=Math.max(textWidth(one('#serviceStateText')),textWidth(one('#serviceStartedAt')));
   const serviceWidth=Math.max(300,serviceCopy+textWidth(one('#serviceButton'))+70);
   const mode=consoleGrid.clientWidth>=serviceWidth+metricWidth*4+48?'five':consoleGrid.clientWidth>=Math.max(170,metricWidth)*4+30?'four':'two';
   consoleGrid.style.setProperty('--tcm-service-preferred',serviceWidth+'px');consoleGrid.classList.toggle('layoutFiveCards',mode==='five');metrics.classList.toggle('layoutTwoColumns',mode==='two');
  }
  const tools=one('.catalogTools'),tabs=one('.catalogTabs'),search=one('#catalogSearch'),filter=one('#catalogSpecFilter');
  if(tools&&tabs&&search&&filter){tools.classList.remove('layoutCompact');tools.classList.toggle('layoutCompact',tabs.scrollWidth+Math.max(250,textWidth(search)+24)+Math.max(150,textWidth(filter)+48)+18>tools.clientWidth+1);}
  const layout=one('.catalogLayout'),list=one('#catalogList'),preview=one('#catalogPreview');
  if(layout&&list&&preview){const single=layout.clientWidth<310+300+14;layout.classList.toggle('layoutSingleColumn',single);if(single)list.style.removeProperty('--catalog-list-height');else list.style.setProperty('--catalog-list-height',Math.max(420,Math.ceil(preview.scrollHeight))+'px');}
 }
 function schedule(){if(!disposed&&!frame)frame=requestAnimationFrame(update);}
 const resize=new ResizeObserver(schedule);all('.top,.metrics,.catalogTools,.catalogLayout').forEach(element=>resize.observe(element));
 const content=new MutationObserver(schedule);content.observe(root,{subtree:true,childList:true,characterData:true});
 window.addEventListener('resize',schedule);root.addEventListener('toggle',schedule,true);
 // Delegation includes settings Teleports that mount later; all timers are released.
 const timers=new Map<Element,ReturnType<typeof setTimeout>>();
 function reveal(surface:Element){surface.classList.add('is-scrolling');clearTimeout(timers.get(surface));timers.set(surface,setTimeout(()=>{surface.classList.remove('is-scrolling');timers.delete(surface);},850));}
 function activity(event:Event){if(event instanceof KeyboardEvent&&!['ArrowDown','ArrowUp','ArrowLeft','ArrowRight','PageDown','PageUp','Home','End',' '].includes(event.key))return;const target=event.target instanceof Element?event.target:null;reveal(target?.closest('.scrollSurface')||document.activeElement?.closest('.scrollSurface')||document.documentElement);}
 function scroll(event:Event){if(event.target instanceof Element&&event.target.matches('.scrollSurface'))reveal(event.target);}
 document.addEventListener('scroll',scroll,true);document.addEventListener('wheel',activity,{passive:true});document.addEventListener('touchmove',activity,{passive:true});document.addEventListener('keydown',activity);
 void document.fonts.ready.then(schedule);schedule();
 return ()=>{disposed=true;cancelAnimationFrame(frame);resize.disconnect();content.disconnect();window.removeEventListener('resize',schedule);root.removeEventListener('toggle',schedule,true);document.removeEventListener('scroll',scroll,true);document.removeEventListener('wheel',activity);document.removeEventListener('touchmove',activity);document.removeEventListener('keydown',activity);for(const [element,timer] of timers){clearTimeout(timer);element.classList.remove('is-scrolling');}timers.clear();};
}
