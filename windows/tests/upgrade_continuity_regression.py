#!/usr/bin/env python3
"""Regression checks for Portable upgrade settings and card readiness."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
service = (ROOT / "service_controller.go").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "portable settings remain authoritative with no external config mirror": (
        'manager-portable-settings.json' not in platform
        and "settingsContinuityMirror" not in platform
        and 'filepath.Join(baseDir(), "settings.json")' in main
    ),
    "a newly extracted version restores roots from the prior successful index": all(value in source for source, value in [
        (main, "platformEnsureSettingsContinuity"),
        (platform, "recoverSettingsFromIndexSummary"),
        (platform, '"上一版有效索引摘要"'),
    ]),
    "status returns the persisted configured-root state instead of zero values": all(value in platform for value in [
        "RootsConfigured: s.RootsConfigured",
        "ConfiguredRoots: append([]LibraryRoot(nil), s.LibraryRoots...)",
    ]),
    "recovered paths pass the normal root safety validation": (
        "sanitizeLibraryRoots(settings.LibraryRoots)" in platform
        and "validatedConfiguredSettings(settings)" in platform
    ),
    "card lease waits for current v7 data for the exact configured roots": all(value in source for source, value in [
        (main, "derivedIndexesValidForSettings"),
        (main, "data.Version != 7"),
        (main, "data.GeneratedAt != summary.GeneratedAt"),
        (service, "if !indexesReady"),
        (service, "正在恢复媒体目录并建立卡片索引"),
        (service, "索引文件未通过版本、目录与一致性复核"),
    ]),
    "missing roots cannot report a live service": (
        'c.state = serviceStopped' in service
        and '服务已停止；请先确认媒体目录' in service
        and 'c.publishLease(false)' in service
    ),
    "saving first-run roots starts the service and index automatically": (
        "if(lastStatus?.service?.running)await action('run');else await action('service-start')" in web
    ),
    "status distinguishes schema validity from current-root readiness": all(value in source for source, value in [
        (platform, 'st.Extra["web_data_current"]'),
        (platform, "dataValid && derivedIndexesValidForSettings(s)"),
        (web, "等待按当前目录重建"),
    ]),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy upgrade continuity regression failed: " + ", ".join(failed))
print("OK Windows legacy Portable upgrade continuity and card readiness contract")
