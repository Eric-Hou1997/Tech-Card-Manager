export interface ManagerJob {running:boolean;action:string;message?:string;exit_code?:number;log?:string;started_at?:string;language?:string}
export function jobLine(job:ManagerJob):string {
 const names:Record<string,string>={'discover-roots':'发现媒体目录',run:'刷新媒体库','scan-space':'刷新当前媒体库',diagnose:'运行诊断','repair-web':'设置/维护网页卡片','rebuild-index':'完整重建只读索引','export-diagnostics':'导出诊断包','disable-integration':'恢复原生 Emby','migrate-legacy':'迁移所列旧版组件',auto:'自动增量检查','scan-root':'目录级检查'};
 const name=names[job.action]||job.action||'';
 return job.running?'运行中 · '+name+(job.message?' · '+job.message:''):job.action?(job.exit_code?'失败':'完成')+' · '+name+(job.message?' · '+job.message:''):'任务空闲';
}
