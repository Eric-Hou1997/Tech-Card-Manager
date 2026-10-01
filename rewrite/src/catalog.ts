import type { ManagerRow, MediaItem, Space } from './contracts';

// Translated from v4.1.0 catalogMatches/renderTvTree/orderedSpecs. These are
// presentation operations; NFO reads and authoritative ownership remain in Rust.
export type CatalogItem = MediaItem & {series_title: string};
export const fieldOrder = ['Runtime','Sound mix','Color','Aspect ratio','Camera','Laboratory','Film Length','Negative Format','Cinematographic Process','Printed Film Format'];
const fieldLabels: Record<string,string> = {'Runtime':'正片时长','Sound mix':'声音制式','Color':'色彩类型','Aspect ratio':'画幅比例','Camera':'摄影器材','Laboratory':'冲印流程','Film Length':'胶片长度','Negative Format':'底片格式','Cinematographic Process':'摄影工艺','Printed Film Format':'放映格式'};
export function fieldLabel(key:string,locale:string){return locale==='en-US'?key:fieldLabels[key]||key;}
export function catalogItems(rows:ManagerRow[]):CatalogItem[]{return rows.map(row=>({...row.item,series_title:row.series_title}));}
export function displayTitle(row:MediaItem):string{return row.title||row.show_title||row.original_title||row.path.split(/[\\/]/).pop()?.replace(/\.nfo$/i,'')||'未命名 NFO';}
export function stateLabel(row:MediaItem):string{return row.error?'解析异常':Object.keys(row.specs).length?'技术规格正常':'缺少技术规格';}
export function matches(row:CatalogItem,space:Space,search:string,filter:string):boolean {
 const query=search.trim().toLocaleLowerCase();
 if(row.space!==space)return false;
 if(filter==='ready'&&(row.error||!Object.keys(row.specs).length))return false;
 if(filter==='missing'&&(row.error||Object.keys(row.specs).length))return false;
 if(filter==='error'&&!row.error)return false;
 return !query||[row.title,row.original_title,row.show_title,row.series_title,row.year,row.imdb,row.path].join(' ').toLocaleLowerCase().includes(query);
}
export function emptyMessage(rows:CatalogItem[],space:Space,search:string,indexError?:string|null):string {
 if(search.trim())return '没有找到匹配的 NFO。';
 if(rows.length&&!rows.some(row=>row.space===space))return space==='movie'?'索引中尚无电影 NFO。':'索引中尚无电视剧 NFO。';
 if(rows.length)return '当前状态筛选没有结果。';
 if(indexError)return '索引尚不可用：'+indexError;
 return '尚未索引到 NFO。请先在设置中选择媒体目录。';
}
export function orderedSpecs(specs:MediaItem['specs']):[string,string[]][]{return fieldOrder.flatMap(key=>{
 const values=(specs[key]||[]).map(value=>String(value||'').trim()).filter(Boolean);
 return values.length?[[key,values] as [string,string[]]]:[];
});}
export interface CatalogLeaf {item:CatalogItem;depth:number;stripe:boolean}
export interface CatalogSeason {id:string;label:string;rows:CatalogLeaf[]}
export interface CatalogShow {id:string;name:string;count:number;rows:CatalogLeaf[];seasons:CatalogSeason[]}
export function tvGroups(rows:CatalogItem[],locale:string):CatalogShow[]{
 const groups=new Map<string,CatalogItem[]>();let stripe=0;
 const leaf=(item:CatalogItem,depth:number):CatalogLeaf=>({item,depth,stripe:Boolean(stripe++%2)});
 for(const row of rows){const show=row.series_title||row.show_title||(row.kind==='Series'?row.title:'')||'未归属节目';const items=groups.get(show)||[];items.push(row);groups.set(show,items);}
 return [...groups].sort((a,b)=>a[0].localeCompare(b[0],locale)).map(([show,items])=>({
  id:show,name:show,count:items.length,rows:items.filter(row=>row.kind==='Series').map(row=>leaf(row,0)),
  seasons:[...new Set(items.filter(row=>row.kind==='Season'||row.kind==='Episode').map(row=>String(row.season||'?')))].sort((a,b)=>(+a||9999)-(+b||9999)).map(season=>({
   id:show+'␟'+season,label:season==='?'?'未标注季':`第 ${season} 季`,rows:[
    ...items.filter(row=>row.kind==='Season'&&String(row.season||'?')===season).map(row=>leaf(row,1)),
    ...items.filter(row=>row.kind==='Episode'&&String(row.season||'?')===season).sort((a,b)=>(+a.episode||0)-(+b.episode||0)).map(row=>leaf(row,2)),
   ],
  })),
 }));
}
