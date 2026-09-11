#!/usr/bin/env python3
"""Freeze discoverable legacy entrypoints; drift check never silently accepts changes."""
import argparse, hashlib, json, pathlib, re, subprocess, sys
ROOT=pathlib.Path(__file__).resolve().parents[2]
PRODUCT='ITM' if (ROOT/'macos/engine/mac-engine.py').exists() else 'TCM'
LEGACY='macos' if PRODUCT=='ITM' else 'windows'
TARGETS=['macos-arm64','windows-x64','windows-arm64','linux-x64','linux-arm64']
# Each contract is a reviewed behavioral boundary, not an assertion that migration exists.
COMMON=[
 ('protocol','持久化和接口字段','^__protocol__$','contracts / persistence','保持字段类型、缺省值、枚举和版本迁移语义；模型在 Rust 为权威并生成 TypeScript；字段不能随语言改变。','旧配置缺失字段、未知枚举、版本升级、历史状态兼容；序列化字段不是独立用户功能。'),
 ('dispatch','操作分发和旧命令兼容','^/api/action$|^action$|^--configure$','application/commands','校验动作、参数、媒体根与运行状态；保留现有用户可观察操作；内部协议替换需要对应入口。','未知动作、缺参数、重复请求、权限失败和任务冲突。'),
 ('legacy','旧组件清理与状态迁移','legacy|cleanup','application/migration','仅处理能够证明由旧版拥有的组件和状态；预演、备份、确认及恢复路径保留。','无法确认归属、部分迁移、陈旧进程信息、配置冲突。'),
 ('locale','语言和语言包','language|locale|i18n|translate|localiz|traditional|flag|语言','locale registry / settings','选择语言；验证包完整性及版本绑定；界面与原生控件同步；任务固定启动语言；保持旧数据和机器字段。','下载失败、包不匹配、任务中切换、不支持语言回退。'),
 ('update','安装和更新','update|install|version|upgrade|更新','platform/update / settings','识别正确产品、系统、架构和包型；校验后更新；显示真实阶段；保留用户数据。','断网、限流、资产错误、签名失败、取消、升级中断及回退。'),
 ('diagnostics','诊断、日志和路径定位','diagnos|doctor|log|export|self.check|self.test|open.path|open.data|error','application/diagnostics / diagnostics','可导出诊断、打开日志/数据和受影响路径；错误包含可用的媒体身份、路径与任务。','无权限、路径不存在、导出失败；不泄露凭据。'),
 ('lifecycle','单实例、窗口、托盘和退出','terminate|orderFront|quit|exit|close|hide|show|window|tray|native|menuRestore|menuExit|heartbeat|agent|auto.start|autostart|startup|silent|headless|--agent','platform/lifecycle / native UI','单实例；最小化和恢复；显式登录启动；退出清理资源；启动偏好独立于后台运行状态。','重复启动/点击、关闭和退出区别、权限失败、进程残留、升级后启动项。'),
 ('task','任务状态与恢复','^run$|^--serve$|job|task|pause|resume|recover|retry|progress|history|busy|wait|cancel|stop|start|interval|auto','application/tasks / task center','权威任务状态、阶段、范围、进度、历史；暂停/取消/恢复遵循旧行为；重复操作不产生重复副作用。','中断、失败重试、任务冲突、退出、日志和状态持久化不一致。'),
 ('scope','媒体空间和操作范围','scope|space|movie|tv|season|episode|root|folder|library|onboard|select|selAll|selInvert|selClear|onboarding','domain/library / library setup','Movie/TV 独立根目录和状态；选择、全选/反选、季级选择及当前范围准确；全库从不默认。','空目录、越界、符号链接、共享盘离线、目录重叠、路径大小写和不同媒体种类。'),
 ('catalog','索引、筛选和状态','pipeline|scan|rebuild|catalog|index|search|filter|sort|refresh|rescan|reload|reconcile|status|cache|health|discover','application/catalog / library','索引权威数据；增量/重建范围准确；搜索过滤排序与选择状态保留；缓存容量、清理和错误状态可见。','XML 异常、缓存过期、部分根离线、外部变更、冷启动、索引重建失败。'),
 ('layout','界面布局与交互','layout|column|split|height|width|scroll|resize|toolbar|menu|context|settings|modal|dialog|button|render|toggle|tab|preview.close','presentation/layout / manager','保留列设置、排序、面板尺寸、上下文入口和响应式语义；可键盘操作；错误/空状态可见。','窄窗口、缩放、长翻译、溢出、遮挡、布局持久化和重置。')]
