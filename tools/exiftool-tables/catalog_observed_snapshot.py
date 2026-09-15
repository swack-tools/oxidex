#!/usr/bin/env python3
"""Validate historical catalog-observation snapshots made by the authenticated join.

This module deliberately does not accept an arbitrary observed join as proof.
`join_catalog_hydrated.py` creates a snapshot only after its native receipt
validators have replayed all supplied evidence.  The resulting snapshot embeds
the exact historical source ledger and a receipt manifest so future CI can
check integrity without falsely asserting that a changed runtime was observed.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re

SCHEMA = "oxidex_catalog_observed_snapshot_v2"
JOIN_SCHEMA = "oxidex_catalog_hydrated_join_v2"
OBSERVATION_FIELDS = {"observed_read", "observed_write", "observed_write_group1_names", "alternate_context_write_group1_names"}
COMMIT = re.compile(r"[0-9a-f]{40}")


def canonical_hash(value: object) -> str:
    # Match join_catalog_hydrated's receipt bindings, including non-ASCII
    # FourCCs and values. UTF-8 and escaped JSON represent the same data but
    # produce different hashes, so this encoding is part of the contract.
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")).hexdigest()


def read(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} is not a JSON object")
    return value


def entries_by_identity(join: dict, label: str) -> dict[tuple[str, str, int], dict]:
    if join.get("schema") != JOIN_SCHEMA:
        raise ValueError(f"{label} has an unsupported join schema")
    rows, counts = join.get("entries"), join.get("counts")
    if not isinstance(rows, list) or not isinstance(counts, dict):
        raise ValueError(f"{label} lacks complete entry accounting")
    indexed = {}
    for row in rows:
        identity = row.get("identity") if isinstance(row, dict) else None
        if not isinstance(identity, dict):
            raise ValueError(f"{label} has a malformed entry")
        table, raw_key, variant = identity.get("table"), identity.get("raw_key"), identity.get("variant_index")
        if not isinstance(table, str) or not isinstance(raw_key, str) or type(variant) is not int:
            raise ValueError(f"{label} has a malformed source coordinate")
        key = (table, raw_key, variant)
        if key in indexed:
            raise ValueError(f"{label} duplicates a source coordinate")
        indexed[key] = row
    if counts.get("joined_records") != len(indexed):
        raise ValueError(f"{label} does not conserve its source denominator")
    return indexed


def stable_row(row: dict) -> dict:
    return {key: value for key, value in row.items() if key not in OBSERVATION_FIELDS}


def source_table_observations(observed: dict) -> dict:
    tables: dict[str, dict] = {}
    for (table, _, _), row in entries_by_identity(observed, "observed join").items():
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
    for axis in ("observed_read", "observed_write"):
        actual = dict(sorted(Counter(row.get(axis) for row in observed_rows.values()).items()))
        recorded = observed.get("counts", {}).get(axis)
        if axis == "observed_write" and recorded is None:
            continue
        if recorded != actual:
            raise ValueError(f"observed receipt {axis} counts do not match its rows")


def evidence_manifest(observed: dict, evidence: dict[str, dict]) -> dict:
    if not evidence:
        raise ValueError("historical receipt requires authenticated native evidence")
    inputs = observed.get("inputs")
    if not isinstance(inputs, dict):
        raise ValueError("observed join lacks evidence input bindings")
    manifest = {}
    commits, runtime_manifests = set(), set()
    for axis, receipt in sorted(evidence.items()):
        if not isinstance(receipt, dict) or not isinstance(receipt.get("producer"), dict):
            raise ValueError(f"{axis} evidence lacks an authenticated producer")
        producer = receipt["producer"]
        commit = producer.get("source_commit")
        runtime_manifest = producer.get("runtime_input_manifest_sha256")
        if not isinstance(commit, str) or not COMMIT.fullmatch(commit) or not isinstance(runtime_manifest, str) or not re.fullmatch(r"[0-9a-f]{64}", runtime_manifest):
            raise ValueError(f"{axis} evidence producer binding is malformed")
        joined = inputs.get(axis)
        expected = {"sha256": canonical_hash(receipt), "producer": producer}
        if joined != expected:
            raise ValueError(f"{axis} evidence is not bound to the observed join")
        commits.add(commit)
        runtime_manifests.add(runtime_manifest)
        manifest[axis] = {"schema": receipt.get("schema"), **expected}
    if len(commits) != 1 or len(runtime_manifests) != 1:
        raise ValueError("historical receipt native evidence does not share one runtime")
    return {"source_commit": next(iter(commits)), "runtime_input_manifest_sha256": next(iter(runtime_manifests)), "axes": manifest}


def make_authenticated_snapshot(source: dict, observed: dict, evidence: dict[str, dict]) -> dict:
    validate_pair(source, observed)
    manifest = evidence_manifest(observed, evidence)
    return {
        "schema": SCHEMA,
        "scope": "historical authenticated native observations; current applicability is reported separately and never inferred",
        "source_join_sha256": canonical_hash(source),
        "source_join": source,
        "native_evidence": manifest,
        "source_table_observations": source_table_observations(observed),
        "observed_join": observed,
    }


def validate_snapshot(snapshot: dict) -> None:
    if snapshot.get("schema") != SCHEMA:
        raise ValueError("published observed receipt schema is malformed")
    source, observed, manifest = snapshot.get("source_join"), snapshot.get("observed_join"), snapshot.get("native_evidence")
    if not isinstance(source, dict) or not isinstance(observed, dict) or not isinstance(manifest, dict):
        raise ValueError("published observed receipt lacks embedded proof")
    if snapshot.get("source_join_sha256") != canonical_hash(source):
        raise ValueError("published observed receipt source join hash differs")
    validate_pair(source, observed)
    if snapshot.get("source_table_observations") != source_table_observations(observed):
        raise ValueError("published observed receipt table counts do not match its rows")
    if not COMMIT.fullmatch(str(manifest.get("source_commit", ""))) or not re.fullmatch(r"[0-9a-f]{64}", str(manifest.get("runtime_input_manifest_sha256", ""))):
        raise ValueError("published observed receipt native provenance is malformed")
    axes = manifest.get("axes")
    if not isinstance(axes, dict) or not axes:
        raise ValueError("published observed receipt has no native evidence manifest")
    inputs = observed.get("inputs", {})
    for axis, receipt in axes.items():
        if not isinstance(receipt, dict) or receipt.get("schema") is None or not isinstance(receipt.get("producer"), dict):
            raise ValueError("published observed receipt evidence manifest is malformed")
        if inputs.get(axis) != {"sha256": receipt.get("sha256"), "producer": receipt["producer"]}:
            raise ValueError("published observed receipt evidence binding differs")
        if receipt["producer"].get("source_commit") != manifest["source_commit"] or receipt["producer"].get("runtime_input_manifest_sha256") != manifest["runtime_input_manifest_sha256"]:
            raise ValueError("published observed receipt mixes native runtimes")


def render_report(snapshot: dict, current_source: dict | None = None) -> str:
    observed = snapshot["observed_join"]
    current = "not compared"
    if current_source is not None:
        current = "matches historical source join" if canonical_hash(current_source) == snapshot["source_join_sha256"] else "differs from historical source join; observations remain historical"
    return "\n".join([
        "# Authenticated catalog observations", "",
        "This is a historical native receipt. It does not assert that a later source or runtime has the same observations.", "",
        f"[Download the authenticated observation snapshot](/measurements/catalog-hydrated-observed-{observed['inputs']['exiftool_version']}.json).",
        "[Compare source classifications and remaining work](goal-checkpoint-20260914.md).", "",
        f"- Observed runtime commit: `{snapshot['native_evidence']['source_commit']}`",
        f"- Runtime input manifest: `{snapshot['native_evidence']['runtime_input_manifest_sha256']}`",
        f"- Historical source join SHA-256: `{snapshot['source_join_sha256']}`",
        f"- Current source applicability: {current}",
        f"- Source denominator: `{observed['counts']['joined_records']}`",
        f"- Catalog entries with observed reads: `{observed['counts'].get('observed_read', {}).get('observed_matched_read', 0)}`",
        f"- Catalog entries with observed writes: `{observed['counts'].get('observed_write', {}).get('observed_matched_write', 0)}`",
        f"- Source tables retained: `{len(snapshot['source_table_observations'])}`",
        f"- Source tables with observations: `{sum(any(bucket[axis].get(state, 0) for axis, state in [('observed_read', 'observed_matched_read'), ('observed_write', 'observed_matched_write')]) for bucket in snapshot['source_table_observations'].values())}`",
        "",
        "Read/write entry counts use catalog table coordinates. Native Group1 names, fixture occurrences and write operations are separate denominators in the machine-readable receipt.",
        "",
        "Historical integrity checks compare the stored ledger and receipt bindings. They do not rerun the native tools or prove current runtime behavior.", "",
    ])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--current-source-join", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--verify", action="store_true", required=True)
    args = parser.parse_args()
    snapshot = read(args.snapshot)
    validate_snapshot(snapshot)
    current = read(args.current_source_join) if args.current_source_join else None
    text = render_report(snapshot, current)
    if args.report:
        args.report.write_text(text, encoding="utf-8")
    print("=== instrument: catalog observed historical receipt check ===")
    print("Historical receipt integrity: PASS")
    if current is not None:
        print("Current source applicability: " + ("MATCH" if canonical_hash(current) == snapshot["source_join_sha256"] else "DIFFERS (receipt retained as historical)"))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        raise SystemExit(f"catalog observed receipt refused: {exc}")
