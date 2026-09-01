#!/usr/bin/env python3
"""legacy Windows contract: catalog generation, friendly XML errors,
card single-line fields, card-only height, ISO fallback shell, custom host."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "manager version 4.0.1": 'const appVersion = "4.0.1"' in main,
    "web ui version 4.0.1": '4.0.1' in web,
    "web card version 4.0.1": 'const WEB_CARD_VERSION = "4.0.1"' in card,
    "engine expects card 4.0.1": "$ExpectedWebCardVersion = '4.0.1'" in engine,
    # Bug 1: catalog is actually generated now (search had no results because
    # nothing ever wrote manager-catalog.json in the earlier implementation).
    "catalog rows are built from the path-keyed cache": all(
        value in engine for value in ["$catalogRows", "rowPath = [string]$entry.Name", "path = $rowPath", "libraryKind = $rowKind", "specs = $obj.specs", "tags = @($obj.tags)"]
    ),
    "catalog file persisted": "-Path $CatalogFile -Depth 20" in engine,
    "catalog includes episodes for the manager": "The Manager catalog lists every parsed NFO" in engine,
    "catalog rows carry nfo path for search": "row.path" in web,
    # Bug 6: raw PowerShell Load() exceptions flooded the manager UI.
    "friendly xml load errors": "NFO XML 解析失败，已跳过" in engine,
    "inner exception surfaced": "$_.Exception.InnerException" in engine,
    # Bug 2 (2.17): field groups - label once, values stacked vertically.
    "native field group renderer": all(v in card for v in ["mediaStreamInnerCardFooter-cardText", "mediaStreamAttribute", "mediaInfoAttributeLabel", "mediaInfoAttributeValue secondaryText"]),
    "long values wrap instead of truncating": all(
        value in card for value in [
            "white-space:normal!important",
            "overflow-wrap:anywhere!important",
            "min-width:0!important",
            "text-overflow:clip!important",
            "ensureCardStyles",
        ]
    ),
    "label layout belongs to Emby theme": "flex:0 0 80px" not in card,
    # Bug 3: height sync must not stretch the shared scroller row.
    "height is fully content driven": all(
        value in card for value in ["align-self:flex-start", 'setOwnedHeightStyle(card, card, "align-self", "flex-start")']
    ),
    "host height styles are restored exactly": all(value in card for value in [
        "cardHeightStyleSnapshots", "previous.priority", "cardHeightStyleSnapshots.delete(card)"
    ]),
    # The card shell and typography are Emby-native, not a self-drawn BEM card.
    "card clones native Emby shell": all(value in card for value in [
        "template.card.cloneNode(true)", "card backdropCard card-horiz backdropCard-horiz",
        "cardPadder-backdrop mediaStreamPadder", "removeInteractiveCardState"
    ]),
    "no self-drawn card chrome": all(value not in card for value in [
        "background:rgba(20,25,32,.95)", "border-radius:14px"
    ]),
    "native width is measured not widened": all(
        value in card for value in ["measureNativeCardWidth", "width > 120 && width < 900"]
    ),
    "responsive wide native-card layout": all(value in card for value in [
        "itm-tech-card--wide{width:fit-content",
        "--itm-standard-card-width",
        "--itm-wide-card-max-width",
        "standardWidth * 2",
        '"wide-card"',
    ]),
    "dual layout modes": '"native-card"' in card and '"wide-card"' in card,
    "captured native media hierarchy": all(value in card for value in [
        "verticalSection verticalSection-cards audioVideoMediaInfo",
        'mediaScroller.className = "emby-scrollbuttons-scroller"',
        "itemsContainer-defaultCardSize scrollSlider itemsContainer"
    ]),
    "generated host cannot impersonate a playable media source": 'className = "mediaSource emby-' not in card,
    "series avoids hidden native media subtree": "before-hidden-native-media-section" in card and "after-media-sources" not in card,
    # Product rule preserved: episodes/seasons stay suppressed on the web.
    "episode season still suppressed": 'CARD_SUPPRESSED_TYPES = new Set(["Episode", "Season"])' in card,
    "episodes still excluded from web index": "episodeSpecsExcludedFromWeb" in engine,
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows catalog contract failed: " + ", ".join(failed))
print("OK Windows legacy contract: catalog/search, friendly XML errors, card layout, ISO shell, custom host")
