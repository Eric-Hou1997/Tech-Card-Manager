#!/usr/bin/env python3
"""Run baseline checks, preserving failures and distinguishing execution from compilation."""
import argparse, datetime, json, os, pathlib, platform, shutil, subprocess, sys, tempfile, time
ROOT=pathlib.Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--only',default='');args=p.parse_args()
product='ITM' if (ROOT/'macos/engine/mac-engine.py').exists() else 'TCM'
legacy=ROOT/('macos' if product=='ITM' else 'windows')
selected=set(args.only.split(',')) if args.only else None
records=[]
if selected and args.output.exists():
    records=[c for c in json.loads(args.output.read_text())['checks'] if c['name'] not in selected]
with tempfile.TemporaryDirectory(prefix='rewrite-baseline-') as td:
    td=pathlib.Path(td);env=os.environ.copy()
    # Isolate Python's Path.home without rewriting HOME or user shell configuration.
    (td/'sitecustomize.py').write_text('import pathlib, os\npathlib.Path.home = classmethod(lambda cls: cls(os.environ["REWRITE_BASELINE_HOME"]))\n')
    env['PYTHONPATH']=str(td)+os.pathsep+env.get('PYTHONPATH','')
    env['PYTHONDONTWRITEBYTECODE']='1';env['GOCACHE']=str(td/'go-cache')
    env['IMDB_TECH_REQUIRE_BROWSER']='1'
    logs=args.output.parent/'baseline-logs';logs.mkdir(parents=True,exist_ok=True)
    def run(name,command,kind,cwd=ROOT,timeout=240):
        if selected is not None and name not in selected: return
        env['REWRITE_BASELINE_HOME']=str(td/name/'user')
        start=time.monotonic()
        try:
            result=subprocess.run(command,cwd=cwd,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=timeout)
            code=result.returncode;output=result.stdout;status='passed' if code==0 else 'failed'
            if code==0 and 'SKIP ' in output: status='partial'
        except subprocess.TimeoutExpired as e:
            code=None;output=str(e);status='timeout'
        except OSError as e:
            code=None;output=str(e);status='blocked'
        (logs/(name+'.txt')).write_text(output)
        records.append({'name':name,'command':command,'kind':kind,'status':status,'exit_code':code,'seconds':round(time.monotonic()-start,2)})
        print(name,status,flush=True)
    for path in sorted((legacy/'tests').glob('*.py')):
        run(path.stem,[sys.executable,str(path)],'legacy-contract-mixed-static-and-behavior')
    if product=='ITM':
        run('engine-self-test',[sys.executable,str(legacy/'engine/mac-engine.py'),'--self-test'],'engine-behavior')
        run('writer-fault-injection',[sys.executable,str(ROOT/'tools/rewrite/itm_behavior.py')],'original-engine-behavior')
        for name,cmd in [('go-vet',['go','vet','./...']),('go-test',['go','test','./...']),('go-race',['go','test','-race','./...']),('go-build',['go','build','-o',str(td/'core'),'.'])]:
            run(name,cmd,'native-'+name,cwd=legacy,timeout=300)
        for file in ['IMDbTechManagerLauncher.m','IMDbWebKitFetcher.m']:
            run(file,['clang','-fobjc-arc','-fmodules','-fmodules-cache-path='+str(td/'clang'),'-mmacosx-version-min=12.0','-fsyntax-only',str(legacy/'native'/file)],'native-syntax-only')
    else:
        for name in ['i18n_runtime_regression.js','responsive_layout_regression.js']:
            run(name,['node',str(legacy/'tests'/name)],'javascript-runtime-or-dom-as-reported')
        run('web-card-syntax',['node','--check',str(legacy/'engine/technical-specs-card.js')],'syntax-only')
        if platform.system()=='Windows':
            run('go-test',['go','test','./...'],'native-windows-tests',cwd=legacy)
            run('original-powershell-behavior',['powershell.exe','-NoProfile','-NonInteractive','-File',str(ROOT/'tools/rewrite/tcm_behavior.ps1')],'original-powershell-behavior')
        else:
            env['GOOS']='windows';env['GOARCH']='amd64';env['CGO_ENABLED']='0'
            run('windows-go-test-compile',['go','test','-c','-o',str(td/'tests.exe'),'.'],'cross-compile-only',cwd=legacy,timeout=300)
            run('windows-go-vet',['go','vet','./...'],'cross-vet-only',cwd=legacy)
            records.extend([{'name':name,'status':'blocked','kind':kind,'reason':'Requires actual Windows; no successful execution inferred from compilation'} for name,kind in [('windows-native-tests','native-windows-tests'),('original-powershell-behavior','original-powershell-behavior')]])
report={'schema':1,'product':product,'timestamp':datetime.datetime.now(datetime.timezone.utc).isoformat(),'host':{'system':platform.system(),'machine':platform.machine(),'python':platform.python_version()},'baseline_commit':subprocess.check_output(['git','rev-parse','v4.1.0^{}'],cwd=ROOT,text=True).strip(),'checks':records,'legacy_runtime_unchanged':not any('/tests/' not in f and not f.endswith('_test.go') for f in subprocess.check_output(['git','diff','--name-only','v4.1.0','--',str(legacy.relative_to(ROOT))],cwd=ROOT,text=True).splitlines()),'limitations':['No new Rust business core exists yet; differential new/old parity is pending.','A passed mixed contract may include source assertions. See each test; it is not full desktop acceptance.','TCM Windows PowerShell, Emby and service integration require the Windows gate.']}
args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({s:sum(x['status']==s for x in records) for s in sorted({x['status'] for x in records})}))
sys.exit(1 if any(x['status'] in ('failed','timeout') for x in records) else 0)