ITM=[
 ('ownership','标签归属和手动编辑','ownership|manifest|sidecar|manual|edit.tag|delete.tag|clear.ai.tags|clearAiTags','domain/ownership / inspector','权威 manifest 区分 External/Generated/Manual；编辑 Generated 转 Manual，External 编辑仍为 External；明确确认后才能删除外部标签。','归属未知/冲突 unsafe-skip；不得凭文字认领；嵌入先写、sidecar 后同步。'),
 ('transaction','NFO 写入、备份和撤销','atomic|undo|backup|write|fsync|source.hash|mutat','infra/nfo-transaction / inspector','完整候选验证、source hash 冲突检测、备份、fsync 和原子替换；BOM/换行/权限保留；撤销需验证后置 hash。','磁盘满、权限拒绝、外部并发修改、失效撤销、崩溃和重复请求。'),
 ('ai','AI 配置、请求和费用','ai|qwen|token|prompt|thinking|provider|budget|meter|usage|protocol|temperature|topP|jsonMode','domain/ai / AI settings','Provider/模型/协议、凭据、默认和自定义提示词保持；截断先识别；重试可解释；每次真实请求和 usage 记账；缓存本次费用为零。','截断、畸形JSON、Schema错误、鉴权、额度、限流、未知失败、重复失败跳过和显式重试。'),
 ('preview','规则/AI 预演和审批','preview|approve|preflight|generateEngine|runGeneration|generateNow|local.generate|local.rebuild','application/generation / preview','规则与 AI 入口并列；预演不写 NFO；审批绑定具体输入、范围及候选；选择项和当前空间准确。','预演后文件或参数变化、空选择、部分失败、缓存候选过期及重复审批。'),
 ('specs','IMDb 获取与规格标准化','spec|imdb|fetch|backfill|refresh.selected|refresh.all|watch.once|parse|section|technical','domain/specs / inspector','获取并结构化十类 Technical Specs；保留手动有效规格；Specs 编辑标记标签过期，不隐式刷新或重建。','无技术数据、网页变化、网络/反爬失败、缓存重解析、错误 IMDb ID。'),
 ('inspector','Inspector 与问题处理','inspector|detail|issue|acknowledge|override','application/inspector / inspector','展示全部根标签、XML状态和归属；问题确认与手动状态覆盖绑定源hash；刷新恢复真实状态。','文件变化后旧确认失效、失效状态覆盖、解析异常及外部标签保护。')]
TCM=[
 ('webpatch','Emby 安装、修复、移除与回滚','patch|repair|install|disable.integration|managed|restore|baseline|backup|transaction|atomic|durable','application/emby-maintenance / maintenance','验证准确目标和归属；提供维护计划、确认与最小提权；外部可信备份、候选校验、事务替换与后置验证。','中途失败、并发修改、备份缺失、未知标记、权限拒绝、重复维护和日志恢复。'),
 ('legacy','历史组件识别与迁移','legacy|migrat|cleanup','application/legacy / migration','只凭路径/命令行/manifest/标记等证据识别历史组件；列明操作、确认后重验目标；保护未知归属。','PID复用、同名进程、未知patch、部分迁移失败、取消和回滚。'),
 ('card','Web Card 数据、渲染与失效','card|lease|runtime|teardown|observer|watchdog|route|detail|native.video|target|spec','domain/card / Emby adapter','只发布允许字段；路由、条目类型和可见详情一致；多语言/布局正确；管理端退出后卡片租约按契约失效。','旧脚本卸载、路由变化、后台页面、公开数据不得含本地路径、服务停止/崩溃/离线。'),
 ('service','服务状态和资源所有权','service|start|stop|heartbeat|interval|session|sequence','application/service / console','请求、运行、磁盘就绪、服务可用、客户端加载和渲染状态区分；启动和停止幂等；失败不得伪报成功。','初始租约写入失败、停止写入失败可重试、重复启动、退出时同步清理。'),
 ('readonly','只读 NFO 解析和归属展示','nfo|xml|tech.object|canonical|merge.spec|read','domain/nfo-reader / inspector','读取 Movie/Series/Season/Episode；直接 section 解析与去重；仅展示 manifest 归属；保持原始 NFO 字节和 mtime。','坏XML、缺IMDb、同IMDb多条目、重复标签、无归属元数据、共享盘暂时不可读。')]
