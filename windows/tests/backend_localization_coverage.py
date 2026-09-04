#!/usr/bin/env python3
"""Ensure new English task output has translations for product-owned source copy."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
localization = (ROOT / "localization.go").read_text(encoding="utf-8")
pairs = re.findall(r'\{"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\}', localization)
pairs.sort(key=lambda pair: len(pair[0]), reverse=True)

def translated(value: str) -> str:
    for chinese, english in pairs:
        value = value.replace(chinese, english)
    return value

failures = []
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
for line_number, line in enumerate(engine.splitlines(), 1):
    for match in re.finditer(r"'([^'\r\n]*[\u3400-\u9fff][^'\r\n]*)'|\"([^\"\r\n]*[\u3400-\u9fff][^\"\r\n]*)\"", line):
        source = match.group(1) if match.group(1) is not None else match.group(2)
        if re.search(r"[\u3400-\u9fff]", translated(source)):
            failures.append(f"windows-engine.ps1:{line_number}: {source}")

for path in ROOT.glob("*.go"):
    if path.name.endswith("_test.go") or path.name in {"localization.go", "language_packs.go"}:
        continue
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if any(call in line for call in ("currentLocalized(", "localized(", "currentNativeLocalized(", "localizedNative(")):
            continue
        for match in re.finditer(r'"([^"\\]*(?:\\.[^"\\]*)*[\u3400-\u9fff][^"\\]*(?:\\.[^"\\]*)*)"|`([^`]*[\u3400-\u9fff][^`]*)`', line):
            source = match.group(1) if match.group(1) is not None else match.group(2)
            if source == "找不到":  # Internal compatibility test for localized schtasks output.
                continue
            if re.search(r"[\u3400-\u9fff]", translated(source)):
                failures.append(f"{path.name}:{line_number}: {source}")

if failures:
    raise SystemExit("Missing English backend translations:\n" + "\n".join(failures))
print("OK all product-owned Go and PowerShell task copy has an English translation path")
