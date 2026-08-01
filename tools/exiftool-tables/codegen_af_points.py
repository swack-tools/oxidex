#!/usr/bin/env python3
"""JSON (from dump_af_points.pl) -> src/parsers/tiff/makernotes/nikon/af_points.rs

Only the data section (above the "hand-written below" marker) is
regenerated; everything below the marker is preserved verbatim so this
script can be re-run without clobbering the print-conversion functions.
"""
import json
import subprocess
import sys
from pathlib import Path

MARKER = "// --- hand-written below: do not edit above this line by hand ---"

HASH_TABLES = ["afPoints51", "afPoints39", "afPoints105", "afPoints135", "afPoints153", "afPoints81"]
ARRAY_TABLES = ["afPoints11", "afPoints231", "afPoints299", "afPoints405"]


def rust_name(name: str) -> str:
    # afPoints51 -> AF_POINTS_51
    digits = "".join(c for c in name if c.isdigit())
    return f"AF_POINTS_{digits}"


def emit_hash(name: str, points: dict) -> str:
    pairs = sorted(((int(k), v) for k, v in points.items()), key=lambda p: p[0])
    body = ", ".join(f'({k}, "{v}")' for k, v in pairs)
    return f"pub const {rust_name(name)}: &[(u8, &str)] = &[{body}];\n"


def emit_array(name: str, points: list) -> str:
    body = ", ".join(f'"{p}"' for p in points)
    return f"pub const {rust_name(name)}: &[&str] = &[{body}];\n"


def main() -> None:
    json_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("tools/exiftool-tables/af_points.json")
    rs_path = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(
        "src/parsers/tiff/makernotes/nikon/af_points.rs"
    )
    data = json.loads(json_path.read_text())

    lines = [
        "//! Nikon AF point-name grids, transcribed from ExifTool's Nikon.pm\n",
        "//! `afPoints*` lexicals by `tools/exiftool-tables/dump_af_points.pl` +\n",
        "//! `codegen_af_points.py`. Regenerate with both scripts; do not hand-edit\n",
        "//! the data section below.\n\n",
    ]
    for name in HASH_TABLES:
        lines.append(emit_hash(name, data[name]["points"]))
    for name in ARRAY_TABLES:
        lines.append(emit_array(name, data[name]["points"]))
    lines.append(f"\n{MARKER}\n")

    existing = rs_path.read_text() if rs_path.exists() else ""
    hand_written = ""
    if MARKER in existing:
        hand_written = existing.split(MARKER, 1)[1]

    rs_path.write_text("".join(lines) + hand_written)
    subprocess.run(["rustfmt", str(rs_path)], check=True)
    print(f"wrote {rs_path}")


if __name__ == "__main__":
    main()
