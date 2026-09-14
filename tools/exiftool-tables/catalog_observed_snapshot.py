#!/usr/bin/env python3
"""Publish or validate an authenticated historical catalog-observation receipt.

The source/declaration join is deterministic and rebuilt in CI.  Native
read-back receipts are different evidence: they bind a particular binary,
fixture corpus and source/runtime commit.  This tool preserves their complete
joined row accounting in a separate Pages artifact and refuses to present a
historical receipt as a fresh observation of a later source ledger.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import tempfile


SCHEMA = "oxidex_catalog_observed_snapshot_v1"
JOIN_SCHEMA = "oxidex_catalog_hydrated_join_v2"
OBSERVATION_FIELDS = {"observed_read", "observed_write", "observed_write_group1_names", "alternate_context_write_group1_names"}
COMMIT = re.compile(r"[0-9a-f]{40}")


def canonical_hash(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()


def read(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} is not a JSON object")
    return value


def entries_by_identity(join: dict, label: str) -> dict[tuple[str, str, int], dict]:
    if join.get("schema") != JOIN_SCHEMA:
        raise ValueError(f"{label} has an unsupported join schema")
    rows = join.get("entries")
    if not isinstance(rows, list) or not isinstance(join.get("counts"), dict):
        raise ValueError(f"{label} lacks complete entry accounting")
    indexed = {}
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("identity"), dict):
            raise ValueError(f"{label} has a malformed entry")
        identity = row["identity"]
        table, raw_key, variant = identity.get("table"), identity.get("raw_key"), identity.get("variant_index")
        if not isinstance(table, str) or not isinstance(raw_key, str) or type(variant) is not int:
            raise ValueError(f"{label} has a malformed source coordinate")
        key = (table, raw_key, variant)
        if key in indexed:
            raise ValueError(f"{label} duplicates a source coordinate")
        indexed[key] = row
    if join["counts"].get("joined_records") != len(indexed):
        raise ValueError(f"{label} does not conserve its source denominator")
    return indexed


def stable_row(row: dict) -> dict:
    return {key: value for key, value in row.items() if key not in OBSERVATION_FIELDS}


def source_table_observations(observed: dict) -> dict:
    rows = entries_by_identity(observed, "observed join")
    tables: dict[str, dict] = {}
    for (table, _, _), row in rows.items():
        bucket = tables.setdefault(table, {"source_entries": 0, "observed_read": Counter(), "observed_write": Counter()})
        bucket["source_entries"] += 1
        bucket["observed_read"][row.get("observed_read")] += 1
        bucket["observed_write"][row.get("observed_write")] += 1
    return {table: {"source_entries": value["source_entries"],
                    "observed_read": dict(sorted(value["observed_read"].items())),
                    "observed_write": dict(sorted(value["observed_write"].items()))}
            for table, value in sorted(tables.items())}


def validate_pair(source: dict, observed: dict) -> None:
    source_rows = entries_by_identity(source, "source join")
    observed_rows = entries_by_identity(observed, "observed join")
    if source.get("inputs", {}).get("exiftool_version") != observed.get("inputs", {}).get("exiftool_version"):
        raise ValueError("observed receipt ExifTool version differs from its source join")
    if set(source_rows) != set(observed_rows):
        raise ValueError("observed receipt source coordinates differ from its source join")
    for identity in source_rows:
        if stable_row(source_rows[identity]) != stable_row(observed_rows[identity]):
            raise ValueError("observed receipt changes source/declaration accounting")
    observed_counts = observed.get("counts", {}).get("observed_read")
    if not isinstance(observed_counts, dict):
        raise ValueError("observed receipt has no observed-read accounting")
    if observed_counts != dict(sorted(Counter(row.get("observed_read") for row in observed_rows.values()).items())):
        raise ValueError("observed receipt read counts do not match its rows")
    if "observed_write" in observed.get("counts", {}):
        write_counts = observed["counts"]["observed_write"]
        if not isinstance(write_counts, dict) or write_counts != dict(sorted(Counter(row.get("observed_write") for row in observed_rows.values()).items())):
            raise ValueError("observed receipt write counts do not match its rows")


def make_snapshot(source: dict, observed: dict, source_sha256: str, source_commit: str,
                  runtime_commit: str, instrument: str) -> dict:
    validate_pair(source, observed)
    if not all(isinstance(value, str) and COMMIT.fullmatch(value) for value in (source_commit, runtime_commit)) or not isinstance(instrument, str) or not instrument:
        raise ValueError("source commit, runtime commit, and instrument are required")
    return {
        "schema": SCHEMA,
        "scope": "historical authenticated native observations; not evidence of current runtime after either recorded commit changes",
        "source_join": {
            "sha256": source_sha256,
            "exiftool_version": source["inputs"]["exiftool_version"],
            "source_commit": source_commit,
        },
        "runtime": {"commit": runtime_commit, "instrument": instrument},
        "source_table_observations": source_table_observations(observed),
        "observed_join": observed,
    }


def validate_snapshot(source: dict, source_bytes: bytes, snapshot: dict) -> None:
    if snapshot.get("schema") != SCHEMA or not isinstance(snapshot.get("source_join"), dict):
        raise ValueError("published observed receipt schema is malformed")
    binding = snapshot["source_join"]
    if binding.get("sha256") != hashlib.sha256(source_bytes).hexdigest():
        raise ValueError("published observed receipt is bound to a different source join")
    if binding.get("exiftool_version") != source.get("inputs", {}).get("exiftool_version"):
        raise ValueError("published observed receipt pin differs from its source join")
    runtime = snapshot.get("runtime")
    if (not isinstance(runtime, dict) or not isinstance(runtime.get("commit"), str)
            or not COMMIT.fullmatch(runtime["commit"]) or not isinstance(runtime.get("instrument"), str)
            or not runtime["instrument"] or not isinstance(binding.get("source_commit"), str)
            or not COMMIT.fullmatch(binding["source_commit"])):
        raise ValueError("published observed receipt lacks runtime commit or instrument")
    observed = snapshot.get("observed_join")
    if not isinstance(observed, dict):
        raise ValueError("published observed receipt lacks joined observations")
    validate_pair(source, observed)
    if snapshot.get("source_table_observations") != source_table_observations(observed):
        raise ValueError("published observed receipt table counts do not match its rows")


def render_report(snapshot: dict) -> str:
    observed = snapshot["observed_join"]
    counts = observed["counts"]
    return "\n".join([
        "# Authenticated catalog observations",
        "",
        "This is a historical native receipt. It does not assert that a later source or runtime has the same observations.",
        "",
        f"- Source commit: `{snapshot['source_join']['source_commit']}`",
        f"- Runtime commit: `{snapshot['runtime']['commit']}`",
        f"- Instrument: `{snapshot['runtime']['instrument']}`",
        f"- Source join SHA-256: `{snapshot['source_join']['sha256']}`",
        f"- Source denominator: `{counts['joined_records']}`",
        f"- Observed reads: `{json.dumps(counts.get('observed_read', {}), sort_keys=True)}`",
        f"- Observed writes: `{json.dumps(counts.get('observed_write', {}), sort_keys=True)}`",
        f"- Source tables with observations: `{len(snapshot['source_table_observations'])}`",
        "",
    ])


def staged_write(documents: list[tuple[Path, str]]) -> None:
    staged = []
    try:
        for destination, body in documents:
            destination.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=destination.parent,
                                             prefix=".catalog-observed-", delete=False) as handle:
                temporary = Path(handle.name)
                handle.write(body)
            staged.append((temporary, destination))
        for temporary, destination in staged:
            temporary.replace(destination)
    finally:
        for temporary, _ in staged:
            temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-join", required=True, type=Path)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--observed-join", type=Path)
    parser.add_argument("--source-commit")
    parser.add_argument("--runtime-commit")
    parser.add_argument("--instrument")
    parser.add_argument("--report", type=Path)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--replace", action="store_true")
    action.add_argument("--check", action="store_true")
    action.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    source_bytes = args.source_join.read_bytes()
    source = json.loads(source_bytes)
    if args.verify:
        if any(value is not None for value in (args.observed_join, args.source_commit, args.runtime_commit, args.instrument, args.report)):
            raise ValueError("--verify accepts only source join and snapshot")
        validate_snapshot(source, source_bytes, read(args.snapshot))
        return 0
    if not all(value is not None for value in (args.observed_join, args.source_commit, args.runtime_commit, args.instrument, args.report)):
        raise ValueError("publication requires observed join, commits, instrument, and report")
    observed = read(args.observed_join)
    snapshot = make_snapshot(source, observed, hashlib.sha256(source_bytes).hexdigest(),
                             args.source_commit, args.runtime_commit, args.instrument)
    rendered_snapshot = json.dumps(snapshot, indent=2, sort_keys=True) + "\n"
    rendered_report = render_report(snapshot)
    if args.check:
        if not args.snapshot.is_file() or not args.report.is_file() or args.snapshot.read_text(encoding="utf-8") != rendered_snapshot or args.report.read_text(encoding="utf-8") != rendered_report:
            raise ValueError("published observed receipt is stale")
        return 0
    input_paths = {args.source_join.resolve(), args.observed_join.resolve()}
    if args.snapshot.resolve() in input_paths or args.report.resolve() in input_paths or args.snapshot.resolve() == args.report.resolve():
        raise ValueError("publication output aliases an input or companion report")
    staged_write([(args.snapshot, rendered_snapshot), (args.report, rendered_report)])
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        raise SystemExit(f"catalog observed receipt refused: {exc}")