GROUPS=(ITM if PRODUCT=='ITM' else TCM)+[g for g in COMMON if not(PRODUCT=='TCM' and g[0]=='legacy')]
# Lifecycle catch-all has explicit review debt, rather than silently declaring unknown controls understood.
def classify(symbol):
    exact={'restore':'inspector','ping':'lifecycle','metrics':'diagnostics','invalidate':'catalog','CheckOnly':'diagnostics','DisableIntegration':'webpatch','bannerAction':'dispatch','data-copy':'diagnostics','data-copy-path':'diagnostics','data-check':'scope','data-ignore-kind':'inspector','data-compact-view':'layout','data-tree':'catalog','data-reorderable':'layout','data-own-badge':'ownership','data-own-choice':'ownership','data-add-manual':'ownership'}
    if symbol in exact:return exact[symbol]
    for key,_,pattern,*_ in GROUPS:
        if re.search(pattern,symbol,re.I): return key
    if symbol=='/': return 'layout'
    return 'unclassified'

def discover():
    entries=[];files={}
    paths=subprocess.check_output(['git','ls-files',LEGACY],cwd=ROOT,text=True).splitlines()
    patterns=[('delegated-control',r'\b(data-(?:add|delete|edit|restore|ignore|choose|test|remove|onboard|own|copy|open|root|scan|close|catalog|tv|season|sort|resize|reorder|compact|check|space|tab|tree)[\w-]*)'),('frontend-action',r'''(?:action|postAction)\(['"]([^'"]+)'''),('tray-action',r'\b(menu(?:Restore|Exit))\s*='),('http-route',r'HandleFunc\("([^"]+)"'),('command',r'case\s+((?:"[^"]+"(?:,\s*)?)+)\s*:'),('engine-option',r'add_argument\("([^"]+)"'),('control',r'<(?:button|input|select|textarea)\b[^>]*\bid="([^"]+)"'),('inline-action',r'(?:onclick|onchange|oninput)="([^"]+)"'),('engine-operation',r'(?:operation|cmd)\s*==\s*"([^"]+)"'),('native-action',r'@selector\(([^)]+)\)'),('serialized-field',r'`json:"([^",]+)(?:,[^"]*)?"`')]
    for relative in paths:
        path=ROOT/relative
        if '/tests/' in relative or relative.endswith('_test.go') or path.suffix not in ('.go','.py','.ps1','.html','.js','.m'):continue
        raw=path.read_bytes();files[relative]=hashlib.sha256(raw).hexdigest();s=raw.decode('utf-8-sig')
        for kind,pattern in patterns:
            for m in re.finditer(pattern,s,re.S if kind=='command' else 0):
                values=re.findall(r'"([^"]+)"',m.group(1)) if kind=='command' else [m.group(1)]
                for value in values:
                    group=classify(value)
                    if kind=='serialized-field' and group=='unclassified': group='protocol'
                    ident=hashlib.sha256((relative+'|'+kind+'|'+value).encode()).hexdigest()[:12]
                    entries.append({'id':PRODUCT+'-E-'+ident,'kind':kind,'symbol':value,'source':relative,'line':s.count('\n',0,m.start())+1,'feature':PRODUCT+'-'+group.upper(),'mapping_status':'needs-semantic-review' if group=='unclassified' else 'source-mapped-runtime-unverified'})
        if path.suffix=='.ps1':
            header=s.split(')\n',1)[0]
            for m in re.finditer(r'\[(?:switch|string)\]\$(\w+)',header,re.I):
                v=m.group(1);ident=hashlib.sha256((relative+'|engine-option|'+v).encode()).hexdigest()[:12]
                entries.append({'id':PRODUCT+'-E-'+ident,'kind':'engine-option','symbol':v,'source':relative,'line':s.count('\n',0,m.start())+1,'feature':PRODUCT+'-'+classify(v).upper(),'mapping_status':'needs-semantic-review' if classify(v)=='unclassified' else 'source-mapped-runtime-unverified'})
    entries=list({x['id']:x for x in entries}.values())
    return {'schema':1,'product':PRODUCT,'source_hashes':files,'entries':sorted(entries,key=lambda x:(x['source'],x['line'],x['symbol']))}

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--write',action='store_true');args=ap.parse_args()
    target=ROOT/'docs/rewrite/entrypoints.json';inventory=discover()
    if not args.write:
        saved=json.loads(target.read_text())
        if saved!=inventory:
            print('FAIL legacy source/entrypoints changed: review and explicitly update inventory');return 1
        features=json.loads((target.parent/'features.json').read_text())
        ids={f['id'] for f in features['features']}
        assert all(e['feature'] in ids for e in inventory['entries'])
        assert set(features['targets'])==set(TARGETS)
        print('PASS frozen inventory: %d entrypoints, %d feature contracts; semantic/runtime acceptance remains separate' % (len(inventory['entries']),len(ids)));return 0
    target.parent.mkdir(parents=True,exist_ok=True);target.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+'\n')
    groups=GROUPS+[('unclassified','待逐项语义复核','', 'pending-design','源码已冻结并定位；仍需逐项检查调用方、用户入口和运行时语义，不能计为已验收。','不得通过通用分组隐藏遗漏。')]
    features=[]
    tests=list((ROOT/LEGACY/'tests').glob('*'))+list((ROOT/LEGACY).glob('*_test.go'))
    for key,title,pattern,destination,contract,negative in groups:
        fid=PRODUCT+'-'+key.upper();anchors=[e for e in inventory['entries'] if e['feature']==fid]
        matched=sorted(str(t.relative_to(ROOT)) for t in tests if t.is_file() and t.suffix in ('.py','.js','.go') and pattern and re.search(pattern,t.name,re.I))
        features.append({'id':fid,'title':title,'contract':contract,'negative_paths':negative,'planned_destination':destination,'entrypoints':[x['id'] for x in anchors],'baseline_test_candidates':matched,'test_mapping_status':'candidate-references-not-proof','new_implementation':'not-implemented','platforms':{t:'not-implemented' for t in TARGETS}})
    (target.parent/'features.json').write_text(json.dumps({'schema':1,'targets':TARGETS,'status':'source-inventory-created-semantic-and-runtime-review-pending','features':features},ensure_ascii=False,indent=2)+'\n')
    lines=['# 原始功能对照台账','', '状态：静态入口已定位；语义复核、原版真实运行与新版功能迁移尚未全部验收。每个入口有稳定编号，不能将分组覆盖率当成功能完成率。', '', '来源：v4.1.0、当前源码、README、原生入口和现有测试。未来 Roadmap 与已实现行为分别处理。', '', '| 编号 | 功能 | 行为契约 | 异常/恢复 | 新版位置 |', '| --- | --- | --- | --- | --- |']
    for f in features:lines.append('| {id} | {title} | {contract} | {negative_paths} | {planned_destination} |'.format(**f))
    lines+=['','## 逐入口追踪','','五个目标的初始功能状态均为 not-implemented，见 features.json。现有测试只是候选证据，需执行并核对其断言。','', '| 入口编号 | 类型 | 原入口 | 源码 | 功能编号 |', '| --- | --- | --- | --- | --- |']
    for e in inventory['entries']:
        symbol=e['symbol'].replace('|','&#124;').replace('\n',' ')
        lines.append(f"| {e['id']} | {e['kind']} | `{symbol}` | [{e['source']}:{e['line']}](../../{e['source']}#L{e['line']}) | {e['feature']} |")
    lines+=['','## 尚未关闭的完整性检查','','- 动态生成菜单/控件、事件委托和原生系统分支需要逐项运行核对；正则发现器不证明其完整性。','- unclassified 项必须完成语义复核后才可关闭步骤 2。','- 修改默认行为、删除旧选项或替换内部命令必须更新台账并说明用户可观察结果。','- Rust 业务核心尚未实现，因此新旧差分仍为待执行，不伪造新版预期输出。','']
    (target.parent/'01-features.md').write_text('\n'.join(lines))
    print('wrote',len(inventory['entries']),'entries; unclassified',sum('UNCLASSIFIED' in e['feature'] for e in inventory['entries']))
    return 0
if __name__=='__main__':sys.exit(main())
