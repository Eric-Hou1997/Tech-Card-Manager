#!/usr/bin/env python3
"""Regression contract derived from the user's Emby 4.9.5 MKV/ISO/Series DOM captures."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
visible_guard = card.split("function isRenderedCardVisible(card)", 1)[1].split(
    "function updateTechnicalCardLayoutDebug", 1
)[0]

checks = {
    "all Chinese spec labels use four-character display names everywhere": all(
        card_label in card and web_label in web
        for card_label, web_label in [
            ('"Runtime": "正片时长"', "'Runtime':'正片时长'"),
            ('"Sound mix": "声音制式"', "'Sound mix':'声音制式'"),
            ('"Color": "色彩类型"', "'Color':'色彩类型'"),
            ('"Aspect ratio": "画幅比例"', "'Aspect ratio':'画幅比例'"),
            ('"Camera": "摄影器材"', "'Camera':'摄影器材'"),
            ('"Laboratory": "冲印流程"', "'Laboratory':'冲印流程'"),
            ('"Film Length": "胶片长度"', "'Film Length':'胶片长度'"),
            ('"Negative Format": "底片格式"', "'Negative Format':'底片格式'"),
            ('"Cinematographic Process": "摄影工艺"', "'Cinematographic Process':'摄影工艺'"),
            ('"Printed Film Format": "放映格式"', "'Printed Film Format':'放映格式'"),
        ]
    ),
    "Emby itemView is a supported visible detail root": all(value in card for value in [
        '".itemView.view-item-item:not(.hide)"',
        '".itemView:not(.hide)"',
    ]),
    "MKV clones the live native video card": all(value in card for value in [
        "template.card.cloneNode(true)",
        "measureNativeCardWidth(native.card)",
        "nativeShell: \"cloned-live-video-card\"",
    ]),
    "fallback shell matches captured Emby card classes": all(value in card for value in [
        'card.className = "card backdropCard card-horiz backdropCard-horiz"',
        'box.className = "cardBox cardBox-touchzoom"',
        "cardContent cardImageContainer cardContent-background cardContent-bxsborder-fv defaultCardBackground cardPadder-backdrop mediaStreamPadder",
        "innerCardFooter mediaStreamInnerCardFooter",
    ]),
    "attributes use Emby native row classes": all(value in card for value in [
        "mediaStreamInnerCardFooter-cardText cardText text-align-start innerFooter-cardText",
        "flex mediaStreamAttribute",
        "mediaInfoAttributeLabel",
        "mediaInfoAttributeValue secondaryText",
    ]),
    "ISO reuses a visible empty native container": all(value in card for value in [
        "findVisibleNativeMediaContainer(root)",
        'placement: "visible-native-media-container"',
    ]),
    "Series mounts beside rather than inside hidden media": (
        "before-hidden-native-media-section" in card
        and "after-media-sources" not in card
    ),
    "a render is not reported until it is visibly laid out": all(value in card for value in [
        "function isRenderedCardVisible(card)",
        "rect.width > 1 && rect.height > 1",
        'debugFailure("native-card-not-visible"',
        'debugFailure("native-target-not-visible"',
    ]),
    "content overflow is repaired without removing the mounted card": (
        "technicalCardContainsContent(card)" not in visible_guard
        and "window.__technicalSpecsDebug.layoutPending = !layoutVerified" in card
    ),
    "renderer exceptions are diagnosable": all(value in card for value in [
        'result = "render-exception"',
        'debugFailure("render-exception"',
        "error && error.message",
    ]),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows native Web Card DOM contract failed: " + ", ".join(failed))
print("OK Windows Web Card follows captured Emby 4.9.5 native MKV/ISO/Series DOM")
