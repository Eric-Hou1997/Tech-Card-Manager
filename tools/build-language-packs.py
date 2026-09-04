#!/usr/bin/env python3
"""Validate and deterministically build release-attached language packs."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import sys
from typing import Optional
import zipfile


ROOT = Path(__file__).resolve().parents[1]
PACK_ROOT = ROOT / "language-packs"
CATALOG_PATH = PACK_ROOT / "catalog.json"
EMBEDDED_CATALOG_PATH = ROOT / "windows" / "language_catalog.json"
FIXED_ZIP_TIME = (1980, 1, 1, 0, 0, 0)
VERSION_RE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
EXPECTED_LOCALES = {"fr-FR", "ru-RU", "ja-JP", "es-ES", "th-TH"}
EXPECTED_SECTIONS = ["web", "core", "engine", "native", "web-card"]
NATIVE_MESSAGES = {
    "Select a Movie or TV Show media folder",
    "The service, window, or owned processes did not stop completely. The app will remain open so you can retry.",
    "Tech Card Manager did not exit completely",
    "Tech Card Manager could not open its window and will not continue running in the background.",
    "Tech Card Manager startup failed",
    "Tech Card Manager is about to exit. All services will stop and the Technical Specs card will be removed from currently open Emby pages. Media files and NFO files will not be changed.",
    "The new service has not started. Exiting will not change detected legacy components; a legacy component may continue to control the Emby Technical Specs card.",
    "The last stop attempt could not confirm that the card was removed from Emby. The app will try again; if it still fails, the window will remain open and show the error.",
    "The Tech Card Manager service is already stopped. Exiting will close the Manager, and Emby will not show a Technical Specs card supplied by the new service.",
    "An administrator maintenance transaction is running. After you confirm, the app will wait for it to finish safely and will not leave an elevated process behind.",
    "The current media-library check will be cancelled safely.", "Exit now?", "Exit Tech Card Manager?",
    "Open Tech Card Manager", "Exit Tech Card Manager",
}
WEB_CARD_MESSAGES = {"Technical Specs", "No Technical Specs data", "Runtime", "Sound mix", "Color", "Aspect ratio", "Camera", "Laboratory", "Film Length", "Negative Format", "Cinematographic Process", "Printed Film Format"}
TARGET_SCRIPT_RE = {
    "ru-RU": re.compile(r"[А-Яа-яЁё]"),
    "ja-JP": re.compile(r"[ぁ-んァ-ヶ一-龯]"),
    "th-TH": re.compile(r"[ก-๙]"),
}
ALLOWED_IDENTICAL = {
    "AI", "API", "CRLF", "Emby", "GitHub", "HTTP", "IMDb", "IMDb ID", "JSON", "LF", "NFO",
    "OK", "Qwen", "SHA-256", "Tech Card Manager", "UTF-8", "URL", "WebKit", "XML", "ZIP",
    "macOS", "—", "1 minute", "5 minutes", "15 minutes",
}
PROTECTED_TOKEN_RE = re.compile(
    r"%(?:\d+\$)?[-+#0 ']*(?:\d+|\*)?(?:\.\d+|\.\*)?[hlLjzt]*[diouxXeEfFgGaAcspnqv%]"
    r"|\{[^{}]+\}|</?[A-Za-z][^>]*>|https?://\S+|`[^`]+`"
)


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode("utf-8")


def stable_message_id(english: str) -> str:
    value = 14695981039346656037
    for byte in english.strip().encode("utf-8"):
        value ^= byte
        value = (value * 1099511628211) & 0xFFFFFFFFFFFFFFFF
    return f"legacy.{value:016x}"


def decode_js_string(value: str) -> str:
    result: list[str] = []
    escapes = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", "'": "'", '"': '"'}
    index = 0
    while index < len(value):
        if value[index] != "\\" or index + 1 >= len(value):
            result.append(value[index])
            index += 1
            continue
        marker = value[index + 1]
        if marker == "u" and index + 5 < len(value):
            result.append(chr(int(value[index + 2:index + 6], 16)))
            index += 6
            continue
        result.append(escapes.get(marker, marker))
        index += 2
    return "".join(result)


def required_messages() -> dict[str, set[str]]:
    web_source = (ROOT / "windows" / "web" / "index.html").read_text(encoding="utf-8")
    english_ui = web_source.split("'en-US':Object.freeze({locale:'en-US',messages:Object.freeze(Object.fromEntries([", 1)[1].split("]))})", 1)[0]
    web = {
        decode_js_string(match.group(2)).strip()
        for match in re.finditer(r"\[\s*'((?:\\.|[^'\\])*)'\s*,\s*'((?:\\.|[^'\\])*)'\s*\]", english_ui)
    }
    localization = (ROOT / "windows" / "localization.go").read_text(encoding="utf-8")
    core = {
        json.loads('"' + match.group(2) + '"').strip()
        for match in re.finditer(r'\{"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\}', localization)
    }
    return {"web": web, "core-engine": core, "native": NATIVE_MESSAGES, "web-card": WEB_CARD_MESSAGES}


def require_complete_translation(source: dict, locale: str, required: dict[str, set[str]]) -> None:
    missing = {
        "web": required["web"] - set(source["web"]),
        "core/engine": required["core-engine"] - (set(source["core"]) | set(source["engine"])),
        "native": required["native"] - set(source["native"]),
        "web-card": required["web-card"] - set(source["web-card"]),
    }
    missing = {section: values for section, values in missing.items() if values}
    if missing:
        summary = ", ".join(f"{section}={len(values)}" for section, values in missing.items())
        details = "; ".join(
            f"{section}: {', '.join(sorted(values))}"
            for section, values in missing.items()
        )
        raise ValueError(f"{locale}: incomplete release translation coverage ({summary}): {details}")


def load_catalog() -> dict:
    catalog = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))
    if catalog.get("schema") != 1 or catalog.get("product") != "tcm":
        raise ValueError("language catalog identity is invalid")
    app_match = VERSION_RE.fullmatch(str(catalog.get("app_version", "")))
    if not app_match:
        raise ValueError("language catalog app_version is invalid")
    if catalog.get("sections") != EXPECTED_SECTIONS:
        raise ValueError("language catalog sections are invalid")
    languages = catalog.get("languages")
    if not isinstance(languages, dict) or set(languages) != EXPECTED_LOCALES:
        raise ValueError("language catalog locale registry is invalid")
    app_version = tuple(int(value) for value in app_match.groups())
    for locale, descriptor in languages.items():
        if not isinstance(descriptor, dict):
            raise ValueError(f"{locale}: descriptor must be an object")
        revision = descriptor.get("revision")
        release_match = VERSION_RE.fullmatch(str(descriptor.get("released_with", "")))
        if isinstance(revision, bool) or not isinstance(revision, int) or revision < 1:
            raise ValueError(f"{locale}: revision is invalid")
        if descriptor.get("catalog_schema") != catalog["schema"]:
            raise ValueError(f"{locale}: catalog_schema is invalid")
        if not release_match or tuple(int(value) for value in release_match.groups()) > app_version:
            raise ValueError(f"{locale}: released_with is invalid")
        if descriptor.get("asset") != f"TCM-Language-{locale}-r{revision}.zip":
            raise ValueError(f"{locale}: asset name is invalid")
    return catalog


def build_pack(catalog: dict, locale: str, descriptor: dict, required: Optional[dict[str, set[str]]] = None) -> tuple[bytes, str]:
    revision = descriptor.get("revision")
    source_path = PACK_ROOT / locale / f"r{revision}" / "translations.json"
    source = json.loads(source_path.read_text(encoding="utf-8"))
    expected_sections = catalog.get("sections", [])
    if sorted(source) != sorted(expected_sections):
        raise ValueError(f"{locale}: translation sections do not match catalog")
    if required is not None:
        require_complete_translation(source, locale, required)
    all_english = {english for messages in source.values() for english in messages}
    encoded_sections: dict[str, dict[str, str]] = {}
    message_set: dict[str, list[str]] = {}
    for section in expected_sections:
        messages = source[section]
        if not isinstance(messages, dict):
            raise ValueError(f"{locale}/{section}: messages must be an object")
        encoded: dict[str, str] = {}
        for english, translated in messages.items():
            if not isinstance(english, str) or not english.strip() or not isinstance(translated, str) or not translated.strip():
                raise ValueError(f"{locale}/{section}: messages must contain non-empty strings")
            if PROTECTED_TOKEN_RE.findall(english) != PROTECTED_TOKEN_RE.findall(translated):
                raise ValueError(f"{locale}/{section}: protected tokens changed for {english!r}")
            if english == translated and english not in ALLOWED_IDENTICAL and len(re.findall(r"[A-Za-z]+", english)) >= 2:
                raise ValueError(f"{locale}/{section}: untranslated English sentence {english!r}")
            script_re = TARGET_SCRIPT_RE.get(locale)
            if script_re and english not in ALLOWED_IDENTICAL and len(re.findall(r"[A-Za-z]+", english)) >= 3 and not script_re.search(translated):
                raise ValueError(f"{locale}/{section}: target script is missing for {english!r}")
            if translated in all_english and translated != english:
                raise ValueError(f"{locale}/{section}: translation for {english!r} matches another English source key")
            message_id = stable_message_id(english)
            if message_id in encoded:
                raise ValueError(f"{locale}/{section}: stable message id collision")
            encoded[message_id] = translated
        encoded_sections[section] = dict(sorted(encoded.items()))
        message_set[section] = sorted(messages)
    message_set_hash = hashlib.sha256(json_bytes(message_set)).hexdigest()
    section_files = {f"{section}.json": json_bytes(messages) for section, messages in encoded_sections.items()}
    manifest = {"schema": 1, "product": catalog["product"], "locale": locale, "revision": revision,
                "released_with": descriptor["released_with"], "catalog_schema": descriptor["catalog_schema"],
                "message_set_hash": message_set_hash,
                "files": {name: hashlib.sha256(payload).hexdigest() for name, payload in sorted(section_files.items())}}
    files = {"manifest.json": json_bytes(manifest)}
    files.update(section_files)
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name in sorted(files):
            entry = zipfile.ZipInfo(name, FIXED_ZIP_TIME)
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(entry, files[name], compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    return output.getvalue(), message_set_hash


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    parser.add_argument("--changed-only", action="store_true")
    parser.add_argument("--update-catalog", action="store_true")
    parser.add_argument("--app-version")
    parser.add_argument("--require-complete", action="store_true")
    args = parser.parse_args()
    catalog = load_catalog()
    if args.app_version and catalog["app_version"] != args.app_version:
        raise ValueError(f"catalog targets {catalog['app_version']}, not {args.app_version}")
    changed = False
    built: list[str] = []
    common_message_set_hash: Optional[str] = None
    required = required_messages() if args.require_complete else None
    for locale, descriptor in sorted(catalog["languages"].items()):
        payload, message_set_hash = build_pack(catalog, locale, descriptor, required)
        if common_message_set_hash is None:
            common_message_set_hash = message_set_hash
        elif message_set_hash != common_message_set_hash:
            raise ValueError(f"{locale}: message keys differ from the other language packs")
        digest = hashlib.sha256(payload).hexdigest()
        if descriptor.get("message_set_hash") != message_set_hash or descriptor.get("sha256") != digest:
            if not args.update_catalog:
                raise ValueError(f"{locale}: catalog hashes are stale; run with --update-catalog")
            if descriptor["released_with"] != catalog["app_version"]:
                raise ValueError(f"{locale}: an older published revision is immutable; create a new revision for {catalog['app_version']}")
            descriptor["message_set_hash"] = message_set_hash
            descriptor["sha256"] = digest
            changed = True
        if args.output and (not args.changed_only or descriptor["released_with"] == catalog["app_version"]):
            args.output.mkdir(parents=True, exist_ok=True)
            target = args.output / descriptor["asset"]
            if target.exists():
                raise FileExistsError(f"refusing to overwrite {target}")
            target.write_bytes(payload)
            built.append(target.name)
    if args.update_catalog and changed:
        payload = json_bytes(catalog)
        CATALOG_PATH.write_bytes(payload)
        EMBEDDED_CATALOG_PATH.write_bytes(payload)
    elif EMBEDDED_CATALOG_PATH.read_bytes() != CATALOG_PATH.read_bytes():
        raise ValueError("embedded language catalog differs from release source catalog")
    print(f"OK {catalog['product']} language catalog: {len(catalog['languages'])} locales; built {len(built)} assets")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"ERROR {error}", file=sys.stderr)
        raise SystemExit(1)
