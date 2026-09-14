#!/usr/bin/env python3
"""Replay ItemList parser behaviors against the source-pinned native oracle."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

import quicktime_baseline as baseline


def cases():
    """One fixture per framing, format, or conversion behavior, not per tag."""
    extra = [
        ("media-enum", b"stik", 21, b"\x02"),
        ("unknown-ascii", b"zzzz", 1, b"not a known tag"),
        ("unknown-binary-key", b"\xff\xfe\xfd\xfc", 1, b"not a known tag"),
        ("utf8-alias", b"\xa9nam", 4, b"A\0\0"),
        ("utf16", b"\xa9nam", 2, "Title".encode("utf-16-be")),
        ("utf16-alias", b"\xa9nam", 5, "Title".encode("utf-16-be")),
        ("shiftjis", b"\xa9nam", 3, bytes([0x82, 0xa0])),
        ("text-enum", b"cpil", 1, b"1"),
        ("signed", b"\xa9nam", 21, b"\xff"),
        ("implicit-u64", b"\xa9nam", 22, struct.pack(">Q", 2**64 - 1)),
        ("float", b"\xa9nam", 23, struct.pack(">f", 1.25)),
        ("double", b"\xa9nam", 24, struct.pack(">d", 1.25)),
        ("float-array", b"\xa9nam", 23, struct.pack(">ff", 1.25, 2.5)),
        ("u64-short", b"plID", 0, b"\x12\x34"),
        ("u64-tail", b"plID", 0, struct.pack(">Q", 4294967297) + b"\xff"),
        ("unsigned-over-signed", b"cpil", 21, b"\xff"),
        ("binary", b"\xa9nam", 13, b"\xff\xd8\xff\xd9"),
        ("integer-binary", b"\xa9nam", 21, b"\x01\x02\x03"),
        ("source-string", b"gshh", 0, b"A\0B"),
        ("malformed-u64", b"plID", 0, b"\x01\x02\x03"),
    ]
    return {**baseline.cases(), **{
        name + ".m4a": baseline.fixture(key, flags, value)
        for name, key, flags, value in extra
    }}


def compare(tree: Path, output: Path):
    root = baseline.ROOT
    if output.exists() or output.is_symlink():
        raise ValueError("output already exists; choose a new evidence directory")
    if output.resolve().is_relative_to(root.resolve()):
        raise ValueError("evidence directory must be outside the worktree")
    state = baseline.instrument.git_state(root)
    override = baseline.instrument.refuse_if_dirty(state, "verify_quicktime_reader.py")
    fingerprint = baseline.source_fingerprint(root)
    manifest_path = Path(__file__).parent / "fixtures/quicktime_oracle_sources_13_59.json"
    manifest = json.loads(manifest_path.read_text())
    baseline.verify_oracle_sources(tree, manifest)
    oracle = baseline.exiftool_oracle.resolve_tree(tree.resolve())
    if not oracle.verified or oracle.version != (root / ".exiftool-version").read_text().strip():
        raise ValueError("native oracle capability/version does not match the pin")
    tool_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    helper_hash = hashlib.sha256(Path(baseline.__file__).read_bytes()).hexdigest()
    output.mkdir(parents=True)
    build = subprocess.run(["cargo", "build", "--bin", "oxidex", "--message-format=json"],
                           cwd=root, text=True, capture_output=True)
    (output / "build.jsonl").write_text(build.stdout)
    (output / "build.stderr").write_text(build.stderr)
    build.check_returncode()
    binaries = [row["executable"] for line in build.stdout.splitlines()
                if (row := json.loads(line)).get("reason") == "compiler-artifact"
                and row.get("target", {}).get("name") == "oxidex" and row.get("executable")]
    if len(binaries) != 1:
        raise ValueError("Cargo did not report exactly one oxidex executable")
    binary = baseline.instrument.resolve_binary(binaries[0])
    fixtures = cases()
    baseline.instrument.print_header(
        tool="verify_quicktime_reader.py", git=state, binary=binary, oracle=oracle,
        dirty_overridden=override, corpus_paths=[output], file_count=len(fixtures))
    rows = []
    for name, contents in sorted(fixtures.items()):
        path = output / name
        path.write_bytes(contents)
        for mode, native_flags, oxidex_flags in [
            ("print", [], []), ("no-print-conv", ["-n"], ["--no-print-conv"]),
        ]:
            native_command = oracle.command(["-config", "", "-j", "-a", "-G1", "-s",
                                             *native_flags, str(path)])
            actual_command = [str(binary.path), "-j", "-a", "-G1", *oxidex_flags, str(path)]
            native = subprocess.run(native_command, text=True, capture_output=True, check=True)
            actual = subprocess.run(actual_command, text=True, capture_output=True, check=True)
            prefix = name + "." + mode
            (output / (prefix + ".native.json")).write_text(native.stdout)
            (output / (prefix + ".oxidex.json")).write_text(actual.stdout)
            expected = baseline.projection(json.loads(native.stdout))
            got = baseline.projection(json.loads(actual.stdout))
            expected_count = 0 if name.startswith("unknown-") else 1
            if len(expected) != expected_count:
                raise ValueError(f"native fixture degraded: {name}/{mode}: {expected}")
            rows.append({"fixture": name, "mode": mode,
                         "fixture_sha256": hashlib.sha256(contents).hexdigest(),
                         "expected": expected, "actual": got, "matched": expected == got})
    if baseline.source_fingerprint(root) != fingerprint:
        raise ValueError("source changed during the comparison")
    baseline.verify_oracle_sources(tree, manifest)
    report = {
        "instrument": "verify_quicktime_reader.py; native and fresh oxidex -j -a -G1; ItemList projection",
        "instrument_sha256": tool_hash, "baseline_helper_sha256": helper_hash,
        "source_commit": state.commit, "source_dirty": state.dirty,
        "source_fingerprint": fingerprint, "oracle_commit": manifest["commit"],
        "oracle_manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        "binary_sha256": hashlib.sha256(Path(binary.path).read_bytes()).hexdigest(),
        "pin": oracle.version, "fixture_count": len(fixtures), "observations": rows,
        "matched_observations": sum(row["matched"] for row in rows),
        "writing_observed": None,
        "scope": "default-locale ItemList behavior fixtures; not corpus coverage or every source row",
    }
    (output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exiftool-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = compare(args.exiftool_dir, args.out)
    total = len(report["observations"])
    print(f"ItemList observations matched: {report['matched_observations']}/{total}")
    if report["matched_observations"] != total:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
