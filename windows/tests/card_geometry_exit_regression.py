#!/usr/bin/env python3
"""Windows legacy scoped card geometry and foreground-exit regression contract."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
tray = (ROOT / "tray_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

height_start = card.index("function syncTechnicalCardHeight(card)")
height_end = card.index("function scheduleTechnicalCardHeightSync", height_start)
height_sync = card[height_start:height_end]
confirm_start = platform.index("func platformConfirmExit")
confirm_end = platform.index("\nfunc openPath", confirm_start)
confirm_exit = platform[confirm_start:confirm_end]

checks = {
    "v4.0.1 source versions are synchronized": all(value in source for source, value in [
        (main, 'const appVersion = "4.0.1"'),
        (engine, "$ManagerVersion = '4.0.1'"),
        (web, "v4.0.1"),
        (card, 'const WEB_CARD_VERSION = "4.0.1"'),
        (engine, "$ExpectedWebCardVersion = '4.0.1'"),
        (platform, 'const expectedWebCardVersion = "4.0.1"'),
    ]),
    "geometry reset is scoped to Manager cards": all(value in card for value in [
        "[data-tech-spec-card='1'] .cardContent",
        "aspect-ratio:auto!important",
        "box-sizing:border-box!important",
    ]),
    "normal MKV cards retain one native width": all(value in card for value in [
        ".itm-tech-card--native{width:var(--itm-standard-card-width)!important",
        "min-width:var(--itm-standard-card-width)!important",
        "max-width:var(--itm-standard-card-width)!important",
    ]),
    "ISO BDMV and Series retain bounded one-to-two-card width": all(value in card for value in [
        ".itm-tech-card--wide{width:fit-content!important",
        "--itm-wide-card-max-width",
        "standardWidth * 2",
        "Math.max(minWidth, preferredWidth || minWidth)",
    ]),
    "inner native surface cannot outgrow or lose rounded corners": all(value in card for value in [
        ".cardBox{max-width:100%!important",
        ".cardContent{width:100%!important;max-width:100%!important",
        ".cardContent{",
        "overflow:hidden!important",
        ".mediaStreamInnerCardFooter{width:100%!important;max-width:100%!important",
    ]),
    "field values use native flex spacing and wrap inside the card": all(value in card for value in [
        ".mediaStreamAttribute{display:flex!important",
        ".mediaInfoAttributeLabel{flex:0 0 auto!important",
        ".mediaInfoAttributeValue{display:block!important;flex:1 1 0!important",
        "overflow-wrap:anywhere!important",
    ]) and "column-gap:.75em!important" not in card,
    "height follows meaningful rows rather than native aspect ratio": all(value in card for value in [
        "function technicalCardMeaningfulBottom(card)",
        "function technicalCardRequiredFooterHeight(card, footer)",
        "function technicalCardBottomClearance(card, footer)",
        "meaningfulBottom",
        'setOwnedHeightStyle(card, content, "aspect-ratio", "auto")',
    ]),
    "native cards keep a pre-insertion Emby minimum height": all(value in card for value in [
        "captureNativeCardBaseline(template.card)",
        "cardLayoutBaselines.set(card, nativeBaseline)",
        "refreshNativeCardBaseline(card)",
        "requiredNodeHeight(node, desiredBottom, baselineHeight)",
        "Scale the pre-insertion native",
    ]),
    "height repair does not mutate shared Emby scrollers": all(value not in height_sync for value in [
        'card.closest(".detailMediaStreamsItemsContainer")',
        'card.closest(".scrollSlider")',
        'card.closest(".emby-scroller")',
        'setOwnedHeightStyle(card, parent',
    ]),
    "layout verification measures final field rather than stretched footer": all(value in card for value in [
        "const meaningfulBottom = technicalCardMeaningfulBottom(card)",
        "const trailingSpace = cardRect.bottom - meaningfulBottom",
        "meaningfulBottom <= contentRect.bottom + 2",
        "meaningfulBottom <= cardRect.bottom + 2",
    ]),
    "mounted cards are never hidden merely because layout is pending": all(value in card for value in [
        "layoutPending",
        "function isRenderedCardVisible(card)",
    ]) and "technicalCardContainsContent(card)" not in card.split(
        "function isRenderedCardVisible(card)", 1
    )[1].split("function updateTechnicalCardLayoutDebug", 1)[0],
    "exit confirmation uses an owned foreground dialog": all(value in confirm_exit + tray for value in [
        "platformExitPromptOwner(windowAlreadyClosed)",
        "mbSetForeground",
        "mbTopmost",
        "owner",
        "hostHwnd",
    ]) and "messageBox.Call(\n\t\t0," not in confirm_exit,
    "only one exit confirmation can be active": all(value in main for value in [
        "exitPromptActive",
        "exitPromptMu.Lock()",
        "defer func()",
    ]),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy card/exit regression failed: " + ", ".join(failed))
print("OK Windows legacy scoped card geometry and foreground exit contract")
