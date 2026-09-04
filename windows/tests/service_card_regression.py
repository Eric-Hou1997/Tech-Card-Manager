#!/usr/bin/env python3
"""Windows legacy service card UI and content-driven Emby card height contract."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
service = (ROOT / "service_controller.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
visible_guard = card.split("function isRenderedCardVisible(card)", 1)[1].split(
    "function updateTechnicalCardLayoutDebug", 1
)[0]

checks = {
    "legacy source version is synchronized": all(value in source for source, value in [
        (main, 'const appVersion = "4.0.4"'),
        (engine, "$ManagerVersion = '4.0.4'"),
        (web, "v4.0.4"),
    ]),
    "Web Card cache version advances everywhere": all(value in source for source, value in [
        (card, 'const WEB_CARD_VERSION = "4.0.4"'),
        (engine, "$ExpectedWebCardVersion = '4.0.4'"),
        (platform, 'const expectedWebCardVersion = "4.0.4"'),
    ]),
    "technical card and native descendants grow with meaningful content": all(value in card for value in [
        ".cardBox",
        ".cardContent",
        ".mediaStreamInnerCardFooter",
        'setOwnedHeightStyle(card, content, "aspect-ratio", "auto")',
        "technicalCardMeaningfulBottom(card)",
        "technicalCardRequiredFooterHeight(card, footer)",
        "technicalCardBottomClearance(card, footer)",
        "refreshNativeCardBaseline(card)",
        "cardHeightStyleSnapshots",
        "previous.priority",
        "scheduleTechnicalCardHeightSync(card)",
        "observeTechnicalCardHeight(card)",
        "technicalCardContainsContent(card)",
        "meaningfulBottom <= cardRect.bottom + 2",
    ]),
    "height correction follows the final row plus one-line clearance": all(value in card for value in [
        "requiredFooterHeight",
        "meaningfulBottom",
        "desiredBottom",
        "requiredNodeHeight(node, desiredBottom, baselineHeight)",
        "last meaningful row",
    ]),
    "render verification rejects excessive empty tail": all(value in card for value in [
        "allowedTrailingSpace",
        "trailingSpace >= bottomClearance - 2",
        "trailingSpace <= allowedTrailingSpace",
        "Math.min(",
        "96,",
        "marginBottom",
        "paddingBottom",
        "borderBottomWidth",
    ]),
    "height synchronization is coalesced and idempotent": all(value in card for value in [
        "cardLayoutSyncPending",
        "cardLayoutSignatures",
        "technicalCardLayoutInputSignature(card)",
        "cardLayoutSyncPending.has(card)) return",
        "node.style.getPropertyValue(property) === value",
    ]),
    "native flex spacing shrinks and wraps before the card edge": all(
        value in card for value in [
            ".mediaStreamInnerCardFooter-cardText{width:100%!important;height:auto!important;min-width:0!important;max-width:100%!important",
            ".mediaStreamAttribute{display:flex!important;width:100%!important",
            ".mediaInfoAttributeLabel{flex:0 0 auto!important",
            ".mediaInfoAttributeValue{display:block!important;flex:1 1 0!important",
            "white-space:normal!important;overflow-wrap:anywhere!important;word-break:normal!important",
            "text-overflow:clip!important",
        ]
    ) and "column-gap:.75em!important" not in card,
    "render verification rejects horizontal clipping": all(
        value in card for value in [
            "footer.scrollWidth <= footer.clientWidth + 2",
            "node.scrollWidth <= node.clientWidth + 2",
            "rect.right <= contentRect.right + 2",
            "rect.right <= cardRect.right + 2",
            "valuesFit",
        ]
    ),
    "layout verification never hides an already mounted card": all(value in card for value in [
        "layoutVerified",
        "layoutPending",
        "updateTechnicalCardLayoutDebug(card)",
    ]) and "technicalCardContainsContent(card)" not in visible_guard,
    "ISO BDMV and Series can expand up to two native card widths": all(
        value in card for value in [
            "function syncTechnicalCardWidth(card)",
            'if (mode !== "wide-card") return',
            "measureUnwrappedTextWidth(line)",
            "standardWidth * 2",
            "preferredContentWidth",
            'setOwnedHeightStyle(card, card, "width", targetWidth + "px")',
            'scheduleRender("window-resize", 80)',
        ]
    ),
    "service API exposes only successful start time": all(value in service for value in [
        'LastStartedAt       string       `json:"last_started_at,omitempty"`',
        "recordSuccessfulStart()",
        "if c.state == serviceRunning",
        "time.Now().Format(time.RFC3339)",
    ]),
    "large yellow slab is replaced by a pale status card": all(value in web for value in [
        'class="serviceCard" id="serviceCard"',
        "background:var(--imdb-hint-bg)",
        'class="serviceStateText" id="serviceStateText"',
        'class="serviceStartedAt" id="serviceStartedAt"',
    ]),
    "right action is content-sized for every locale and says only start or stop": all(value in web for value in [
        ".serviceButton{flex:0 0 auto;min-width:max-content;min-height:72px",
        "border-radius:14px",
        "button.textContent=stopAction?'停止':'启动'",
    ]),
    "service button color follows actual running state": all(value in web for value in [
        ".serviceButton.running{background:var(--imdb-yellow)",
        "button.classList.toggle('running',state==='running')",
    ]),
    "service copy and time reflect backend state": all(value in web for value in [
        "Emby 卡片显示服务已启动",
        "Emby 卡片显示服务已关闭",
        "service?.last_started_at",
        "formatServiceStartedAt",
    ]),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy service/card regression failed: " + ", ".join(failed))
print("OK Windows legacy content-driven Emby card height and compact service status card")
