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
    parser.add_argument('--integration-driver', type=Path)
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
            # The official binary uses a relative ELF interpreter, resolved from
            # the package root (the same working directory as its launcher).
            launch_directory = server.parent.parent
            elf = subprocess.check_output(['readelf', '-l', str(server)], text=True)
            match = re.search(r'Requesting program interpreter: ([^\]]+)', elf)
            if match:
                interpreter = Path(match.group(1))
                if interpreter.is_absolute() or not (launch_directory / interpreter).is_file():
                    raise RuntimeError('unexpected-ELF-loader: ' + str(interpreter))
                report['elf_interpreter'] = str(interpreter)
            # Mirror the verified official package launcher using this isolated
            # package root; default paths otherwise point at an absent /opt install.
            command = [str(server)]
            for name in ['ffdetect', 'ffmpeg', 'ffprobe']:
                binary = launch_directory / 'bin' / name
                if not binary.is_file():
                    raise RuntimeError('missing-package-media-tool: ' + name)
                command.extend(['-' + name, str(binary)])
            server_env = os.environ.copy()
            server_env["LD_LIBRARY_PATH"] = os.pathsep.join([str(launch_directory / "lib"), str(launch_directory / "extra/lib"), str(server.parent)])
            server_env["PATH"] = str(launch_directory / "bin") + os.pathsep + server_env.get("PATH", "")
            server_env["FONTCONFIG_PATH"] = str(launch_directory / "etc/fonts")
            server_env["SSL_CERT_FILE"] = str(launch_directory / "etc/ssl/certs/ca-certificates.crt")
            data = work / 'programdata'
            server_env['XDG_CACHE_HOME'] = str(data / 'cache')
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
                                               cwd=launch_directory, env=server_env, stdout=output, stderr=subprocess.STDOUT,
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
                        if cycle == 0 and args.integration_driver:
                            integration_reports = args.report.parent / 'emby-card'
                            subprocess.run(['node', 'tools/rewrite/emby_card_acceptance.mjs',
                                            str(server.parent / 'dashboard-ui'), str(work),
                                            str(args.integration_driver.resolve()), str(integration_reports.resolve())],
                                           check=True, timeout=240)
                            report['tcm_integration'] = 'passed-linux-real-server'
                            report['card_rendering'] = 'passed-chromium-real-emby-page'
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
