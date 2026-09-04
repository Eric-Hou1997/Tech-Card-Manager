#!/usr/bin/env python3
"""Windows legacy responsive card layout and scroll-stability contract."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

height_start = card.index("function syncTechnicalCardHeight(card)")
height_end = card.index("function scheduleTechnicalCardHeightSync", height_start)
height_sync = card[height_start:height_end]

checks = {
    "legacy and Web Card 4.0.4 are synchronized": all(
        value in source for source, value in [
            (main, 'const appVersion = "4.0.4"'),
            (engine, "$ManagerVersion = '4.0.4'"),
            (web, "v4.0.4"),
            (card, 'const WEB_CARD_VERSION = "4.0.4"'),
            (engine, "$ExpectedWebCardVersion = '4.0.4'"),
            (platform, 'const expectedWebCardVersion = "4.0.4"'),
        ]
    ),
    "card bottom reserves one measured text line": all(value in card for value in [
        "function technicalCardLineHeight(card)",
        "function technicalCardBottomClearance(card, footer)",
        "Math.max(nativePadding, technicalCardLineHeight(card))",
        "meaningfulBottom +",
        "technicalCardBottomClearance(card, footer)",
        "trailingSpace >= bottomClearance - 2",
    ]),
    "MKV-like native card never falls below its pre-insertion Emby card": all(
        value in card for value in [
            "captureNativeCardBaseline(template.card)",
            "cardLayoutBaselines.set(card, nativeBaseline)",
            "refreshNativeCardBaseline(card)",
            "Math.max(requiredFooterHeight, baseline.footer || 0)",
            "requiredNodeHeight(node, desiredBottom, baselineHeight)",
        ]
    ),
    "ISO BDMV and Series remain content-height cards": (
        'layoutMode === "native-card"' in card
        and 'data-tech-spec-render-mode") !== "native-card"' in card
        and "baseline.card" in card
    ),
    "labels retain native Emby flex spacing": all(value in card for value in [
        ".mediaStreamAttribute{display:flex!important",
        ".mediaInfoAttributeLabel{flex:0 0 auto!important",
        ".mediaInfoAttributeValue{display:block!important;flex:1 1 0!important",
    ]) and "column-gap:.75em!important" not in card,
    "responsive width uses live native geometry without stale session cache": all(
        value in card for value in [
            "function findLiveNativeReferenceCard(card)",
            "function resolvedStandardCardWidth(card)",
            'if (mode === "native-card")',
            'listen(window, "resize", () => {',
            ".forEach(scheduleTechnicalCardHeightSync)",
        ]
    ) and 'sessionStorage.setItem("itm-standard-card-width"' not in card,
    "height updates do not reset the card before every measurement": (
        "resetTechnicalCardHeight(card)" not in height_sync
        and "node.style.getPropertyValue(property) === value" in card
        and "cardLayoutSyncPending" in card
        and "cardLayoutSignatures" in card
    ),
    "Manager DOM churn is excluded from full-page render observation": all(
        value in card for value in [
            "function managerOwnsMutationNode(node)",
            "function mutationTouchesRenderSurface(record)",
            "changed.every(managerOwnsMutationNode)",
            "records.some(mutationTouchesRenderSurface)",
        ]
    ),
    "native cards are guarded against indirect width changes": all(
        value in card for value in [
            "captureNativeSiblingGeometry(",
            "native.container",
            "protectNativeSiblingGeometry(",
            "nativeSiblingGeometryChanged(snapshot)",
            "native-sibling-geometry-guard",
            "nativeSiblingGeometryPreserved",
        ]
    ),
    "service state copy distinguishes started closed and transitions": all(
        value in web for value in [
            "Emby 卡片显示服务已启动",
            "Emby 卡片显示服务已关闭",
            "Emby 卡片显示服务正在启动",
            "Emby 卡片显示服务正在关闭",
        ]
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy layout stability regression failed: " + ", ".join(failed))
print("OK Windows legacy responsive layout preserves Emby cards and scroll position")
