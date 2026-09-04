#!/usr/bin/env python3
"""Regression contract for formal release metadata and settings update checks."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
build = (ROOT.parent / "tools" / "build-release.sh").read_text(encoding="utf-8")
update = (ROOT / "update.go").read_text(encoding="utf-8")

checks = {
    "formal version is synchronized": all(
        value in source for source, value in [
            (main, 'const appVersion = "4.1.0"'),
            (web, "v4.1.0"),
            (engine, "$ManagerVersion = '4.1.0'"),
            (engine, "$ExpectedWebCardVersion = '4.1.0'"),
            (platform, 'const expectedWebCardVersion = "4.1.0"'),
            (card, 'const WEB_CARD_VERSION = "4.1.0"'),
        ]
    ),
    "about metadata identifies the source version without claiming release": (
        "v4.1.0　Windows · x64 Portable" in web
        and "2026-09-02 发布" not in web
        and "正式发布时写入日期" not in web
    ),
    "every settings path starts an update check": (
        "function openSettingsModal()" in web
        and "checkCardUpdate()" in web
        and "$('#openSettings').onclick=openSettingsModal" in web
        and "openSettingsModal()" in web.split("setupOpenedThisSession=true", 1)[1]
    ),
    "update state remains in the about panel": all(
        value in web for value in [
            'id="cardUpdateState"',
            "正在检查 GitHub 正式发布…",
            "已是最新版本 ",
            "最新版本 ",
            "检查失败：",
        ]
    ),
    "about icons and download actions have resilient spacing": all(
        value in web for value in [
            ".aboutHead img{width:44px;height:32px}",
            ".aboutInstall>div:last-child{display:flex;flex-wrap:wrap;gap:12px;margin-top:12px}",
        ]
    ),
    "repeated settings opens cannot overlap checks": (
        "cardUpdateCheckInFlight" in web
        and "if(cardUpdateCheckInFlight)return" in web
        and "cardUpdateCheckInFlight=false" in web
    ),
    "update checks cache results and classify blocked requests": all(
        value in update for value in [
            '"update-state.json"',
            '"If-None-Match"',
            '"github-primary-rate-limit"',
            '"github-secondary-rate-limit"',
            '"proxy-forbidden"',
            '"github-forbidden"',
            "discoverCardReleaseViaPage",
        ]
    ),
    "download opens the exact verified package": (
        "cardPackageURL=result.package_url" in web
        and "const target=cardPackageURL||cardReleaseURL" in web
        and "下载指定安装包" in web
        and "?force=1" in web
    ),
    "release filenames use the short canonical scheme": all(
        value in build for value in [
            'ARTIFACT_BASE="TCM-v${VERSION}-Windows-x64-EXE"',
            'WIN_ZIP_NAME="${ARTIFACT_BASE}.zip"',
            'WIN_EXE_NAME="Tech-Card-Manager.exe"',
            'SHA_NAME="${ARTIFACT_BASE}-SHA256SUMS.txt"',
        ]
    ),
    "new language assets share the release checksum manifest": (
        "for language_asset in TCM-Language-*-r*.zip" in build
        and 'set -- "$@" "$language_asset"' in build
        and '--app-version "v$VERSION" --require-complete' in build
        and build.index("--changed-only --output") < build.index("shasum -a 256")
    ),
    "update check requires the exact Windows package": (
        'TCM-v%s.%s.%s-Windows-x64-EXE.zip' in update
        and "selectCardUpdateAsset" in update
        and "asset.Name == expectedArchive" in update
        and 'id="cardPackageName"' in web
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows release metadata/update contract failed: " + ", ".join(failed))
print("OK Tech Card Manager v4.1.0 release metadata and settings update contract")
