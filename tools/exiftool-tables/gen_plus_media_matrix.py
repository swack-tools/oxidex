#!/usr/bin/env python3
"""Generate the selected PLUS MediaSummaryCode lookup from pinned Perl."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "src/parsers/xmp/generated_plus_media_matrix.rs"


def render(data: dict) -> str:
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if (data.get("schema") != "plus_media_matrix_v1"
            or data.get("exiftool_version") != pin
            or data.get("module") != "Image/ExifTool/PLUS.pm"
            or data.get("module_version") != "1.02"
            or data.get("exceptions") != {"OTHER": "CODE", "Notes": "STRING"}):
        raise ValueError("selected PLUS MediaSummaryCode source contract changed")
    rows = data.get("rows")
    if (not isinstance(rows, list) or len(rows) != 2143
            or any(not isinstance(row, list) or len(row) != 2
                   or not isinstance(row[0], str) or not isinstance(row[1], str)
                   or len(row[0]) != 4 or row[0][0] not in "0123456789"
                   or any(c not in "ABCDEFGHIJKLMNOPQRSTUVWXYZ" for c in row[0][1:])
                   for row in rows)
            or [row[0] for row in rows] != sorted(set(row[0] for row in rows))):
        raise ValueError("unsupported PLUS MediaSummaryCode rows")
    sha = data.get("source_sha256")
    if not isinstance(sha, str) or len(sha) != 64 or any(c not in "0123456789abcdef" for c in sha):
        raise ValueError("invalid PLUS.pm source SHA-256")
    parts = [
        "//! PLUS::XMP MediaSummaryCode PrintConv literals. DO NOT EDIT BY HAND.\n",
        "//! Regenerate with `tools/exiftool-tables/gen_plus_media_matrix.py`.\n",
        f"//! ExifTool {pin}, PLUS.pm {data['module_version']}, SHA-256 {sha}.\n\n",
        "pub fn lookup(code: &str) -> Option<&'static str> {\n",
        "    ROWS.binary_search_by(|row| row.0.cmp(&code)).ok()",
        ".map(|index| ROWS[index].1)\n}\n\n",
        "#[rustfmt::skip]\n",
        f"pub static ROWS: [(&str, &str); {len(rows)}] = [\n",
    ]
    for key, value in rows:
        if "\x00" in value:
            raise ValueError(f"NUL in PLUS MediaSummaryCode value {key}")
        parts.append(f"    ({json.dumps(key)}, {json.dumps(value, ensure_ascii=False)}),\n")
    parts.append("];\n")
    formatted = subprocess.run(
        [os.environ.get("RUSTFMT", "rustfmt"), "--edition", "2024",
         "--config-path", str(ROOT / "rustfmt.toml"), "--emit", "stdout"],
        input="".join(parts), text=True, capture_output=True, check=True,
    )
    return formatted.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exiftool-dir", type=Path, required=True)
    parser.add_argument("--perl", default=os.environ.get("OXIDEX_PERL", "perl"))
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.out.resolve() != OUT:
        raise ValueError("generated Rust target must be generated_plus_media_matrix.rs")
    output = subprocess.check_output(
        [args.perl, str(Path(__file__).with_name("export_plus_media_matrix.pl")),
         "--exiftool-dir", str(args.exiftool_dir.resolve())], cwd=ROOT,
    )
    result = render(json.loads(output))
    if args.check:
        if not OUT.is_file() or OUT.read_text() != result:
            raise ValueError(f"generated PLUS MediaSummaryCode lookup differs: {OUT}")
    else:
        OUT.write_text(result)
    print("PLUS MediaSummaryCode lookup current: 2143 rows")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as exc:
        raise SystemExit(str(exc)) from exc
