"""Reviewed QuickTime source fixtures for selected-artifact replay tests."""

import json
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
REVIEWED_VERSIONS = frozenset({"11.78", "12.64", "13.59"})


def selected_document():
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if pin not in REVIEWED_VERSIONS:
        raise ValueError(f"unreviewed QuickTime source pin: {pin}")
    document = json.loads((HERE / "fixtures" / f"quicktime_source_{pin.replace('.', '_')}.json").read_text())
    if document.get("exiftool_version") != pin:
        raise ValueError(f"QuickTime source fixture differs from repository pin: {pin}")
    return document
