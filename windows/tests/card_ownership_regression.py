#!/usr/bin/env python3
"""Regression checks for the card fallback and generated-tag layout fixes."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

tag_function = web[web.index("function catalogTagHTML"):web.index("function renderCatalogPreview")]
render_function = card[card.index("async function render(requestId)"):card.index("function clearRetryTimer")]

checks = {
    "unreleased source version advances after the defective package": all(value in source for source, value in [
        (main, 'const appVersion = "4.1.0"'),
        (engine, "$ManagerVersion = '4.1.0'"),
        (web, "v4.1.0"),
    ]),
    "web card cache key advances everywhere": all(value in source for source, value in [
        (card, 'const WEB_CARD_VERSION = "4.1.0"'),
        (engine, "$ExpectedWebCardVersion = '4.1.0'"),
        (platform, 'const expectedWebCardVersion = "4.1.0"'),
    ]),
    "public card index carries path-free eligible type hints": all(value in engine for value in [
        "$itemTypes = [ordered]@{}", "$itemTypes[$imdb] = [string]$obj.type",
        "itemTypes = $itemTypes", "version = 7",
    ]),
    "manager health rejects stale public-index schemas": all(value in platform for value in [
        'Version   int                        `json:"version"`',
        'ItemTypes map[string]string          `json:"itemTypes"`',
        "data.Version == 7", "data.ItemTypes != nil",
    ]),
    "episode and season never enter public card index": (
        "([string]$obj.type) -eq 'Episode' -or ([string]$obj.type) -eq 'Season'" in engine
        and engine.index("continue", engine.index("([string]$obj.type) -eq 'Episode'"))
        < engine.index("$itemTypes[$imdb] = [string]$obj.type")
    ),
    "renderer recovers type from the verified public index": all(value in card for value in [
        "function getIndexedItemType(database, imdb)", "database.itemTypes[imdb]",
        "if (!itemType) itemType = indexedItemType", 'return "item-type-not-in-public-index"',
    ]),
    "route item id is optional when visible identity is sufficient": (
        "item = itemId ? await getCurrentItem(itemId) : null" in card
        and 'return "route-item-id-not-ready"' not in card
    ),
    "known episode is rejected before provider-link fallback": (
        render_function.index("if (itemType && CARD_SUPPRESSED_TYPES.has(itemType))")
        < render_function.index("getImdbFromDom(detailRoot)")
    ),
    "generated ownership color is independent of generator engine": (
        "tag.ownership==='generated'" in tag_function
        and "tag.engine" not in tag_function
        and "managerGenerated" in tag_function
        and "本程序生成（来自 NFO ownership 清单）" in tag_function
    ),
    "generated tag highlight is visibly pale yellow": all(value in web for value in [
        ".tagList .managerGenerated", "rgba(245,197,24,.14)", "rgba(245,197,24,.42)",
    ]),
    "generated tags begin on their own row": all(value in web for value in [
        "function renderTagList(tags)", "generatedTagRow", "flex:0 0 100%",
        "external.map(catalogTagHTML)", "generated.map(catalogTagHTML)",
    ]),
    "unknown ownership remains unstyled instead of guessed": (
        "structured&&tag.ownership==='generated'" in tag_function
        and "Never guess ownership from a tag's wording" in engine
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy card/ownership regression failed: " + ", ".join(failed))
print("OK Windows legacy card visibility and generated-tag ownership contract")
