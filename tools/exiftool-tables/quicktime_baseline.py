#!/usr/bin/env python3
"""Reproduce the bounded ItemList baseline; never infer global tag coverage."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "tests/fixtures/quicktime/source_family_baseline"
sys.path.insert(0, str(ROOT / "scripts"))
import exiftool_oracle
import instrument


def atom(key: bytes, payload: bytes) -> bytes:
    if len(key) != 4:
        raise ValueError("atom key must have four bytes")
    return struct.pack(">I", len(payload) + 8) + key + payload


def fixture(key: bytes, flags: int, value: bytes) -> bytes:
    item = atom(key, atom(b"data", struct.pack(">II", flags, 0) + value))
    handler = atom(b"hdlr", b"\0" * 8 + b"mdirappl" + b"\0" * 9)
    meta = atom(b"meta", b"\0" * 4 + handler + atom(b"ilst", item))
    return atom(b"ftyp", b"M4A \0\0\0\0M4A isom") + atom(b"moov", atom(b"udta", meta))


def cases() -> dict[str, bytes]:
    return {
        "text.m4a": fixture(b"\xa9nam", 1, b"Generic source reader"),
        "enum.m4a": fixture(b"cpil", 21, b"\1"),
        "u16.m4a": fixture(b"tmpo", 0, struct.pack(">H", 257)),
        "u64.m4a": fixture(b"plID", 0, struct.pack(">Q", 4294967297)),
        "u64-max.m4a": fixture(b"plID", 0, struct.pack(">Q", 2**64 - 1)),
    }


def projection(value):
    if isinstance(value, list) and len(value) == 1:
        value = value[0]
    if not isinstance(value, dict):
        raise ValueError("expected one metadata object")
    return {key: val for key, val in value.items() if key.startswith("ItemList:")}


def verify_fixtures() -> None:
    for name, data in cases().items():
        if (FIXTURES / name).read_bytes() != data:
            raise ValueError(f"fixture drift: {name}")


def check_reading_baseline(report, recorded):
    if report["pin"] != recorded["pin"] or report["fixtures"] != recorded["fixtures"]:
        raise ValueError("reading observations differ from the recorded baseline")


def compare(tree: Path, output: Path) -> dict:
    """Build this checkout and measure the exact compiler-reported executable."""
    verify_fixtures()
    if output.resolve().is_relative_to(ROOT.resolve()):
        raise ValueError("replay output must be outside the worktree")
    pin = (ROOT / ".exiftool-version").read_text().strip()
    oracle = exiftool_oracle.resolve_tree(tree.resolve())
    if not oracle.verified or oracle.version != pin:
        raise ValueError("oracle capability/version must match the repository pin")
    state = instrument.git_state(ROOT)
    overridden = instrument.refuse_if_dirty(state, "quicktime_baseline.py")
    output.mkdir(parents=True, exist_ok=False)
    command = ["cargo", "build", "--bin", "oxidex", "--message-format=json"]
    with (output / "build.stderr").open("w") as err:
        build = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=err)
    (output / "build.jsonl").write_text(build.stdout)
    build.check_returncode()
    binaries = [row["executable"] for line in build.stdout.splitlines()
                if (row := json.loads(line)).get("reason") == "compiler-artifact"
                and row.get("target", {}).get("name") == "oxidex" and row.get("executable")]
    if len(binaries) != 1:
        raise ValueError("compiler did not identify exactly one oxidex binary")
    binary = instrument.resolve_binary(binaries[0])
    instrument.print_header(tool="quicktime_baseline.py", git=state, binary=binary,
                            dirty_overridden=overridden, oracle=oracle,
                            corpus_paths=[FIXTURES], file_count=len(cases()))
    observations = []
    for name, data in sorted(cases().items()):
        path = FIXTURES / name
        native = subprocess.run(oracle.command(["-j", "-a", "-G1", "-s", str(path)]),
                                check=True, capture_output=True, text=True)
        actual = subprocess.run([str(binary.path), "-j", str(path)], check=True,
                                capture_output=True, text=True)
        (output / (name + ".native.json")).write_text(native.stdout)
        (output / (name + ".oxidex.json")).write_text(actual.stdout)
        expected, got = projection(json.loads(native.stdout)), projection(json.loads(actual.stdout))
        if len(expected) != 1:
            raise ValueError(f"native fixture degraded: {name}: {expected}")
        observations.append({"fixture": name, "fixture_sha256": hashlib.sha256(data).hexdigest(),
                             "expected": expected, "actual": got, "matched": expected == got})
    if instrument.git_state(ROOT) != state:
        raise ValueError("source state changed during measurement")
    report = {"instrument": "quicktime_baseline.py; pinned -j -a -G1 -s vs fresh oxidex -j; ItemList projection",
              "provenance_status": "replay_verified",
              "instrument_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "pin": pin, "source_commit": state.commit,
              "source_dirty": state.dirty, "source_dirty_files": state.dirty_files,
              "binary_sha256": hashlib.sha256(Path(binary.path).read_bytes()).hexdigest(),
              "fixtures": observations,
              "reading_matched_fixture_projections": sum(row["matched"] for row in observations),
              "writing_observed": None,
              "interpretation": "Five pre-migration behavior fixtures only; neither corpus conformance nor writing coverage."}
    (output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-fixtures", action="store_true")
    parser.add_argument("--exiftool-dir", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check-reading-baseline", type=Path)
    args = parser.parse_args()
    if args.check_fixtures:
        verify_fixtures()
        print("Five fixture byte streams match their behavior definitions.")
    elif args.exiftool_dir and args.out:
        report = compare(args.exiftool_dir, args.out)
        if args.check_reading_baseline:
            check_reading_baseline(report, json.loads(args.check_reading_baseline.read_text()))
        print(f"ItemList fixture projections matched: {report['reading_matched_fixture_projections']}/5")
    else:
        parser.error("use --check-fixtures or --exiftool-dir TREE --out NEW_DIRECTORY")


if __name__ == "__main__":
    main()
