#!/usr/bin/env python3
"""Collect the reviewed v5 packages; never install, tag or publish them."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import plistlib

ROOT = Path(__file__).resolve().parents[2]
CATALOG = ROOT / 'rewrite/src-tauri/core/assets/release-packages.json'


def machine(path):
    data = path.read_bytes()
    if data[:4] == b'\x7fELF':
        return {62: 'x86_64', 183: 'aarch64'}[struct.unpack_from('<H', data, 18)[0]]
    if data[:2] == b'MZ':
        offset = struct.unpack_from('<I', data, 60)[0]
        if data[offset:offset + 4] != b'PE\0\0':
            raise ValueError('Invalid PE header')
        return {0x8664: 'x86_64', 0xAA64: 'aarch64'}[struct.unpack_from('<H', data, offset + 4)[0]]
    if data[:4] == b'\xcf\xfa\xed\xfe':
        return {0x1000007: 'x86_64', 0x100000C: 'aarch64'}[struct.unpack_from('<I', data, 4)[0]]
    raise ValueError(f'Unsupported executable: {path}')


def collect(target, target_dir, output):
    catalog = json.loads(CATALOG.read_text())
    version = catalog['version']
    config = json.loads((ROOT / 'rewrite/src-tauri/tauri.release.conf.json').read_text())
    assert config['identifier'] == 'io.github.eric-hou1997.tcm'
    assert os.environ.get('TCM_RELEASE_BUILD') == '1', 'Use the release build environment'
    entries = [p for p in catalog['packages'].values() if p['triple'] == target]
    assert len(entries) == (3 if 'linux' in target else 1), 'Unsupported release target'
    directory = target_dir / target / 'release'
    suffix = '.exe' if 'windows' in target else ''
    architecture = target.split('-')[0]
    binaries = [directory / f'Tech-Card-Manager{suffix}', ROOT / f'rewrite/src-tauri/binaries/tcm-maintenance-helper-{target}{suffix}']
    for binary in binaries:
        assert machine(binary) == architecture, f'Wrong application/helper architecture: {binary}'
    output.mkdir(parents=True, exist_ok=True)
    for entry in entries:
        extension = {'dmg': '.dmg', 'nsis': '.exe', 'appimage': '.AppImage', 'deb': '.deb', 'rpm': '.rpm'}[entry['kind']]
        candidates = list((directory / 'bundle' / entry['kind']).glob('*' + extension))
        assert len(candidates) == 1, f'Ambiguous/missing {entry["kind"]}: {candidates}'
        source = candidates[0]
        assert source.stat().st_size > 100000, f'Empty package: {source}'
        inspect_package(source.resolve(), entry, architecture)
        name = entry['pattern'].replace('{version}', version)
        destination = output / name
        assert not destination.exists(), f'Refusing to overwrite {destination}'
        shutil.copy2(source, destination)
        print(name)
    evidence = {'target': target, 'version': version, 'identifier': config['identifier'],
                'binaries': {p.name: {'architecture': machine(p), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in binaries},
                'package_checks': ['extracted main/helper architecture', 'packaged LICENSE/NOTICE byte parity'],
                'runtime_acceptance': 'not performed', 'macos_signing': 'ad hoc; not notarized' if 'apple' in target else 'not applicable'}
    (output / f'TCM-v{version}-{target}-build.json').write_text(json.dumps(evidence, indent=2) + '\n')


def inspect_package(source, entry, architecture):
    """Unpack without installing or starting the product or Emby service."""
    with tempfile.TemporaryDirectory(prefix='tcm-package-') as temporary:
        unpacked = Path(temporary)
        kind = entry['kind']
        mounted = False
        def run(*args):
            subprocess.run(args, cwd=unpacked, check=True, stdout=subprocess.DEVNULL)
        try:
            if kind == 'dmg':
                run('hdiutil', 'attach', str(source), '-readonly', '-nobrowse', '-mountpoint', str(unpacked))
                mounted = True
                app = unpacked / 'Tech Card Manager.app'
                run('codesign', '--verify', '--deep', '--strict', str(app))
                info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
                assert info['CFBundleIdentifier'] == 'io.github.eric-hou1997.tcm'
                assert info['CFBundleShortVersionString'] == json.loads(CATALOG.read_text())['version']
            elif kind == 'deb':
                run('dpkg-deb', '--extract', str(source), str(unpacked))
                actual = subprocess.check_output(['dpkg-deb', '--field', str(source), 'Architecture'], text=True).strip()
                assert actual == {'x86_64': 'amd64', 'aarch64': 'arm64'}[architecture]
            elif kind == 'rpm':
                run('bsdtar', '-xf', str(source))
                actual = subprocess.check_output(['rpm', '-qp', '--queryformat', '%{ARCH}', str(source)], text=True).strip()
                assert actual == architecture
            elif kind == 'appimage':
                run(str(source), '--appimage-extract')
            else:
                sevenzip = shutil.which('7z') or str(Path(os.environ.get('ProgramFiles', 'C:/Program Files')) / '7-Zip/7z.exe')
                run(sevenzip, 'x', str(source), '-y', f'-o{unpacked}')
            suffix = '.exe' if kind == 'nsis' else ''
            for name in [f'Tech-Card-Manager{suffix}', f'tcm-maintenance-helper{suffix}']:
                files = [p for p in unpacked.rglob(name) if p.is_file() and not p.is_symlink()]
                assert len(files) == 1, f'{kind} does not contain exactly one {name}: {files}'
                assert machine(files[0]) == architecture, f'{kind}: wrong packaged {name} architecture'
            for name in ['LICENSE', 'NOTICE']:
                expected = (ROOT / name).read_bytes()
                assert any(p.is_file() and p.read_bytes() == expected for p in unpacked.rglob(name)), f'{kind} missing {name}'
        finally:
            if mounted:
                subprocess.run(['hdiutil', 'detach', str(unpacked)], check=True, stdout=subprocess.DEVNULL)


def verify(output):
    catalog = json.loads(CATALOG.read_text())
    expected = {p['pattern'].replace('{version}', catalog['version']) for p in catalog['packages'].values()}
    actual = {p.name for p in output.iterdir() if p.suffix.lower() in ('.dmg', '.exe', '.appimage', '.deb', '.rpm')}
    assert actual == expected, f'Package inventory differs: missing={expected-actual}, extra={actual-expected}'
    languages = json.loads((ROOT / 'rewrite/src-tauri/core/assets/language_catalog.json').read_text())
    for descriptor in languages['languages'].values():
        path = output / descriptor['asset']
        assert hashlib.sha256(path.read_bytes()).hexdigest() == descriptor['sha256'], f'Language digest differs: {path}'
    files = sorted(p for p in output.iterdir() if p.is_file() and p.name != 'TCM-v5.0.0-SHA256SUMS.txt')
    (output / 'TCM-v5.0.0-SHA256SUMS.txt').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in files))
    print('Verified nine package files and five exact language assets; no runtime acceptance implied.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--target')
    parser.add_argument('--target-dir', type=Path)
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    try:
        if args.verify:
            verify(args.output)
        else:
            collect(args.target, args.target_dir, args.output)
    except (AssertionError, OSError, KeyError, ValueError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
