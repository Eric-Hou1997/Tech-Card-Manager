#!/usr/bin/env python3
"""Check exact required package count per target; never claim install/runtime acceptance."""
import argparse, json, pathlib
root=pathlib.Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--target',required=True);a=p.parse_args()
scope=json.loads((root/'docs/rewrite/scope.json').read_text())
selected=next(t for t in scope['targets'] if t['rust_target']==a.target)
base=root/'rewrite/src-tauri/target'/a.target/'release/bundle'
patterns={'dmg':'dmg/*.dmg','nsis':'nsis/*-setup.exe','appimage':'appimage/*.AppImage','deb':'deb/*.deb','rpm':'rpm/*.rpm'}
for kind in selected['bundles']:
    files=list(base.glob(patterns[kind]))
    if len(files)!=1 or files[0].stat().st_size==0:raise SystemExit(f'FAIL {kind}: expected exactly one nonempty artifact, got {files}')
    print(f'package-present: {files[0].name}')
print('Package presence only. Installation, UI, architecture, update and uninstall acceptance remain separate.')
