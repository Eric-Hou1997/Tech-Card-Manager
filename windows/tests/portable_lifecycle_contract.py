#!/usr/bin/env python3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
tray = (ROOT / "tray_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")


checks = {
    "manager version 4.0.1": 'const appVersion = "4.0.1"' in main,
    "web card version 4.0.1": 'const WEB_CARD_VERSION = "4.0.1"' in card,
    "engine expects card 4.0.1": "$ExpectedWebCardVersion = '4.0.1'" in engine,
    "portable root follows current exe": all(
        value in platform
        for value in ["portableRootDir", "os.Executable()", "filepath.Dir(exe)"]
    ),
    "portable folders are explicit": all(
        value in platform + main
        for value in ['"runtime"', '"data"', '"logs"', '"backup"', '"updates"']
    ),
    "no localappdata manager home": 'filepath.Join(p, "IMDb Tech Manager")' not in platform,
    "runtime is not copied away from portable folder": (
        "upgradeInstalledRuntime()" not in main
        and "copySelf(installedExePath())" not in platform
    ),
    "portable folder write check": "ensurePortableWorkspace" in platform,
    "single manager instance": all(
        value in tray + main
        for value in ["platformAcquireManagerInstance", "CreateMutexW", "ERROR_ALREADY_EXISTS"]
    ),
    "second launch restores manager": "restoreManagerWindow" in tray and "primaryInstance" in main,
    "disable action available": all(
        value in main + platform + engine + web
        for value in ["disable-integration", "DisableIntegration", "恢复原生 Emby"]
    ),
    "disable removes only manager web artifacts": all(
        value in engine for value in ["Remove-WebPatch", "$LiveJs", "$DataFile", "$Worker"]
    ),
    "windows remains nfo read only": "Windows 只读索引媒体库 NFO" in web and "Save-Xml" not in engine,
    "native fallback hierarchy": all(
        value in card
        for value in [
            "createNativeMediaHost",
            "mediaSource",
            "emby-scroller",
            "detailMediaStreamsItemsContainer",
        ]
    ),
    "current detail root is required": "visible-detail-root-not-ready" in card and "getOrCreateNativeTarget(detailRoot)" in card,
    "no custom fallback surface": all(
        value not in card
        for value in ["techSpecsFallbackSurface", "techSpecsFallbackCard", "techSpecsFallbackHeading"]
    ),
    "field groups: native labels and stacked values": all(value in card for value in ["mediaInfoAttributeLabel", "mediaInfoAttributeValue secondaryText", "itm-tech-spec-value-line"]),
    "content-driven native card height": all(
        value in card for value in ["syncTechnicalCardHeight", "resetTechnicalCardHeight", 'setOwnedHeightStyle(card, card, "align-self", "flex-start")', "footer.scrollHeight"]
    ),
    "private manager catalog": all(
        value in engine + platform + main
        for value in ["manager-catalog.json", "managerCatalog", "/api/catalog"]
    ),
    "catalog covers tv hierarchy": all(
        value in engine for value in ["tvshow", "season", "episodedetails", "showTitle"]
    ),
    "movie tv manager spaces": all(
        value in web
        for value in ["catalogMovieTab", "catalogTvTab", "catalogSearch", "catalogSpecFilter", "renderTvTree"]
    ),
    "read only nfo preview": all(
        value in web for value in ["catalogPreview", "根级标签", "技术规格", "打开所在文件夹"]
    ),
    "targeted root scan": all(
        value in main + platform + engine + web
        for value in ["scan-root", "OnlyRoot", "data-scan-root", "检查此目录"]
    ),
}


failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows portable lifecycle contract failed: " + ", ".join(failed))
