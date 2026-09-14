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
import quicktime_atom_tables as quicktime_selector
import quicktime_generated_specs as quicktime_specs

EVIDENCE_SCHEMA = "oxidex_quicktime_generated_read_evidence_v1"


def authenticated_artifacts(source: Path, ledger: Path, capabilities: Path, rust: Path) -> tuple[dict, dict]:
    """Replay every supplied generated input before it can bind runtime evidence."""
    raw, emitted = source.read_bytes(), rust.read_text()
    bounded, supplied_ledger = json.loads(raw), json.loads(ledger.read_text())
    supplied_capabilities = json.loads(capabilities.read_text())
    if (quicktime_specs.compile_document(bounded) != supplied_ledger
            or quicktime_selector.report(raw) != supplied_capabilities
            or not rust_matches(quicktime_specs.render_rust(supplied_ledger), emitted)):
        raise ValueError("generated QuickTime artifacts do not replay from bounded source")
    return ({"source_sha256": hashlib.sha256(raw).hexdigest(), "ledger_sha256": hashlib.sha256(ledger.read_bytes()).hexdigest(),
             "capabilities_sha256": hashlib.sha256(capabilities.read_bytes()).hexdigest(), "rust_sha256": hashlib.sha256(emitted.encode()).hexdigest()}, supplied_ledger)


def rust_matches(expected: str, supplied: str) -> bool:
    # Keep the same formatting-aware replay used by the catalog join.
    from join_catalog_hydrated import quicktime_rust_matches
    return quicktime_rust_matches(expected, supplied)


def fixture_raw_key(contents: bytes) -> str:
    """Read the one ItemList key from our constructed atom fixture itself."""
    def atoms(data):
        offset = 0
        while offset < len(data):
            if len(data) - offset < 8:
                raise ValueError("truncated fixture atom")
            size = int.from_bytes(data[offset:offset + 4], "big")
            if size < 8 or size > len(data) - offset:
                raise ValueError("invalid fixture atom size")
            yield data[offset + 4:offset + 8], data[offset + 8:offset + size]
            offset += size
    data = contents
    for key in (b"moov", b"udta", b"meta", b"ilst"):
        matches = [payload for name, payload in atoms(data) if name == key]
        if len(matches) != 1:
            raise ValueError("fixture must contain exactly one ItemList path")
        data = matches[0][4:] if key == b"meta" else matches[0]
    items = list(atoms(data))
    if len(items) != 1:
        raise ValueError("fixture must contain exactly one ItemList item")
    return items[0][0].decode("latin1")


def transcript_projection(row: dict, transcript: str, claim: str) -> dict:
    """Decode a hash-bound JSON transcript and verify its projected claim."""
    raw = row.get(transcript)
    digest = row.get(transcript + "_sha256")
    if not isinstance(raw, str) or not isinstance(digest, str):
        raise ValueError("read evidence transcript or hash is missing or malformed")
    if hashlib.sha256(raw.encode()).hexdigest() != digest:
        raise ValueError("read evidence transcript hash differs")
    try:
        projection = baseline.projection(json.loads(raw))
    except (TypeError, json.JSONDecodeError, ValueError) as error:
        raise ValueError("read evidence transcript is not a projected metadata JSON document") from error
    if row.get(claim) != projection:
        raise ValueError("read evidence projection claim differs from transcript")
    return projection


