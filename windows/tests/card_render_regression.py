#!/usr/bin/env python3
"""Regression contract for the legacy card blackout and catalog layout bugs."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "web card cache key is bumped across all components": all(
        value in source
        for source, value in [
            (card, 'const WEB_CARD_VERSION = "4.0.4"'),
            (engine, "$ExpectedWebCardVersion = '4.0.4'"),
            (platform, 'const expectedWebCardVersion = "4.0.4"'),
        ]
    ),
    "real Emby detail roots are accepted": all(
        value in card for value in ['".itemView.view-item-item:not(.hide)"', '".itemDetailPage:not(.hide)"', '".detailPage:not(.hide)"']
    ),
    "visible page type can recover from delayed ApiClient": (
        "function getVisibleItemType(root)" in card
        and "normalizeItemType(item && item.Type) || getVisibleItemType(detailRoot)" in card
        and card.index("const detailRoot = findVisibleDetailRoot()") < card.index("item = itemId ? await getCurrentItem(itemId) : null")
    ),
    "episode and season remain suppressed after fallback": (
        'const CARD_SUPPRESSED_TYPES = new Set(["Episode", "Season"])' in card
        and "CARD_SUPPRESSED_TYPES.has(itemType)" in card
    ),
    "lease uses server time and really polls": all(
        value in card for value in [
            'response.headers.get("Date")',
            "cachedRuntimeValidUntil",
            "leaseInterval = setInterval",
            "enforceRuntimeLease(\"lease-poll\")",
            "clearInterval(leaseInterval)",
        ]
    ),
    "desktop catalog height follows preview": all(
        value in web for value in [
            "--catalog-list-height",
            "function syncCatalogListHeight()",
            "preview.scrollHeight",
            "requestAnimationFrame(syncCatalogListHeight)",
        ]
    ) and "max-height:680px" not in web,
    "tv groups start closed and survive refresh": all(
        value in web for value in [
            "expandedShows:new Set()",
            "expandedSeasons:new Set()",
            "data-tv-show=",
            "data-tv-season=",
            "document.addEventListener('toggle'",
        ]
    ) and '<details class="catalogTree" open>' not in web,
    "manager does not claim a remote browser rendered": "卡片服务已就绪" in web,
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy card render regression failed: " + ", ".join(failed))
print("OK Windows legacy card rendering and catalog layout regression contract")
