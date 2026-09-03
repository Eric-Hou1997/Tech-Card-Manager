#!/usr/bin/env python3
"""Regression contract for idle-hidden, accessible Manager scrollbars."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "document, catalog, settings, log, and long modal have explicit owners": all(
        value in web
        for value in [
            '<html lang="zh-CN" class="scrollSurface documentScroll">',
            'class="catalogList scrollSurface" id="catalogList"',
            'class="settingsBody scrollSurface" id="settingsContent"',
            'class="log scrollSurface" id="joblog" tabindex="0"',
            'class="modal small scrollSurface"',
        ]
    ),
    "idle scrollbars are transparent rather than disabled": all(
        value in web
        for value in [
            '.scrollSurface{scrollbar-width:thin;scrollbar-color:transparent transparent}',
            '.scrollSurface::-webkit-scrollbar{width:10px;height:10px}',
            '.scrollSurface::-webkit-scrollbar-thumb{background-color:transparent',
        ]
    ),
    "interactive scrollbars reveal on hover focus or scroll activity": all(
        value in web
        for value in [
            '.scrollSurface:not(.documentScroll):is(:hover,:focus-within,.is-scrolling)',
            '.scrollSurface.documentScroll.is-scrolling',
            "function revealScrollbar(surface)",
            "surface.classList.add('is-scrolling')",
            "surface.classList.remove('is-scrolling')",
            "},850)",
        ]
    ),
    "wheel touch keyboard and direct scroll all reveal the owner": all(
        value in web
        for value in [
            "surface.addEventListener('scroll'",
            "document.addEventListener('wheel'",
            "document.addEventListener('touchmove'",
            "document.addEventListener('keydown'",
            "scrollbarNavigationKeys",
            "installAutoHideScrollbars();",
        ]
    ),
    "reduced-motion and forced-colors modes remain usable": all(
        value in web
        for value in [
            "@media(prefers-reduced-motion:reduce)",
            "@media(forced-colors:active)",
            "scrollbar-color:auto",
            "background:CanvasText",
        ]
    ),
    "settings still has one scroll owner below its fixed header": (
        '#settingsBackdrop .modal{display:flex;flex-direction:column;overflow:hidden;padding:0}' in web
        and '#settingsBackdrop .settingsBody{min-height:0;overflow:auto;padding:16px 20px 20px}' in web
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Scrollbar visibility contract failed: " + ", ".join(failed))
print("OK Manager scrollbars stay hidden while idle and remain accessible on interaction")
