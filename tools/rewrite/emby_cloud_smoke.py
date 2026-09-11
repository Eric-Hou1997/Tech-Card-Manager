#!/usr/bin/env python3
"""Run a pinned real Emby server in an ephemeral Linux CI directory.

This establishes an environment, not TCM integration or desktop acceptance.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.request


def request(path):
    with urllib.request.urlopen('http://127.0.0.1:18096' + path, timeout=3) as response:
        return response.status, response.read()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--sha256', required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    report = {'scope': 'real-emby-environment-only', 'version': '4.9.5.0',
              'host_arch': os.uname().machine, 'checks': [],
              'tcm_integration': 'unverified', 'card_rendering': 'unverified'}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    try:
        digest = hashlib.sha256(args.package.read_bytes()).hexdigest()
        if digest != args.sha256:
            raise RuntimeError('official-package-sha256-mismatch')
        report['package_sha256'] = digest
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 18096))
        with tempfile.TemporaryDirectory(prefix='tcm-emby-') as temporary:
            work = Path(temporary)
            subprocess.run(['dpkg-deb', '-x', str(args.package), str(work / 'package')], check=True)
            server = work / 'package/opt/emby-server/system/EmbyServer'
            if not server.is_file():
                raise RuntimeError('official-package-layout-changed')
            # Official packages name their private ELF loader under /opt.
            # Resolve that loader inside the extraction; never install into host /opt.
            elf = subprocess.check_output(['readelf', '-l', str(server)], text=True)
            match = re.search(r'Requesting program interpreter: ([^\]]+)', elf)
            command = [str(server)]
            if match:
                interpreter = match.group(1)
                loader = work / 'package' / interpreter.lstrip('/')
                if loader.is_file():
                    libraries = os.pathsep.join([str(loader.parent), str(server.parent)])
                    command = [str(loader), '--library-path', libraries, str(server)]
                elif not Path(interpreter).is_file():
                    raise RuntimeError('missing-ELF-loader: ' + interpreter)
                report['elf_interpreter'] = interpreter
            data = work / 'programdata'
            (data / 'config').mkdir(parents=True)
            (data / 'config/system.xml').write_text('''<?xml version="1.0" encoding="utf-8"?>
<ServerConfiguration><HttpServerPortNumber>18096</HttpServerPortNumber>
<EnableUPnP>false</EnableUPnP><EnableAutoUpdate>false</EnableAutoUpdate>
<EnableAnonymousUsageReporting>false</EnableAnonymousUsageReporting>
<LocalNetworkAddresses><string>127.0.0.1</string></LocalNetworkAddresses>
</ServerConfiguration>''', encoding='utf-8')
            for cycle in range(2):
                log = work / ('server-' + str(cycle) + '.log')
                with log.open('wb') as output:
                    process = subprocess.Popen(command + ['-programdata', str(data)],
                                               cwd=server.parent, stdout=output, stderr=subprocess.STDOUT,
                                               start_new_session=True)
                    try:
                        deadline = time.monotonic() + 90
                        while True:
                            if process.poll() is not None:
                                raise RuntimeError('server-exited-before-ready: ' + log.read_text(errors='replace')[-4000:])
                            try:
                                status, body = request('/emby/System/Info/Public')
                                info = json.loads(body)
                                if status != 200 or info.get('Version') != report['version']:
                                    raise RuntimeError('unexpected-server-version')
                                break
                            except (OSError, ValueError):
                                if time.monotonic() >= deadline:
                                    raise RuntimeError('server-readiness-timeout: ' + log.read_text(errors='replace')[-4000:])
                                time.sleep(1)
                        status, html = request('/web/index.html')
                        if status != 200 or b'</html>' not in html.lower():
                            raise RuntimeError('web-client-not-served')
                        report['checks'].append({'cycle': cycle + 1, 'api_version': info['Version'],
                                                 'web_html_sha256': hashlib.sha256(html).hexdigest()})
                    finally:
                        forced = False
                        if process.poll() is None:
                            os.killpg(process.pid, signal.SIGTERM)
                            try:
                                process.wait(timeout=20)
                            except subprocess.TimeoutExpired:
                                forced = True
                                os.killpg(process.pid, signal.SIGKILL)
                                process.wait(timeout=10)
                        if forced:
                            raise RuntimeError('server-required-forced-shutdown')
                        try:
                            with socket.create_connection(('127.0.0.1', 18096), timeout=1):
                                raise RuntimeError('server-port-still-listening-after-stop')
                        except (ConnectionRefusedError, TimeoutError):
                            pass
            report['shutdown'] = 'two-cycles-port-closed'
        report['temporary_data'] = 'removed'
        report['status'] = 'passed'
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        raise
    finally:
        args.report.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
