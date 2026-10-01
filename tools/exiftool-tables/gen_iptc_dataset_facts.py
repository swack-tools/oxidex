#!/usr/bin/env python3
"""Generate IPTC IIM names, format classes and list flags from pinned Perl."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "src/parsers/jpeg/generated_iptc_dataset_facts.rs"
SOURCE = Path(__file__).with_name("export_iptc_dataset_facts.pl")
FORMAT = re.compile(r"^(int(?:8|16|32)u|string\[\d+(?:,\d+)?\]|digits\[\d+(?:,\d+)?\]|undef\[\d+(?:,\d+)?\])$")


def render(data: dict) -> str:
    pin = (ROOT / ".exiftool-version").read_text().strip()
    rows = data.get("rows")
    if (data.get("schema") != "iptc_dataset_facts_v1"
            or data.get("exiftool_version") != pin
            or data.get("module") != "Image/ExifTool/IPTC.pm"
            or not isinstance(rows, list) or len(rows) != 84):
        raise ValueError("selected IPTC dataset source contract changed")
    source_sha = data.get("source_sha256")
    if not isinstance(source_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", source_sha):
        raise ValueError("invalid IPTC.pm source SHA-256")
    keys = [(row.get("record"), row.get("dataset")) for row in rows]
    if keys != sorted(set(keys)) or any(record not in (1, 2) or not isinstance(dataset, int)
                                        or not 0 <= dataset <= 255 for record, dataset in keys):
        raise ValueError("unsupported IPTC dataset identities")
    parts = [
        "//! IPTC IIM record-table facts. DO NOT EDIT BY HAND.\n",
        "//! Regenerate with `tools/exiftool-tables/gen_iptc_dataset_facts.py`.\n",
        f"//! ExifTool {pin}, IPTC.pm SHA-256 {source_sha}.\n\n",
        "use super::IptcFormat;\n\n",
        "#[derive(Debug, Clone, Copy)]\n",
        "pub(super) struct DatasetFact {\n",
        "    pub name: &'static str,\n",
        "    pub format: IptcFormat,\n",
        "    pub list: bool,\n",
        "    pub conversion: bool,\n",
        "}\n\n",
        "pub(super) fn find(record: u8, dataset: u8) -> Option<&'static DatasetFact> {\n",
        "    let key = (record, dataset);\n",
        "    ROWS.binary_search_by_key(&key, |row| row.0).ok().map(|index| &ROWS[index].1)\n",
        "}\n\n",
        "#[rustfmt::skip]\n",
        "static ROWS: &[((u8, u8), DatasetFact)] = &[\n",
    ]
    for row in rows:
        name = row.get("name")
        fmt = row.get("format")
        if (not isinstance(name, str) or not name or "\x00" in name
                or not isinstance(fmt, str) or not FORMAT.fullmatch(fmt)
                or not isinstance(row.get("list"), bool)
                or not isinstance(row.get("conversion"), bool)):
            raise ValueError(f"unsupported IPTC dataset row: {row}")
        kind = ("Int" if fmt.startswith("int") else "Str" if fmt.startswith("string")
                else "Digits" if fmt.startswith("digits") else "Undef")
        parts.append(
            f"    (({row['record']}, {row['dataset']}), DatasetFact {{ name: {json.dumps('IPTC:' + name, ensure_ascii=False)}, "
            f"format: IptcFormat::{kind}, list: {str(row['list']).lower()}, "
            f"conversion: {str(row['conversion']).lower()} }}),\n"
        )
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
    parser.add_argument("--perl", required=True)
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.out.resolve() != OUT:
        raise ValueError("generated Rust target must be generated_iptc_dataset_facts.rs")
    output = subprocess.check_output(
        [args.perl, str(SOURCE), "--exiftool-dir", str(args.exiftool_dir.resolve())],
        cwd=ROOT,
    )
    rendered = render(json.loads(output))
    if args.check:
        if not OUT.is_file() or OUT.read_text() != rendered:
            raise ValueError(f"generated IPTC dataset facts differ: {OUT}")
    else:
        OUT.write_text(rendered)
    print("IPTC dataset facts current: 84 rows")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as exc:
        raise SystemExit(str(exc)) from exc
