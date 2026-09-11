#!/usr/bin/env python3
"""Install a validation DMG into a fresh temporary directory, verify IPC and remove it.

This is a developer smoke check, not Gatekeeper, updater or production uninstall
acceptance. It never launches or replaces a production application.
"""
import argparse
import datetime
import hashlib
import json
import os
import pathlib
import platform
import plistlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
PRODUCT = "ITM" if (ROOT / "macos/engine/mac-engine.py").exists() else "TCM"
TARGET = "aarch64-apple-darwin"


def run(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manual-quit", action="store_true", help="Wait for a real native Quit action instead of automatic IPC exit")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Requires a native ARM64 macOS desktop session")
    binary_name = PRODUCT.lower() + "-validation"
    prior = subprocess.run(["pgrep", "-x", binary_name], capture_output=True, text=True)
    if prior.returncode != 1:
        raise SystemExit("Close existing validation instances before running this check")
    bundles = list((ROOT / "rewrite/src-tauri/target" / TARGET / "release/bundle/dmg").glob("*.dmg"))
    if len(bundles) != 1:
        raise SystemExit("Expected exactly one validation DMG")
    dmg = bundles[0]
    expected_id = "io.github.eric-hou1997." + PRODUCT.lower() + ".validation"
    with tempfile.TemporaryDirectory(prefix=PRODUCT.lower() + "-native-smoke-") as temp:
        work = pathlib.Path(temp)
        volume = work / "volume"
        volume.mkdir()
        run("hdiutil", "attach", str(dmg), "-readonly", "-nobrowse", "-mountpoint", str(volume))
        try:
            source_app = volume / (PRODUCT + " Validation.app")
            destination = work / "install" / source_app.name
            destination.parent.mkdir()
            run("ditto", str(source_app), str(destination))
        finally:
            run("hdiutil", "detach", str(volume))
        info = plistlib.loads((destination / "Contents/Info.plist").read_bytes())
        if info["CFBundleIdentifier"] != expected_id:
            raise RuntimeError("Unexpected bundle identifier")
        binary = destination / "Contents/MacOS" / info["CFBundleExecutable"]
        architecture = run("lipo", "-archs", str(binary)).strip()
        if architecture != "arm64":
            raise RuntimeError("Application is not exclusively ARM64")
        for name in ("LICENSE", "NOTICE"):
            if digest(destination / "Contents/Resources" / name) != digest(ROOT / name):
                raise RuntimeError("Redistribution resource mismatch: " + name)
        report_path = work / "events.jsonl"
        env = os.environ.copy()
        env["REWRITE_PROBE_REPORT"] = str(report_path)
        if args.manual_quit:
            env.pop("REWRITE_PROBE_AUTOCLOSE", None)
        else:
            env["REWRITE_PROBE_AUTOCLOSE"] = "1"
        child = subprocess.Popen([str(binary)], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        if args.manual_quit:
            print("AWAITING NATIVE QUIT: " + str(destination), flush=True)
        try:
            output, _ = child.communicate(timeout=120 if args.manual_quit else 30)
        except subprocess.TimeoutExpired:
            child.kill()
            child.communicate()
            raise RuntimeError("Native IPC/exit smoke timed out")
        if child.returncode != 0:
            raise RuntimeError("Validation exited with error: " + output)
        events = [json.loads(line) for line in report_path.read_text().splitlines()]
        if [e["event"] for e in events] != ["native-setup-complete", "frontend-mounted-ipc-roundtrip", "process-exit"]:
            raise RuntimeError("Incomplete native lifecycle evidence")
        if any(e["product"] != PRODUCT or e["arch"] != "aarch64" for e in events):
            raise RuntimeError("Runtime identity mismatch")
        remaining = subprocess.run(["pgrep", "-x", binary_name], capture_output=True, text=True)
        if remaining.returncode != 1:
            raise RuntimeError("Validation process remains after exit")
        result = {
            "product": PRODUCT,
            "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "host": {"macos": platform.mac_ver()[0], "arch": platform.machine()},
            "artifact": {"path": str(dmg.relative_to(ROOT)), "sha256": digest(dmg), "bytes": dmg.stat().st_size},
            "source_sha256": {name: digest(ROOT / name) for name in sorted(set((
                "rewrite/src-tauri/src/main.rs", "rewrite/src/App.vue", "rewrite/src-tauri/Cargo.lock",
                "rewrite/package-lock.json", "rewrite/src-tauri/tauri.conf.json",
                "rewrite/src-tauri/icons/icon.png", "rewrite/src-tauri/icons/icon.icns", "rewrite/src-tauri/icons/icon.ico",
                "rewrite/src/LibraryPanel.vue", "rewrite/src/contracts.ts", "rewrite/src-tauri/Cargo.toml",
                "rewrite/src-tauri/core/Cargo.toml", "rewrite/src-tauri/src/desktop.rs", "rewrite/src-tauri/src/credentials.rs"
            )) | {str(p.relative_to(ROOT)) for p in (ROOT / "rewrite/src-tauri/core/src").rglob("*.rs")} | {str(p.relative_to(ROOT)) for p in (ROOT / "rewrite/src-tauri/core/assets").glob("*")})},
            "app_architecture": architecture,
            "license_notice": "byte-identical-to-repository",
            "install": "copied-from-readonly-DMG-to-isolated-directory",
            "events": events,
            "exit_code": child.returncode,
            "exit_trigger": "external-native-action-no-autoclose" if args.manual_quit else "automatic-IPC-smoke",
            "worker_shutdown": "process-exit event occurs after synchronous Desktop.shutdown",
            "remaining_app_processes": 0,
            "limitations": ["No Gatekeeper/notarization acceptance", "No update or data migration exercised", "Visual appearance requires separate native UI inspection", "Removal of isolated app only; not production uninstaller validation"]
        }
    if work.exists():
        raise RuntimeError("Temporary installation cleanup failed")
    result["isolated_install_removal"] = "passed"
    output_path = ROOT / "build/rewrite" / ("quit-smoke.json" if args.manual_quit else "native-smoke.json")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
