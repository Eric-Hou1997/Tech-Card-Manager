#!/usr/bin/env python3
"""Verify the file:// preview always renders the complete language picker."""
from pathlib import Path
import os
import re
import shutil
import signal
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "web" / "index.html").read_text(encoding="utf-8")
EXPECTED_CODES = ("zh-CN", "zh-Hant", "en-US", "fr-FR", "ru-RU", "ja-JP", "es-ES", "th-TH")
EXPECTED_NATIVE_NAMES = {
    "zh-CN": "简体中文",
    "zh-Hant": "繁體中文",
    "en-US": "",
    "fr-FR": "Français",
    "ru-RU": "Русский",
    "ja-JP": "日本語",
    "es-ES": "Español",
    "th-TH": "ไทย",
}


def chrome_binary():
    candidates = [
        os.environ.get("TECH_CARD_MANAGER_CHROME", "").strip(),
        shutil.which("google-chrome") or "",
        shutil.which("chromium") or "",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ]
    return next((Path(item) for item in candidates if item and Path(item).is_file()), None)


assert re.search(r"\.languageCurrent\{[^}]*background-image:none", SOURCE)
assert "languageOptionsState=DEFAULT_LANGUAGE_OPTIONS.map" in SOURCE

chrome = chrome_binary()
if chrome is None:
    if os.environ.get("TECH_CARD_REQUIRE_BROWSER") == "1":
        raise AssertionError("Chrome/Chromium is required for the language-picker DOM gate")
    print("SKIP TCM language-picker DOM: Chrome/Chromium not installed")
    raise SystemExit(0)

with tempfile.TemporaryDirectory(prefix="tech-card-language-picker-") as temp_dir:
    temp = Path(temp_dir)
    web = temp / "index.html"
    english_preview = SOURCE.replace(
        "</body>",
        "<script>setUILanguage('zh-CN');renderLanguagePicker();"
        "document.documentElement.dataset.englishNativeName="
        "document.querySelector('[data-language-code=\"en-US\"] .languageNative').textContent;"
        "document.documentElement.dataset.simplifiedEnglishDisplay="
        "document.querySelector('[data-language-code=\"en-US\"]>span:nth-child(2)').textContent;"
        "setUILanguage('zh-Hant');renderLanguagePicker();"
        "document.documentElement.dataset.traditionalEnglishNative="
        "document.querySelector('[data-language-code=\"en-US\"] .languageNative').textContent;"
        "document.documentElement.dataset.traditionalEnglishDisplay="
        "document.querySelector('[data-language-code=\"en-US\"]>span:nth-child(2)').textContent;"
        "document.documentElement.dataset.englishEndonymEverywhere="
        "Object.keys(LANGUAGE_NAMES).every(language=>{setUILanguage(language);renderLanguagePicker();"
        "return document.querySelector('[data-language-code=\"en-US\"] .languageNative').textContent==="
        "(language==='en-US'?'':'English (United States)')})?'ok':'failed';"
        "setUILanguage('en-US');renderLanguagePicker()</script></body>",
        1,
    )
    web.write_text(english_preview, encoding="utf-8")
    dump_path = temp / "dom.html"
    error_path = temp / "chrome.stderr"
    with dump_path.open("w", encoding="utf-8") as dump, error_path.open("w", encoding="utf-8") as errors:
        process = subprocess.Popen(
            [
                str(chrome),
                "--headless=new",
                "--disable-gpu",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-default-apps",
                "--no-first-run",
                "--password-store=basic",
                "--use-mock-keychain",
                "--allow-file-access-from-files",
                "--window-size=1100,900",
                f"--user-data-dir={temp / 'profile'}",
                "--virtual-time-budget=1200",
                "--dump-dom",
                web.as_uri(),
            ],
            stdout=dump,
            stderr=errors,
            text=True,
            start_new_session=True,
        )
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=3)

    dom = dump_path.read_text(encoding="utf-8")
    errors = error_path.read_text(encoding="utf-8")[-2000:]
    assert dom, "Chrome produced no DOM output: " + errors
    assert 'data-language-picker="ok"' in dom, "language picker did not reach its verified state"
    assert 'data-english-native-name="English (United States)"' in dom, (
        "Simplified-Chinese view did not preserve the English endonym"
    )
    assert 'data-simplified-english-display="英语（美国）"' in dom
    assert 'data-traditional-english-display="英語（美國）"' in dom
    assert 'data-traditional-english-native="English (United States)"' in dom
    assert 'data-english-endonym-everywhere="ok"' in dom
    for code in EXPECTED_CODES:
        assert f'data-language-code="{code}"' in dom, f"missing static-preview language {code}"
        match = re.search(
            rf'data-language-code="{re.escape(code)}"[\s\S]*?'
            r'<span class="languageNative" data-i18n-user="">([^<]*)</span>',
            dom,
        )
        assert match and match.group(1) == EXPECTED_NATIVE_NAMES[code], (
            f"wrong English-view native name for {code}: {match.group(1) if match else 'missing'}"
        )
    assert re.search(r'id="languageCurrentFlag"[^>]*>\s*<svg', dom), "current-language flag was not rendered"

print("OK TCM file preview renders all languages, the current flag, and one custom chevron")
