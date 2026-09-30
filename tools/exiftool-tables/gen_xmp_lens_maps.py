#!/usr/bin/env python3
"""Generate the exact XMP::PrintLensID-selected lookup maps from pinned Perl.

The Perl exporter authenticates the selected source tree and records the
dynamic OTHER exceptions. This driver verifies the existing Canon/Pentax
registries and writes a deterministic Rust module. --check never modifies it.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
OUT = ROOT / "src/composite/xmp_lens_maps.rs"
LENS_DATA = ROOT / "src/parsers/tiff/makernotes/lens_data.rs"


def selected_source(perl: str, tree: Path) -> dict:
    output = subprocess.check_output(
        [perl, str(HERE / "export_xmp_lens_maps.pl"), "--exiftool-dir", str(tree)],
        cwd=ROOT,
    )
    data = json.loads(output)
    if data.get("schema") != "xmp_lens_maps_v1":
        raise ValueError("unrecognized Perl exporter schema")
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if data["exiftool_version"] != pin:
        raise ValueError("selected ExifTool version disagrees with repository pin")
    if [m["make"] for m in data["makers"]] != [
        "Canon", "Nikon", "Pentax", "Sony", "Sigma", "Samsung", "Leica"
    ]:
        raise ValueError("XMP maker dispatch order changed")
    return data


def rust_string(s: str) -> str:
    if "\x00" in s:
        raise ValueError("lens label contains NUL")
    # JSON's ordinary escapes are also Rust string escapes. Keep UTF-8 text
    # literal so generated names are inspectable and do not depend on locale.
    return json.dumps(s, ensure_ascii=False)


def verify_existing(perl: str, tree: Path, data: dict) -> None:
    # The existing source-driven Canon/Pentax alternatives producer checks its
    # entire output, including every fractional chain and source label.
    subprocess.run(
        [perl, str(HERE / "dump_lens_alternatives.pl"),
         "--exiftool-dir", str(tree), "--check",
         str(ROOT / "src/composite/lens_alternatives.rs")],
        cwd=ROOT, check=True,
    )
    source = {m["make"]: dict(m["rows"]) for m in data["makers"]}
    text = LENS_DATA.read_text()
    canon_body = text.split("pub static CANON_LENS_TYPES:", 1)[1].split("];", 1)[0]
    canon = {}
    for key, label in re.findall(r'\(\s*(-?\d+)\s*,\s*("(?:\\.|[^"\\])*")\s*\)', canon_body):
        canon[key] = json.loads(label)
    expected_canon = {k: v for k, v in source["Canon"].items() if "." not in k}
    if canon != expected_canon:
        raise ValueError("Canon runtime base registry drifted from selected source")

    pentax_body = text.split("pub static PENTAX_LENS_TYPES:", 1)[1].split("];", 1)[0]
    pentax = {}
    for major, minor, label in re.findall(
        r'\(\s*(\d+)\s*,\s*(\d+)\s*,\s*("(?:\\.|[^"\\])*")\s*,?\s*\)',
        pentax_body,
    ):
        pentax[f"{major} {minor}"] = json.loads(label)
    expected_pentax = {k: v for k, v in source["Pentax"].items() if "." not in k}
    if pentax != expected_pentax:
        missing = sorted(expected_pentax.keys() - pentax.keys())
        changed = sorted(k for k in pentax.keys() & expected_pentax.keys()
                         if pentax[k] != expected_pentax[k])
        extra = sorted(pentax.keys() - expected_pentax.keys())
        raise ValueError(f"Pentax runtime base registry drifted: missing={missing[:5]} "
                         f"changed={changed[:5]} extra={extra[:5]}")


def render(data: dict) -> str:
    parts = [
        "//! XMP::PrintLensID selected maker lookup maps. DO NOT EDIT BY HAND.\n",
        "//! Regenerate with `tools/exiftool-tables/gen_xmp_lens_maps.py`.\n",
        "//! The selected ExifTool version comes from `.exiftool-version`.\n",
        "//! `OTHER` CODE hooks and `Notes` metadata are recorded by the exporter,\n",
        "//! never converted into a fabricated lens string.\n\n",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n",
        "pub enum XmpLensMaker { Canon, Nikon, Pentax, Sony, Sigma, Samsung, Leica }\n\n",
        "pub fn rows(maker: XmpLensMaker) -> &'static [(&'static str, &'static str)] {\n",
        "    match maker {\n",
    ]
    for m in data["makers"]:
        parts.append(f"        XmpLensMaker::{m['make']} => &{m['make'].upper()}_ROWS,\n")
    parts += ["    }\n", "}\n\n"]
    for m in data["makers"]:
        name = m["make"].upper()
        parts.append(f"/// {m['module']}::{m['table']}: {len(m['rows'])} literal "
                     f"rows ({m['base_count']} base, {m['fractional_count']} fractional).\n")
        parts.append(f"/// Source module version: "
                     f"{data['source']['Image/ExifTool/' + m['module'] + '.pm']['version']}.\n")
        for ex in m["exceptions"]:
            parts.append(f"/// Excluded {ex['key']} ({ex['type']}): {ex['disposition']}.\n")
        parts.append(f"#[rustfmt::skip]\npub static {name}_ROWS: [(&str, &str); {len(m['rows'])}] = [\n")
        for key, value in m["rows"]:
            parts.append(f"    ({rust_string(key)}, {rust_string(value)}),\n")
        parts.append("];\n\n")
    result = "".join(parts)
    formatted = subprocess.run(
        [os.environ.get("RUSTFMT", "rustfmt"), "--edition", "2024",
         "--config-path", str(ROOT / "rustfmt.toml"), "--emit", "stdout"],
        input=result, text=True, capture_output=True, check=True,
    )
    return formatted.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exiftool-dir", type=Path, required=True)
    parser.add_argument("--perl", default=os.environ.get("OXIDEX_PERL", "perl"))
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = selected_source(args.perl, args.exiftool_dir.resolve())
    verify_existing(args.perl, args.exiftool_dir.resolve(), data)
    target = args.out.resolve()
    if target != OUT:
        raise ValueError("generated Rust target must be src/composite/xmp_lens_maps.rs")
    result = render(data)
    if args.check:
        if not target.is_file() or target.read_text() != result:
            raise ValueError(f"generated XMP lens maps differ: {target}")
    else:
        target.write_text(result)
    print("XMP lens maps current: " + ", ".join(
        f"{m['make']}={len(m['rows'])}" for m in data["makers"]), file=sys.stderr)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, subprocess.CalledProcessError) as exc:
        raise SystemExit(str(exc)) from exc