def observation_evidence(rows: list, specs: list) -> tuple[list, list, str]:
    """Recompute observed identities from fixture bytes and bound transcripts."""
    fixtures = cases()
    expected_pairs = {(name, mode) for name in fixtures for mode in ("print", "no-print-conv")}
    seen, occurrences = set(), []
    by_key = {}
    for spec in specs:
        key = (spec["source_identity"]["raw_key"], spec["name"])
        if key in by_key:
            raise ValueError("generated source identity is ambiguous for fixture")
        by_key[key] = spec["source_identity"]
    for row in rows:
        pair = (row.get("fixture"), row.get("mode"))
        if pair not in expected_pairs or pair in seen:
            raise ValueError("read evidence fixture/mode is unknown or duplicated")
        seen.add(pair)
        contents = fixtures[pair[0]]
        if row.get("fixture_sha256") != hashlib.sha256(contents).hexdigest():
            raise ValueError("read evidence fixture bytes differ")
        expected = transcript_projection(row, "native_json", "expected")
        actual = transcript_projection(row, "oxidex_json", "actual")
        matched = expected == actual
        if row.get("matched") is not matched:
            raise ValueError("read evidence matched flag disagrees with output")
        if not matched:
            continue
        for emitted in actual:
            if not emitted.startswith("ItemList:"):
                raise ValueError("read evidence projection contains another group")
            name = emitted.removeprefix("ItemList:")
            identity = by_key.get((fixture_raw_key(contents), name))
            if identity is None:
                raise ValueError("matched emitted tag lacks an exact generated source identity")
            occurrences.append({"fixture": pair[0], "mode": pair[1], "source_identity": identity,
                                "group1": "ItemList", "tag_name": name, "matched": True})
    if seen != expected_pairs:
        raise ValueError("read evidence fixture/mode set is incomplete")
    unique = {json.dumps(row["source_identity"], sort_keys=True): row for row in occurrences}
    manifest = {name: hashlib.sha256(data).hexdigest() for name, data in fixtures.items()}
    digest = hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()
    return occurrences, list(unique.values()), digest


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


def compare(tree: Path, output: Path, artifacts: tuple[Path, Path, Path, Path]):
    root = baseline.ROOT
    if output.exists() or output.is_symlink():
        raise ValueError("output already exists; choose a new evidence directory")
    if output.resolve().is_relative_to(root.resolve()):
        raise ValueError("evidence directory must be outside the worktree")
    state = baseline.instrument.git_state(root)
    if state.dirty:
        raise ValueError("authenticated read evidence requires a clean source checkout")
    override = False
    input_digests, ledger = authenticated_artifacts(*artifacts)
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
                         "native_json": native.stdout,
                         "native_json_sha256": hashlib.sha256(native.stdout.encode()).hexdigest(),
                         "oxidex_json": actual.stdout,
                         "oxidex_json_sha256": hashlib.sha256(actual.stdout.encode()).hexdigest(),
                         "expected": expected, "actual": got, "matched": expected == got})
    if baseline.source_fingerprint(root) != fingerprint:
        raise ValueError("source changed during the comparison")
    baseline.verify_oracle_sources(tree, manifest)
    occurrences, identities, fixture_digest = observation_evidence(rows, ledger["specs"])
    report = {
        "schema": EVIDENCE_SCHEMA,
        "instrument": "verify_quicktime_reader.py; native and fresh oxidex -j -a -G1; ItemList projection",
        "instrument_sha256": tool_hash, "baseline_helper_sha256": helper_hash,
        "source_commit": state.commit, "source_dirty": state.dirty,
        "source_fingerprint": fingerprint, "oracle_commit": manifest["commit"],
        "oracle_manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        "binary_sha256": hashlib.sha256(Path(binary.path).read_bytes()).hexdigest(),
        "pin": oracle.version, "fixture_count": len(fixtures), "observations": rows,
        "inputs": dict(sorted(input_digests.items())),
        "producer": {"source_commit": state.commit, "source_dirty": False, "source_fingerprint": fingerprint,
                     "runtime_artifact_sha256": hashlib.sha256(Path(binary.path).read_bytes()).hexdigest(),
                     "fixture_manifest_sha256": fixture_digest, "pin": oracle.version},
        "matched_occurrences": occurrences, "observed_identities": identities,
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
    parser.add_argument("--quicktime-bounded-source", type=Path, required=True)
    parser.add_argument("--quicktime-itemlist-ledger", type=Path, required=True)
    parser.add_argument("--quicktime-source-capabilities", type=Path, required=True)
    parser.add_argument("--quicktime-itemlist-rust", type=Path, required=True)
    args = parser.parse_args()
    report = compare(args.exiftool_dir, args.out, (args.quicktime_bounded_source, args.quicktime_itemlist_ledger,
                                                   args.quicktime_source_capabilities, args.quicktime_itemlist_rust))
    total = len(report["observations"])
    print(f"ItemList observations matched: {report['matched_observations']}/{total}")
    if report["matched_observations"] != total:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
