#!/usr/bin/env python3
"""CI-only package startup check. This does not certify OTA or full desktop UX."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
PRODUCT = 'ITM' if (ROOT / 'macos/engine/mac-engine.py').exists() else 'TCM'


def launch(binary, work, expected_arch):
    events = work / 'events.jsonl'
    events.unlink(missing_ok=True)
    env = os.environ.copy()
    env.update(REWRITE_PROBE_REPORT=str(events), REWRITE_PROBE_AUTOCLOSE='1')
    if os.name != 'nt':
        env['APPIMAGE_EXTRACT_AND_RUN'] = '1'
    with (work / 'app.log').open('wb') as output:
        child = subprocess.Popen([str(binary)], env=env, stdout=output, stderr=subprocess.STDOUT,
                                 start_new_session=os.name != 'nt')
        try:
            result = child.wait(timeout=60)
        except subprocess.TimeoutExpired:
            if os.name == 'nt':
                subprocess.run(['taskkill', '/PID', str(child.pid), '/T', '/F'], check=False)
            else:
                os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=10)
            raise RuntimeError('packaged-app-startup-or-shutdown-timeout')
    if result != 0:
        raise RuntimeError('packaged-app-exit-' + str(result) + ': ' + (work / 'app.log').read_text(errors='replace')[-2000:])
    records = [json.loads(line) for line in events.read_text(encoding='utf-8').splitlines()]
    if [r['event'] for r in records] != ['native-setup-complete', 'frontend-mounted-ipc-roundtrip', 'process-exit']:
        raise RuntimeError('incomplete-lifecycle-evidence')
    if any(r['product'] != PRODUCT or r['arch'] != expected_arch for r in records):
        raise RuntimeError('runtime-product-or-architecture-mismatch')
    return records


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--target', required=True)
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Only run on disposable GitHub Actions nodes')
    windows = args.target.endswith('windows-msvc')
    expected = 'aarch64' if args.target.startswith('aarch64-') else 'x86_64'
    base = ROOT / 'rewrite/src-tauri/target' / args.target / 'release/bundle'
    packages = list(base.glob('nsis/*-setup.exe' if windows else 'appimage/*.AppImage'))
    if len(packages) != 1:
        raise RuntimeError('expected-one-validation-package')
    package = packages[0]
    report = {'product': PRODUCT, 'target': args.target, 'package': package.name,
              'sha256': hashlib.sha256(package.read_bytes()).hexdigest(), 'status': 'failed',
              'limitations': ['OTA and data migration unverified', 'Full visual and system behavior unverified',
                              'DEB/RPM installation unverified', 'Same-version reinstall is not upgrade acceptance']}
    output = ROOT / 'build/rewrite/package-smoke.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with tempfile.TemporaryDirectory(prefix='package-smoke-') as temp:
            work = Path(temp)
            if windows:
                install = work / 'installed'
                binary = install / (PRODUCT.lower() + '-validation.exe')
                for cycle in range(2):
                    subprocess.run([str(package), '/S', '/D=' + str(install)], check=True, timeout=240)
                    if not binary.is_file():
                        raise RuntimeError('installer-did-not-create-expected-executable')
                    report['cycle_' + str(cycle + 1)] = launch(binary, work, expected)
                uninstaller = install / 'uninstall.exe'
                subprocess.run([str(uninstaller), '/S'], check=True, timeout=90)
                deadline = time.monotonic() + 30
                while binary.exists() and time.monotonic() < deadline:
                    time.sleep(.5)
                if binary.exists():
                    raise RuntimeError('uninstaller-left-application-executable')
                report['installation'] = 'NSIS-silent-install-reinstall-uninstall'
            else:
                package.chmod(package.stat().st_mode | 0o100)
                for cycle in range(2):
                    report['cycle_' + str(cycle + 1)] = launch(package, work, expected)
                report['installation'] = 'AppImage-extract-and-run-on-Xvfb; package-manager-install-unverified'
        report['status'] = 'passed'
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
