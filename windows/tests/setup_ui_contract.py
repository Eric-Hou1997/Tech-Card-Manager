#!/usr/bin/env python3
"""Source contract for the visible Windows legacy setup flow."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")

before_maintenance, maintenance = web.split('<details class="maintenance">', 1)
maintenance = maintenance.split("</details>", 1)[0]

checks = {
    "web card action is visible outside maintenance": (
        web.count('data-action="repair-web"') == 1
        and 'data-action="repair-web"' in before_maintenance
        and 'data-action="repair-web"' not in maintenance
    ),
    "web card action shares the interval row": (
        'class="intervalActions"' in web
        and '<select class="select" id="interval">' in web
        and 'class="intervalActions"><label class="muted">检查周期' in web
        and 'data-action="repair-web"' in web.split('class="intervalActions"', 1)[1].split('</div>', 1)[0]
        and "settingActionRow" not in web
    ),
    "three setup cards have stable identities": all(
        value in web for value in ["setupStepEmby", "setupStepRoots", "setupStepWeb"]
    ),
    "all completion cards receive the green state": (
        ".setupStep.done" in web
        and "step.classList.toggle('done',done)" in web
        and web.count("renderSetupStep('#setupStep") == 3
    ),
    "completion is based on backend evidence": all(
        value in platform + web
        for value in [
            'st.Extra["emby_detected"]',
            'st.Extra["web_setup_complete"]',
            "!!s.roots_configured",
            "!!ex.emby_detected",
            "!!ex.web_setup_complete",
        ]
    ),
    "web setup means installed files match": (
        'st.Extra["web_setup_complete"] = indexInjected && liveJSExists && liveJSVersion == expectedWebCardVersion && liveJSMatches'
        in platform
    ),
    "setup copy points to the visible action": "点击下方按钮完成" in web,
    "settings header stays available while the modal scrolls": all(
        value in web
        for value in [
            "#settingsBackdrop .modal{display:flex;flex-direction:column;overflow:hidden;padding:0}",
            "#settingsBackdrop .settingsBody{min-height:0;overflow:auto;padding:16px 20px 20px}",
            '<div class="settingsBody scrollSurface" id="settingsContent">',
            "data-close-modal=\"settingsBackdrop\" aria-label=\"关闭\"",
            '<div class="setupFlowIntro">配置媒体目录和检查周期</div>',
        ]
    ) and '<div class="muted">配置媒体目录和检查周期</div>' not in web and "settingsContent.insertAdjacentHTML('afterbegin'" in web,
    "settings has one persistent close control and no redundant done button": (
        web.count('data-close-modal="settingsBackdrop"') == 1
        and ">完成</button>" not in web
    ),
    "media-directory setup uses the standard settings-heading treatment": all(
        value in web
        for value in [
            '<div class="settingGroup"><h3>设置媒体目录</h3>',
            "#settingsBackdrop .settingGroup h3{color:var(--text);font-size:14px;font-weight:700}",
        ]
    ),
    "setup-flow introduction matches standard settings-heading emphasis": (
        ".setupFlowIntro{margin:0 0 10px;color:var(--text);font-size:14px;font-weight:700}"
        in web
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("Windows legacy setup UI contract failed: " + ", ".join(failed))
print("OK Windows legacy visible setup flow contract")
