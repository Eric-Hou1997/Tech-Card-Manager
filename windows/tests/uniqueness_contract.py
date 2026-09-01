#!/usr/bin/env python3
"""Windows legacy contract: uniqueness, stale-card elimination, search."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "does not force-kill other manager versions": "killLegacyManagerProcesses" not in platform + main and "terminated legacy manager processes" not in main,
    "stable web script URL": "$WebStampFile" not in engine and "&t=" not in engine and "Get-WebCardScriptTag" in engine,
    "background scan cannot patch web": "Ensure-WebPatch" not in engine and "Ensure-WebPatchIfAvailable" not in engine,
    "explicit version verification": all(value in engine for value in ["Assert-WebCardVersion", "WEB_CARD_VERSION\\s*=\\s*", "$ExpectedWebCardVersion"]),
    # Stale old-version cards can never render alongside the current one.
    "single-run version guard": all(value in card for value in ["__itmTechCardActiveVersion", "versionRank"]),
    "broad legacy card cleanup": '"[data-tech-spec-card], .itm-tech-card, .techSpecsFallbackShell"' in card,
    # Field groups (Codex doc §3/§4).
    "grouped native label once": 'label.className = "mediaInfoAttributeLabel"' in card and "for (const value of values)" in card,
    "values stacked vertically": "itm-tech-spec-value-line" in card and "values.join" not in card,
    # Search (Codex doc §2).
    "originaltitle indexed": "originalTitle = Direct-Text -Name 'originaltitle'" in engine,
    "originaltitle in catalog": "originalTitle = [string]$obj.originalTitle" in engine,
    "originaltitle searchable": "row.originalTitle" in web,
    "search debounce 200ms": "setTimeout(renderCatalog,200)" in web,
    # Versions.
    "manager version 4.0.0": 'const appVersion = "4.0.0"' in main,
    "card version 4.0.0": 'const WEB_CARD_VERSION = "4.0.0"' in card,
    "engine expects 4.0.0": "$ExpectedWebCardVersion = '4.0.0'" in engine,
    "web ui version 4.0.0": "4.0.0" in web,
    # Still suppressed: Season/Episode (product rule unchanged).
    "episode season still suppressed": 'CARD_SUPPRESSED_TYPES = new Set(["Episode", "Season"])' in card,
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows uniqueness contract failed: " + ", ".join(failed))
print("OK Windows legacy contract: uniqueness, stale-card elimination, grouped fields, search, versions")
