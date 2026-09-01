#!/usr/bin/env python3
"""Windows legacy safety and read-only catalog contract."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
tray = (ROOT / "tray_windows.go").read_text(encoding="utf-8")

index_branch = engine.split("if ($IndexOnly)", 1)[1].split("if ($RebuildIndexOnly)", 1)[0]
update_body = engine.split("function Update-TechIndex", 1)[1].split("function Show-IndexStatus", 1)[0]
public_write = engine.split("Keep it limited to the card lookup", 1)[1].split("return $output", 1)[0]

checks = {
    "version triplet": all(x for x in [
        'const appVersion = "4.0.1"' in main,
        "$ManagerVersion = '4.0.1'" in engine,
        'const WEB_CARD_VERSION = "4.0.1"' in card,
        "$ExpectedWebCardVersion = '4.0.1'" in engine,
    ]),
    "startup is web-read-only": 'platformRunEngine("repair-web"' not in main,
    "index branch cannot patch web": all(x not in index_branch for x in [
        "Install-WebPatch", "Remove-WebPatch", "Sync-ManagerFrontendAsset", "IndexHtml"
    ]),
    "index function cannot patch web": all(x not in update_body for x in [
        "Install-WebPatch", "Remove-WebPatch", "Ensure-WebPatch", "IndexHtml"
    ]),
    "rebuild is index-only": "-RebuildIndexOnly" in platform and "-ManagedInstall" not in platform.split('case "rebuild-index":', 1)[1].split('case "discover-roots":', 1)[0],
    "rebuild preserves last valid output": "Update-TechIndex -ForceReparse" in engine and "FULL_REBUILD_OFFLINE_ROOT" in engine,
    "repair requires elevation": all(x in main + platform for x in [
        'case "install", "repair-web", "disable-integration", "migrate-legacy"',
        "startPrivilegedJob(req.Action",
        "platformRunElevatedAction",
        "ShellExecuteExW",
    ]),
    "external per-instance backup": all(x in engine for x in [
        "WEB_PATCH_BACKUP_INSIDE_EMBY", "index.original.", "InstanceId", "baselineSha256"
    ]),
    "cross-process patch lock": all(x in engine for x in [
        "Global\\IMDbTechManager.EmbyWebPatch.", "web-patch.lock", "FileShare]::None", "AbandonedMutexException"
    ]),
    "durable journal phases": all(x in engine for x in [
        "PREPARING", "CANDIDATE_VERIFIED", "REPLACED", "COMMITTED", "Recover-WebPatchTransaction"
    ]),
    "committed recovery validates operation": "journal.expectPatch" in engine and "已提交候选与日志声明的 Patch 状态不一致" in engine,
    "hash CAS and postverify": all(x in engine for x in [
        "CONCURRENT_MODIFICATION", "WEB_PATCH_POST_VERIFY_FAILED", "candidateHash", "sourceHash"
    ]),
    "same-directory atomic replacement": "[System.IO.File]::Replace($tmp, $IndexHtml, $rollback, $false)" in engine,
    "unique transaction names": ".index.html.techspec.' + [guid]::NewGuid()" in engine and ".techspec.tmp" not in engine,
    "missing index fails closed": "EMBY_INDEX_MISSING" in engine and "NO_TRUSTED_BASELINE" in engine,
    "stable managed script block": all(x in engine for x in [
        "IMDbTechManager WebPatch BEGIN", "IMDbTechManager WebPatch END", "Stable version-only URL"
    ]),
    "no launch force-kill or cache stamp": all(x not in main + platform + engine for x in [
        "killLegacyManagerProcesses", "writeWebCacheStamp", "$WebStampFile"
    ]),
    "parser cache migration": "$ParserCacheVersion + ':'" in engine,
    "season nfo is indexed": "if ($file.Name -ieq 'season.nfo')" not in update_body,
    "catalog not gated by imdb": "$catalogRows.Add" in update_body and update_body.index("$catalogRows.Add") < update_body.index("if (-not $obj.imdb -or -not $obj.specs)"),
    "catalog api maps items": all(x in main for x in [
        'Items       []json.RawMessage `json:"items"`', '"managerCatalog": catalog.Items'
    ]),
    "catalog reloads after background completion": "loadCatalog(true)" in web and "catalogLoadedAt" in web,
    "movie tv state is independent": "catalogViewState={movies:" in web and "switchCatalogSpace" in web,
    "tv tree defaults closed and preserves user state": all(x in web for x in [
        "expandedShows:new Set()", "expandedSeasons:new Set()", "data-tv-show=", "data-tv-season=",
    ]) and '<details class="catalogTree" open>' not in web and '<details class="catalogSeason" open>' not in web,
    "season inherits series grouping": "seriesTitle = $seriesTitle" in engine and "row.seriesTitle" in web,
    "public card data has no local paths": all(x not in public_write for x in [
        "libraryRoots", "libraries =", "scanStats", "CatalogFile"
    ]),
    "private index summary": "-Path $IndexSummaryFile -Depth 24" in engine and "indexSummaryFile()" in platform,
    "wide card is bounded between one and two native cards": all(x in card for x in [
        "itm-tech-card--wide{width:fit-content",
        "min-width:min(var(--itm-standard-card-width)",
        "max-width:min(var(--itm-wide-card-max-width)",
        "standardWidth * 2",
    ]),
    "dom lookup is visible-root scoped": all(x in card for x in [
        "findNativeVideoCard(detailRoot)", "getOrCreateNativeTarget(detailRoot)", "root.querySelectorAll(\".mediaSources\")"
    ]),
    "route identity fails closed": all(x in card for x in [
        "item-type-not-in-public-index", "visible-detail-root-not-ready", "route-index-type-mismatch"
    ]),
    "old renderer teardown": all(x in card for x in [
        "__itmTechCardTeardown", "observer.disconnect()", "clearInterval(watchdogInterval)", "clearInterval(leaseInterval)"
    ]),
    "cache persisted before freshness stamps": engine.index("Save-JsonAtomic -Object $cache -Path $CacheFile") < engine.index("Save-JsonAtomic -Object $state -Path $StateFile"),
    "restore postverify rejects leftover patch": "恢复结果仍含受管 Web Patch" in engine,
    "manager state uses unique durable replace": "os.CreateTemp" in main and "platformAtomicReplace(tmp, path)" in main,
    "embedded engine assets update atomically": "atomicWrite(path, b, 0755)" in main and "atomicWrite(jsPath, js, 0644)" in main,
    "tray callback does not roundtrip go pointer": "enumeratedManagerWindow atomic.Uintptr" in tray and "unsafe.Pointer(lParam)" not in tray,
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows read-only patch contract failed: " + ", ".join(failed))
print("OK Windows v4.0.1: transactional Web Patch, read-only NFO catalog, scoped Web Card")
