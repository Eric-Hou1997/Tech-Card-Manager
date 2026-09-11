#!/usr/bin/env python3
"""CI-container package-manager acceptance. Never run on a user's Linux installation."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from package_smoke import PRODUCT, ROOT, launch


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--kind', choices=['deb', 'rpm'], required=True)
    p.add_argument('--arch', choices=['x86_64', 'aarch64'], required=True)
    p.add_argument('--probe', action='store_true')
    args = p.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or not Path('/.dockerenv').exists():
        raise SystemExit('Requires a disposable GitHub Actions container')
    result = Path('/tmp/package-acceptance/result.json')
    binary = Path('/usr/bin') / (PRODUCT.lower() + '-validation')
    if args.probe:
        if os.geteuid() == 0:
            raise SystemExit('The application probe must run as an ordinary user')
        with tempfile.TemporaryDirectory() as temp:
            evidence = launch(binary, Path(temp), args.arch)
        with result.open('a', encoding='utf-8') as output:
            output.write(json.dumps(evidence) + '\n')
        return
    if os.geteuid() != 0:
        raise SystemExit('Package manager setup requires container root')
    packages = list((ROOT / 'build/rewrite/package-input').rglob('*.' + args.kind))
    if len(packages) != 1:
        raise RuntimeError('expected-one-input-package')
    package = packages[0]
    if args.kind == 'deb':
        name = subprocess.check_output(['dpkg-deb', '-f', str(package), 'Package'], text=True).strip()
        install = ['apt-get', 'install', '-y', '--reinstall', str(package)]
        remove = ['apt-get', 'remove', '-y', name]
    else:
        name = subprocess.check_output(['rpm', '-qp', '--queryformat', '%{NAME}', str(package)], text=True).strip()
        install = ['dnf', 'install', '-y', '--nogpgcheck', str(package)]
        remove = ['dnf', 'remove', '-y', name]
    subprocess.run(['useradd', '-m', 'package-tester'], check=True)
    subprocess.run(['install', '-d', '-o', 'package-tester', '-g', 'package-tester', str(result.parent)], check=True)
    report = {'product': PRODUCT, 'kind': args.kind, 'arch': args.arch, 'package': package.name,
              'sha256': hashlib.sha256(package.read_bytes()).hexdigest(),
              'distribution': Path('/etc/os-release').read_text(), 'source_run': os.environ['SOURCE_RUN'],
              'status': 'failed', 'limitations': ['Container userland with host kernel', 'Xvfb desktop only',
              'Same-version reinstall is not OTA or version-upgrade acceptance', 'Full desktop UX and data migration unverified']}
    try:
        for cycle in range(2):
            command = install
            if args.kind == 'rpm' and cycle == 1:
                command = ['dnf', 'reinstall', '-y', '--nogpgcheck', str(package)]
            subprocess.run(command, check=True, timeout=240)
            if not binary.is_file():
                raise RuntimeError('package-missing-expected-executable')
            listing = ['dpkg-query', '-L', name] if args.kind == 'deb' else ['rpm', '-ql', name]
            installed = subprocess.check_output(listing, text=True).splitlines()
            for resource in ['LICENSE', 'NOTICE']:
                expected = (ROOT / resource).read_bytes()
                candidates = [Path(item) for item in installed if Path(item).name == resource]
                if not any(item.is_file() and item.read_bytes() == expected for item in candidates):
                    raise RuntimeError('installed-resource-mismatch: ' + resource)
            subprocess.run(['runuser', '-u', 'package-tester', '--', 'dbus-run-session', '--',
                            'xvfb-run', '-a', 'python3', str(Path(__file__).resolve()),
                            '--kind', args.kind, '--arch', args.arch, '--probe'], check=True, timeout=90)
        subprocess.run(remove, check=True, timeout=120)
        if binary.exists():
            raise RuntimeError('uninstaller-left-application')
        if args.kind == 'deb':
            state = subprocess.run(['dpkg-query', '-W', '-f=${db:Status-Status}', name], capture_output=True, text=True)
            if state.returncode == 0 and state.stdout.strip() == 'installed':
                raise RuntimeError('dpkg-still-reports-installed')
        elif subprocess.run(['rpm', '-q', name], capture_output=True).returncode == 0:
            raise RuntimeError('rpm-still-reports-installed')
        report['cycles'] = [json.loads(line) for line in result.read_text().splitlines()]
        report['status'] = 'passed'
        report['uninstall'] = 'executable-removed-and-package-database-checked'
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        output = ROOT / 'build/rewrite/linux-package-acceptance.json'
        output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
