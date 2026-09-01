#!/usr/bin/env python3
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
fixture = ROOT / "tests" / "fixtures" / "schema21-manual-movie.nfo"
engine = (ROOT / "engine" / "windows-engine.ps1").read_text(encoding="utf-8-sig")
root = ET.parse(fixture).getroot()
tech = root.find("./technicalspecs")
direct = {node.attrib["name"]: [item.text for item in node.findall("./item")] for node in tech.findall("./section")}

assert direct == {"Camera": ["用户认可的摄影机"], "Sound mix": ["Dolby Atmos"]}, direct
assert "IMDb 原始摄影机" not in str(direct)
assert "$tech.SelectNodes('./section')" in engine
assert "$tech.SelectNodes('.//section')" not in engine

print("OK Windows schema 21 contract: only effective direct sections enter the read-only index")
