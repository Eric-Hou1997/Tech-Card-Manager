#!/usr/bin/env python3
"""Upgrade, cache, task-language, and localization-boundary regression contract."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
main = (ROOT / "main.go").read_text(encoding="utf-8")
localization = (ROOT / "localization.go").read_text(encoding="utf-8")
platform = (ROOT / "platform_windows.go").read_text(encoding="utf-8")
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
card = (ROOT / "engine" / "technical-specs-card.js").read_text(encoding="utf-8")
web = (ROOT / "web" / "index.html").read_text(encoding="utf-8")

checks = {
    "old settings without a language default to Chinese without a separate migration": all(
        value in source for source, value in [
            (localization, 'const defaultLanguage = "zh-CN"'),
            (main, "s := Settings{IntervalSeconds: 60, Language: defaultLanguage}"),
            (main, "_ = json.Unmarshal(b, &s)"),
            (main, "s.Language = normalizedLanguage(s.Language)"),
        ]
    ) and "languageMigration" not in main + platform,
    "unsupported saved language fails closed while supported English survives": all(
        value in localization for value in [
            "if supportedLanguage(language)",
            "return defaultLanguage",
            '{Code: "en-US"',
        ]
    ),
    "recovery writes an explicit default language but preserves normal old settings": (
        "Language: defaultLanguage" in platform
        and "if _, validErr := validatedConfiguredSettings(current); validErr == nil" in platform
        and 'return "", nil' in platform
    ),
    "language changes write only Manager settings and never NFO or cache payloads": (
        'raw["language"]' in main
        and "set.Language = language" in main
        and "saveSettings(set)" in main
        and "manager-items-cache.json" not in main
    ),
    "public index and parser cache schemas remain unchanged": (
        "$ParserCacheVersion = 'tech-card-cache-1'" in engine
        and "data.Version != 7" in main
        and "CARD_LOCALES" in card
    ),
    "new task language is captured once and persisted with the job": all(
        value in main for value in [
            "jobLanguage := currentLanguage()",
            "Language:     jobLanguage",
            "newLocalizedLineWriter(f, jobLanguage)",
        ]
    ),
    "old task logs are read as historical bytes and never rewritten": (
        "os.ReadFile(jobLogPath())" in main
        and "tailString(string(b), 40000)" in main
        and "localizeStatusForPresentation" in main
        and "status.Job" not in localization
    ),
    "PowerShell receives the frozen writer language on every engine action": (
        engine.count("[string]$OutputLanguage = 'zh-CN'") == 1
        and platform.count('"-OutputLanguage", outputLanguageForWriter(w)') >= 6
    ),
    "Manager language registry and Web Card locale registry are independent of data keys": all(
        value in source for source, value in [
            (localization, "var languageOptions = []LanguageOption"),
            (web, "const UI_LOCALES=Object.freeze"),
            (web, "const FIELD_LABELS=Object.freeze"),
            (card, "const CARD_LOCALES = Object.freeze"),
            (card, "const DEFAULT_CARD_LOCALE = \"zh-CN\""),
        ]
    ),
}

failed = [name for name, ok in checks.items() if not ok]
for name, ok in checks.items():
    print(("OK  " if ok else "FAIL ") + name)
if failed:
    raise SystemExit("TCM multilingual upgrade regression failed: " + ", ".join(failed))
print("OK TCM multilingual upgrade keeps old settings, caches, logs, and NFO data compatible")
