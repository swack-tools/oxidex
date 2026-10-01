#!/usr/bin/env python3
"""Check all selected PLUS rows and a bounded native XMP read matrix.

The optional --oxidex compares the current binary to pinned ExifTool. Without
it, this records native expectations for a source-ready change awaiting Cargo.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import gen_plus_media_matrix

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import instrument  # noqa: E402
from exiftool_oracle import Oracle  # noqa: E402

PLUS_URI = "http://ns.useplus.org/ldf/xmp/1.0/"

EDGE_CASES = [
    "2AAA",
    "2aaa",
    "|plus|v0100|u001|1iaa2bft|",
    "|PLUS|V0100|U001|ZZ2BFT|",
    "|PLUS|V0100|U001|1IAA|2BFT|",
    "|PLUS|V0100|U001|1IAF1UNF|",
    "|PLUS|fooV0001|abcU0009xyz|2AAA|",
    "|PLUS|BAD|BAD|2AAA|",
    "oops|plus|v0100|u001|2AAA|",
    "|PLUS|V0100|U000|2AAA|",
    "|PLUS|V0100|U999999999999999999999999999|2AAA|",
    "|PLUS|V0100|U001|Z|2AAA|x3PTZ|",
    "|PLUS|V0100|U001|1IAAZZZZ|",
]


def read_tag(command: list[str], path: Path) -> str | None:
    completed = subprocess.run([*command, str(path)], check=True, text=True,
                               capture_output=True)
    data = json.loads(completed.stdout)
    return data[0].get("XMP-plus:MediaSummaryCode")


def read_foreign(command: list[str], path: Path) -> dict:
    completed = subprocess.run([*command, str(path)], check=True, text=True,
                               capture_output=True)
    data = json.loads(completed.stdout)[0]
    return {key: value for key, value in data.items() if key.endswith(":MediaSummaryCode")}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exiftool-dir", type=Path, required=True)
    parser.add_argument("--perl", required=True)
    parser.add_argument("--oxidex", type=Path)
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()
    git = instrument.git_state(ROOT)
    dirty_override = instrument.refuse_if_dirty(git, "check_plus_media_matrix.py")
    binary = instrument.resolve_binary(str(args.oxidex)) if args.oxidex else None
    tree = args.exiftool_dir.resolve()
    output = subprocess.check_output(
        [args.perl, str(ROOT / "tools/exiftool-tables/export_plus_media_matrix.pl"),
         "--exiftool-dir", str(tree)], cwd=ROOT,
    )
    data = json.loads(output)
    generated = gen_plus_media_matrix.render(data)
    if gen_plus_media_matrix.OUT.read_text() != generated:
        raise SystemExit("generated PLUS lookup does not match all selected Perl rows")
    rows = data["rows"]
    positions = [0, 1, 10, 100, 500, 1000, 1500, 2000, len(rows) - 1]
    cases = EDGE_CASES + [f"|PLUS|V0100|U001|{rows[index][0]}|" for index in positions]
    oracle = Oracle([args.perl, f"-I{tree / 'lib'}", str(tree / "exiftool"),
                     "-config", ""], data["exiftool_version"],
                    (ROOT / ".exiftool-version").read_text().strip(),
                    "explicit pinned source diagnostic", args.perl, [])
    oracle.check_container_support(tree / "t/images/OOXML.docx")
    instrument.print_header(tool="check_plus_media_matrix.py", git=git,
                            binary=binary, dirty_overridden=dirty_override,
                            oracle=oracle,
                            extra=[f"native fixture cases: {len(cases)} plus foreign namespace"])
    native_cmd = oracle.command(["-G1", "-a", "-s", "-j", "-MediaSummaryCode"])
    oxidex_cmd = [str(args.oxidex.resolve()), "-G1", "-a", "-s", "-j"] if args.oxidex else None
    results = []
    foreign_value = "|PLUS|V0100|U001|2AAA|"
    with tempfile.TemporaryDirectory(prefix="oxidex-plus-matrix-") as directory:
        for index, value in enumerate(cases):
            path = Path(directory) / f"case-{index:02}.xmp"
            path.write_text(
                f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
                f'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" '
                f'xmlns:plus="{PLUS_URI}"><rdf:Description '
                f'rdf:about="" plus:MediaSummaryCode="{value}"/>'
                f'</rdf:RDF></x:xmpmeta>')
            native = read_tag(native_cmd, path)
            if native is None:
                raise SystemExit(f"native PLUS tag missing for case {index}")
            candidate = read_tag(oxidex_cmd, path) if oxidex_cmd else None
            if oxidex_cmd and candidate != native:
                raise SystemExit(f"case {index} mismatch: native={native!r}, oxidex={candidate!r}")
            results.append({"case": index, "input": value, "native": native,
                            "oxidex": candidate})
        foreign_path = Path(directory) / "foreign.xmp"
        foreign_path.write_text(
            '<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
            '<rdf:Description xmlns:plus="https://example.test/foreign/">'
            f'<plus:MediaSummaryCode>{foreign_value}</plus:MediaSummaryCode>'
            '</rdf:Description></rdf:RDF>')
        native_foreign = read_foreign(native_cmd[:-1], foreign_path)
        if native_foreign != {"XMP-tmp0:MediaSummaryCode": foreign_value}:
            raise SystemExit(f"unexpected native foreign namespace routing: {native_foreign}")
        candidate_foreign = (read_foreign(oxidex_cmd, foreign_path)
                             if oxidex_cmd else None)
        if oxidex_cmd and candidate_foreign != native_foreign:
            raise SystemExit(f"foreign namespace mismatch: {candidate_foreign!r}")
    receipt = {
        "instrument": "check_plus_media_matrix.py",
        "git_commit": git.commit,
        "dirty_files": git.dirty_files,
        "dirty_override": dirty_override,
        "oxidex_sha256": (hashlib.sha256(binary.path.read_bytes()).hexdigest()
                          if binary else None),
        "exiftool_version": data["exiftool_version"],
        "plus_source_sha256": data["source_sha256"],
        "source_rows_compared": len(rows),
        "native_cases": len(cases),
        "foreign_native": native_foreign,
        "foreign_oxidex": candidate_foreign,
        "oxidex_binary": str(args.oxidex.resolve()) if args.oxidex else None,
        "results": results,
    }
    if args.json_out:
        args.json_out.write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"source rows {len(rows)}; native cases {len(cases)}; "
          f"oxidex {'matched' if oxidex_cmd else 'deferred'}")


if __name__ == "__main__":
    main()
