#!/usr/bin/env python3
"""Isolated official Emby server on Windows or macOS, with real Rust/card checks."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import socket
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--package',type=Path,required=True)
    parser.add_argument('--sha256',required=True)
    parser.add_argument('--driver',type=Path,required=True)
    parser.add_argument('--reports',type=Path,required=True)
    args=parser.parse_args()
    args.reports.mkdir(parents=True,exist_ok=True)
    report={'os':platform.system(),'host_arch':platform.machine(),'server_arch':'x64' if os.name=='nt' else 'arm64','version':'4.9.5.0','status':'running'}
    process=None
    try:
        if hashlib.sha256(args.package.read_bytes()).hexdigest()!=args.sha256:raise RuntimeError('official-package-hash-mismatch')
        with socket.socket() as probe:probe.bind(('127.0.0.1',18096))
        with tempfile.TemporaryDirectory(prefix='tcm-emby-') as temporary:
            root=Path(temporary).resolve();package=root/'package';package.mkdir()
            if os.name=='nt':
                subprocess.run(['7z','x',str(args.package.resolve()),'-o'+str(package),'-y'],check=True,stdout=subprocess.DEVNULL)
                candidates=list(package.rglob('EmbyServer.exe'))
            else:
                subprocess.run(['ditto','-x','-k',str(args.package.resolve()),str(package)],check=True)
                candidates=[p for p in package.rglob('EmbyServer') if p.is_file() and os.access(p,os.X_OK)]
            if len(candidates)!=1:raise RuntimeError('ambiguous-server-layout: '+str([str(p.relative_to(package)) for p in candidates]))
            server=candidates[0]
            web_candidates=[p.parent for p in package.rglob('index.html') if p.parent.name=='dashboard-ui']
            if len(web_candidates)!=1:raise RuntimeError('ambiguous-web-root')
            web=web_candidates[0]
            report['server_relative_path']=str(server.relative_to(package))
            data=root/'programdata';(data/'config').mkdir(parents=True)
            (data/'config/system.xml').write_text('''<?xml version="1.0" encoding="utf-8"?><ServerConfiguration>
<HttpServerPortNumber>18096</HttpServerPortNumber><EnableUPnP>false</EnableUPnP><EnableAutoUpdate>false</EnableAutoUpdate>
<EnableAnonymousUsageReporting>false</EnableAnonymousUsageReporting><LocalNetworkAddresses><string>127.0.0.1</string></LocalNetworkAddresses>
</ServerConfiguration>''',encoding='utf-8')
            for cycle in range(2):
                with (root/('server-'+str(cycle)+'.log')).open('wb') as output:
                    options={'creationflags':subprocess.CREATE_NEW_PROCESS_GROUP} if os.name=='nt' else {'start_new_session':True}
                    process=subprocess.Popen([str(server),'-programdata',str(data)],cwd=server.parent,stdout=output,stderr=subprocess.STDOUT,**options)
                    try:
                        deadline=time.monotonic()+90
                        while True:
                            if process.poll() is not None:raise RuntimeError('Emby-exited-before-ready: '+(root/('server-'+str(cycle)+'.log')).read_text(errors='replace')[-3000:])
                            try:
                                with urllib.request.urlopen('http://127.0.0.1:18096/emby/System/Info/Public',timeout=2) as response:info=json.load(response)
                                if info['Version']!='4.9.5.0':raise RuntimeError('wrong-server-version')
                                break
                            except (OSError,ValueError):
                                if time.monotonic()>deadline:raise RuntimeError('Emby-start-timeout')
                                time.sleep(1)
                        if cycle==0:
                            subprocess.run(['node','tools/rewrite/emby_card_acceptance.mjs',str(web),str(root),str(args.driver.resolve()),str(args.reports.resolve())],check=True,timeout=240)
                            report['card_chain']='passed'
                    finally:
                        if process.poll() is None:
                            if os.name=='nt':process.send_signal(signal.CTRL_BREAK_EVENT)
                            else:os.killpg(process.pid,signal.SIGTERM)
                            try:process.wait(timeout=20)
                            except subprocess.TimeoutExpired:
                                process.kill();process.wait(timeout=10);raise RuntimeError('Emby-required-forced-shutdown')
                        try:
                            with socket.create_connection(('127.0.0.1',18096),timeout=1):raise RuntimeError('server-port-still-open')
                        except (ConnectionRefusedError,TimeoutError):pass
            report['restart_and_cleanup']='passed'
        report['status']='passed'
    except Exception as error:
        report['status']='failed';report['error']=str(error);raise
    finally:
        (args.reports/'server-acceptance.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        print(json.dumps(report))

if __name__=='__main__':main()
