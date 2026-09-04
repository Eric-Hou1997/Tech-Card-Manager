#!/usr/bin/env python3
"""Prove release language packs are deterministic and catalog-bound."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parents[2]
CATALOG = json.loads((ROOT / "language-packs" / "catalog.json").read_text(encoding="utf-8"))
BUILDER = ROOT / "tools" / "build-language-packs.py"

assert CATALOG["schema"] == 1
assert CATALOG["product"] == "tcm"
assert CATALOG["app_version"] == "v4.1.0"
assert (ROOT / "windows" / "language_catalog.json").read_bytes() == (
    ROOT / "language-packs" / "catalog.json"
).read_bytes()

with tempfile.TemporaryDirectory(prefix="tcm-language-pack-") as temporary:
    first = Path(temporary) / "first"
    second = Path(temporary) / "second"
    for output in (first, second):
        subprocess.run(
            [
                "python3",
                str(BUILDER),
                "--output",
                str(output),
                "--changed-only",
                "--app-version",
                CATALOG["app_version"],
            ],
            cwd=ROOT,
            check=True,
        )

    expected_assets = sorted(
        descriptor["asset"]
        for descriptor in CATALOG["languages"].values()
        if descriptor["released_with"] == CATALOG["app_version"]
    )
    assert sorted(path.name for path in first.iterdir()) == expected_assets
    assert sorted(path.name for path in second.iterdir()) == expected_assets

    allowed_sections = set(CATALOG["sections"])
    for locale, descriptor in CATALOG["languages"].items():
        if descriptor["released_with"] != CATALOG["app_version"]:
            continue
        first_payload = (first / descriptor["asset"]).read_bytes()
        second_payload = (second / descriptor["asset"]).read_bytes()
        assert first_payload == second_payload, f"{locale}: nondeterministic ZIP"
        assert hashlib.sha256(first_payload).hexdigest() == descriptor["sha256"]
        with zipfile.ZipFile(first / descriptor["asset"]) as archive:
            assert set(archive.namelist()) == {"manifest.json"} | {
                f"{section}.json" for section in allowed_sections
            }
            manifest = json.loads(archive.read("manifest.json"))
            assert manifest == {
                "schema": 1,
                "product": CATALOG["product"],
                "locale": locale,
                "revision": descriptor["revision"],
                "released_with": descriptor["released_with"],
                "catalog_schema": descriptor["catalog_schema"],
                "message_set_hash": descriptor["message_set_hash"],
                "files": {
                    name: hashlib.sha256(archive.read(name)).hexdigest()
                    for name in sorted(archive.namelist())
                    if name != "manifest.json"
                },
            }

with tempfile.TemporaryDirectory(prefix="tcm-language-pack-immutable-") as temporary:
    checkout = Path(temporary)
    (checkout / "tools").mkdir()
    (checkout / "windows").mkdir()
    shutil.copy2(BUILDER, checkout / "tools" / BUILDER.name)
    shutil.copytree(ROOT / "language-packs", checkout / "language-packs")
    shutil.copy2(ROOT / "windows" / "language_catalog.json", checkout / "windows" / "language_catalog.json")
    catalog_path = checkout / "language-packs" / "catalog.json"
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    catalog["app_version"] = "v4.2.0"
    catalog_path.write_text(json.dumps(catalog, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    translation_path = checkout / "language-packs" / "fr-FR" / "r1" / "translations.json"
    translations = json.loads(translation_path.read_text(encoding="utf-8"))
    translations["web"]["Settings"] += " modifié"
    translation_path.write_text(json.dumps(translations, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    rejected = subprocess.run(
        ["python3", str(checkout / "tools" / BUILDER.name), "--update-catalog"],
        cwd=checkout,
        capture_output=True,
        text=True,
    )
    assert rejected.returncode != 0
    assert "older published revision is immutable" in rejected.stderr

print("OK deterministic TCM language-pack release contract")
