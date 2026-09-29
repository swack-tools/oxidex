"""Pure, fail-closed comparison policy for Task19 native read transitions.

This module consumes already authenticated fixture manifests and measurements.
It never reads a corpus, selects an oracle, builds a binary, or changes a tree.
"""

from __future__ import annotations

import hashlib
import json
import re
from collections import Counter, defaultdict
from pathlib import PurePosixPath

import conformance


UNION_SCHEMA = "task19-read-union/v1"
LEDGER_SCHEMA = "task19-read-ledger/v1"
PAIR_SCHEMA = "task19-read-pair/v1"
MANIFEST_KIND = "oxidex_version_rehearsal_fixture_manifest"
HEX_SHA = re.compile(r"[0-9a-f]{64}\Z")


class ReadPolicyRefused(ValueError):
    """An incomplete, ambiguous, or inconsistent read proof cannot pass."""


def _canonical_bytes(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode("utf-8")


def _sha(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _unique_object(pairs: list[tuple]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ReadPolicyRefused("fixture manifest repeats a JSON key")
        result[key] = value
    return result


def _manifest(data: bytes) -> tuple[str, dict[tuple[str, str], dict]]:
    if not isinstance(data, bytes):
        raise ReadPolicyRefused("fixture manifest must be immutable raw bytes")
    try:
        document = json.loads(data, object_pairs_hook=_unique_object)
    except (ValueError, UnicodeDecodeError) as exc:
        raise ReadPolicyRefused("fixture manifest JSON is invalid") from exc
    if not isinstance(document, dict) or document.get("schema") != 1 \
            or document.get("kind") != MANIFEST_KIND \
            or not isinstance(document.get("fixtures"), list) \
            or not document["fixtures"]:
        raise ReadPolicyRefused("fixture manifest schema or selection is invalid")
    rows = {}
    paths = set()
    logical_names = set()
    for row in document["fixtures"]:
        if not isinstance(row, dict) or set(row) != {"path", "sha256", "bytes"}:
            raise ReadPolicyRefused("fixture row shape is invalid")
        path, digest, size = row["path"], row["sha256"], row["bytes"]
        if (not isinstance(path, str) or not path.startswith("/")
                or path.count("/t/images/") != 1 or ".." in PurePosixPath(path).parts
                or not isinstance(digest, str) or HEX_SHA.fullmatch(digest) is None
                or type(size) is not int or size <= 0):
            raise ReadPolicyRefused("fixture path, digest, or positive size is invalid")
        logical = path.split("/t/images/", 1)[1]
        if not logical or path in paths or logical in logical_names:
            raise ReadPolicyRefused("fixture path or logical identity repeats")
        paths.add(path)
        logical_names.add(logical)
        rows[(logical, digest)] = {"path": path, "bytes": size}
    return _sha(data), rows


def freeze_union(before_manifest: bytes, after_manifest: bytes) -> dict:
    """Retain every distinct logical name/content pair from both manifests."""
    before_sha, before = _manifest(before_manifest)
    after_sha, after = _manifest(after_manifest)
    fixtures = []
    for logical, digest in sorted(before.keys() | after.keys()):
        prior, later = before.get((logical, digest)), after.get((logical, digest))
        if prior and later and prior["bytes"] != later["bytes"]:
            raise ReadPolicyRefused("same fixture content hash has conflicting sizes")
        fixtures.append({
            "logical_name": logical, "sha256": digest,
            "bytes": (prior or later)["bytes"],
            "sources": {"before": prior["path"] if prior else None,
                        "after": later["path"] if later else None},
        })
    proof = {"schema": UNION_SCHEMA,
             "manifest_sha256": {"before": before_sha, "after": after_sha},
             "fixtures": fixtures}
    return {**proof, "union_sha256": _sha(_canonical_bytes(proof))}


def _checked_fixture(fixture: dict) -> dict:
    if (not isinstance(fixture, dict)
            or not isinstance(fixture.get("logical_name"), str)
            or not fixture["logical_name"] or fixture["logical_name"].startswith("/")
            or ".." in PurePosixPath(fixture["logical_name"]).parts
            or not isinstance(fixture.get("sha256"), str)
            or HEX_SHA.fullmatch(fixture["sha256"]) is None
            or type(fixture.get("bytes")) is not int or fixture["bytes"] <= 0
            or not isinstance(fixture.get("sources"), dict)
            or set(fixture["sources"]) != {"before", "after"}
            or all(path is None for path in fixture["sources"].values())
            or any(path is not None and (not isinstance(path, str)
                                          or not path.startswith("/"))
                   for path in fixture["sources"].values())):
        raise ReadPolicyRefused("ledger fixture does not bind a union carrier")
    return {name: fixture[name] for name in ("logical_name", "sha256", "bytes")}


def _tag_rows(tags: dict, split, key_name: str) -> list[dict]:
    rows = []
    for key, value in conformance.transcript_tags(tags, split).items():
        group, name = split(key)
        rows.append({key_name: key, "group": group, "name": name,
                     "value": value, "normalized": conformance.norm_value(value)})
    # compare() consumes insertion order inside each duplicate-name bucket.
    return rows


def occurrence_ledger(fixture: dict, oracle_tags: dict, candidate_tags: dict,
                      transcript: dict, *, native_status: int) -> dict:
    """Replay one scored conformance row without losing full keys or duplicates."""
    bound_fixture = _checked_fixture(fixture)
    if (not isinstance(oracle_tags, dict) or not isinstance(candidate_tags, dict)
            or not all(isinstance(key, str) for tags in (oracle_tags, candidate_tags)
                       for key in tags)
            or type(native_status) is not int or native_status < 0
            or not isinstance(transcript, dict)
            or not isinstance(transcript.get("path"), str)
            or not transcript["path"].startswith("/")):
        raise ReadPolicyRefused("raw maps or transcript row are malformed")
    comparison = conformance.compare(oracle_tags, candidate_tags)
    expected = conformance.transcript_row(transcript["path"], oracle_tags,
                                          candidate_tags, comparison)
    if transcript != expected or not oracle_tags:
        raise ReadPolicyRefused("raw maps differ from the scored transcript commitment")
    native = _tag_rows(oracle_tags, conformance.split_oracle_key, "oracle_key")
    candidate = _tag_rows(candidate_tags, conformance.split_oxidex_key,
                          "candidate_key")
    native_buckets: dict[tuple[str, str, str], list[dict]] = defaultdict(list)
    for row in native:
        native_buckets[(row["group"], row["name"], row["normalized"])].append(row)
    matched = []
    native_by_name = conformance.tags_by_name(oracle_tags, conformance.split_oracle_key)
    candidate_by_name = conformance.tags_by_name(candidate_tags,
                                                  conformance.split_oxidex_key)
    for name in sorted(native_by_name.keys() | candidate_by_name.keys()):
        expected_bucket = native_by_name.get(name, [])
        actual_bucket = candidate_by_name.get(name, [])
        pairs, _diff, _missing, _extra, _duplicate = conformance._match_bucket(
            name, expected_bucket, actual_bucket)
        wanted = Counter((group, name, conformance.norm_value(value))
                         for group, value in pairs)
        for bucket, count in sorted(wanted.items()):
            available = native_buckets[bucket]
            if len(available) < count or (len(available) > count and len(available) > 1):
                raise ReadPolicyRefused("matched duplicate cannot identify its full oracle key")
            matched.extend({"oracle_key": row["oracle_key"], "group": row["group"],
                            "name": row["name"], "value": row["value"],
                            "normalized": row["normalized"]}
                           for row in available[:count])
    matched.sort(key=lambda row: row["oracle_key"])
    if len(matched) != expected["matched_occurrences"]:
        raise ReadPolicyRefused("matched ledger count differs from conformance")
    counts = {"matched": len(matched), "missing": len(comparison["missing"]),
              "value_diff": len(comparison["value_diff"]),
              "renames": len(comparison["renames"]), "extra": len(comparison["extra"])}
    payload = sum(row["group"] not in {"", "File", "System", "ExifTool"}
                  and row["name"] not in {"Error", "Warning", "FileType",
                                          "FileTypeExtension", "MIMEType"}
                  for row in native)
    proof = {"schema": LEDGER_SCHEMA, "fixture": bound_fixture,
             "transcript": expected, "native": native, "candidate": candidate,
             "native_status": native_status,
             "matched": matched,
             "discrepancies": {
                 "missing": {key: list(value) for key, value in comparison["missing"].items()},
                 "value_diff": [list(row) for row in comparison["value_diff"]],
                 "renames": [list(row) for row in comparison["renames"]],
                 "extra": {key: list(value) for key, value in comparison["extra"].items()},
             }, "counts": counts, "payload_occurrences": payload}
    try:
        return {**proof, "ledger_sha256": _sha(_canonical_bytes(proof))}
    except (TypeError, ValueError) as exc:
        raise ReadPolicyRefused("raw tag values are not canonical JSON") from exc


def _verified_union(union: dict) -> dict[tuple[str, str], dict]:
    if (not isinstance(union, dict) or set(union) !=
            {"schema", "manifest_sha256", "fixtures", "union_sha256"}
            or union["schema"] != UNION_SCHEMA
            or not isinstance(union["manifest_sha256"], dict)
            or set(union["manifest_sha256"]) != {"before", "after"}
            or any(not isinstance(v, str) or not HEX_SHA.fullmatch(v)
                   for v in union["manifest_sha256"].values())
            or not isinstance(union["fixtures"], list) or not union["fixtures"]):
        raise ReadPolicyRefused("frozen fixture union is incomplete")
    proof = {key: union[key] for key in ("schema", "manifest_sha256", "fixtures")}
    if union["union_sha256"] != _sha(_canonical_bytes(proof)):
        raise ReadPolicyRefused("frozen fixture union digest differs")
    indexed = {}
    for fixture in union["fixtures"]:
        _checked_fixture(fixture)
        key = fixture["logical_name"], fixture["sha256"]
        if key in indexed:
            raise ReadPolicyRefused("frozen fixture union repeats a carrier")
        indexed[key] = fixture
    if list(indexed) != sorted(indexed):
        raise ReadPolicyRefused("frozen fixture union order differs")
    return indexed


def _verified_ledgers(rows: list[dict], fixtures: dict) -> dict:
    if not isinstance(rows, list):
        raise ReadPolicyRefused("side occurrence ledgers are missing")
    indexed = {}
    for row in rows:
        if (not isinstance(row, dict) or row.get("schema") != LEDGER_SCHEMA
                or not isinstance(row.get("fixture"), dict)
                or not isinstance(row.get("ledger_sha256"), str)):
            raise ReadPolicyRefused("side occurrence ledger is malformed")
        key = row["fixture"].get("logical_name"), row["fixture"].get("sha256")
        if key not in fixtures or row["fixture"] != _checked_fixture(fixtures[key]) \
                or key in indexed:
            raise ReadPolicyRefused("side occurrence ledger carrier differs")
        proof = {k: v for k, v in row.items() if k != "ledger_sha256"}
        if row["ledger_sha256"] != _sha(_canonical_bytes(proof)):
            raise ReadPolicyRefused("side occurrence ledger digest differs")
        if (type(row.get("native_status")) is not int or row["native_status"] < 0
                or type(row.get("payload_occurrences")) is not int
                or row["payload_occurrences"] < 0):
            raise ReadPolicyRefused("native status or payload count is invalid")
        try:
            native = {entry["oracle_key"]: entry["value"] for entry in row["native"]}
            candidate = {entry["candidate_key"]: entry["value"]
                         for entry in row["candidate"]}
            replayed = occurrence_ledger(fixtures[key], native, candidate,
                                         row["transcript"],
                                         native_status=row["native_status"])
        except (KeyError, TypeError, ValueError) as exc:
            raise ReadPolicyRefused("side occurrence ledger cannot replay") from exc
        if row != replayed:
            raise ReadPolicyRefused("side occurrence ledger differs from raw-map replay")
        indexed[key] = row
    if set(indexed) != set(fixtures):
        raise ReadPolicyRefused("side occurrence ledgers omit a frozen carrier")
    return indexed


def _artifact_map(value: dict) -> dict:
    if (not isinstance(value, dict) or not value
            or any(not isinstance(path, str) or not path or path.startswith("/")
                   or ".." in PurePosixPath(path).parts
                   or not isinstance(digest, str) or not HEX_SHA.fullmatch(digest)
                   for path, digest in value.items())):
        raise ReadPolicyRefused("generated artifact manifest is incomplete")
    return value


def _evidence_index(evidence: list, required: set, shape: set, label: str,
                    allowed_reasons: set[str]) -> dict:
    if not isinstance(evidence, list):
        raise ReadPolicyRefused(f"{label} classification evidence is missing")
    indexed = {}
    for row in evidence:
        if (not isinstance(row, dict) or set(row) != shape
                or row.get("reason") not in allowed_reasons):
            raise ReadPolicyRefused(f"{label} classification row is malformed")
        key = tuple(row[k] for k in sorted(shape - {"reason"}))
        if key in indexed:
            raise ReadPolicyRefused(f"{label} classification repeats a change")
        indexed[key] = row
    if set(indexed) != required:
        raise ReadPolicyRefused(f"{label} classification omits or invents a change")
    return indexed


def _keyed_values(rows: list[dict], status: int) -> dict[str, str]:
    if status != 0:
        return {}
    return {row["oracle_key"]: row["normalized"]
            for row in rows if _is_scored_read(row)}


def _matched_counter(row: dict) -> Counter:
    if row["native_status"] != 0:
        return Counter()
    return Counter((entry["oracle_key"], entry["normalized"])
                   for entry in row["matched"] if _is_scored_read(entry))


def _is_scored_read(row: dict) -> bool:
    return row["name"] not in {"Error", "Warning"}


def _is_payload(row: dict) -> bool:
    return (row["group"] not in {"", "File", "System", "ExifTool"}
            and row["name"] not in {"Error", "Warning", "FileType",
                                    "FileTypeExtension", "MIMEType"})


def _extra_counter(row: dict) -> Counter:
    return Counter((name, group, conformance.norm_value(value))
                   for name, (group, value) in row["discrepancies"]["extra"].items())


def _gap_counter(row: dict) -> Counter:
    gaps = row["discrepancies"]
    entries = [["missing", name, group, conformance.norm_value(value)]
               for name, (group, value) in gaps["missing"].items()]
    entries += [["value", name, conformance.norm_value(expected),
                 conformance.norm_value(actual)]
                for name, expected, actual, _severity in gaps["value_diff"]]
    entries += [["rename", source, target, conformance.norm_value(value)]
                for source, target, value in gaps["renames"]]
    return Counter(_canonical_bytes(entry).decode("utf-8") for entry in entries)


def _missing_gap_id(name: str, group: str, value: object) -> str:
    return _canonical_bytes(["missing", name, group,
                             conformance.norm_value(value)]).decode("utf-8")


def _exact_missing_key(row: dict, native_row: dict) -> str | None:
    """Map a new native occurrence to one unambiguous MISSING report row."""
    signature = (native_row["group"], native_row["name"],
                 native_row["normalized"])
    native_count = sum((entry["group"], entry["name"], entry["normalized"])
                       == signature for entry in row["native"])
    if native_count != 1:
        return None
    missing = []
    for report_key, (group, value) in row["discrepancies"]["missing"].items():
        base = re.sub(r" \([2-9][0-9]*\)$", "", report_key)
        if (group, conformance.norm_value(value)) == signature[::2] \
                and base in {native_row["name"],
                             f'{native_row["group"]}:{native_row["name"]}'}:
            missing.append(report_key)
    return missing[0] if len(missing) == 1 else None


def replay_pair(*, mode: str, union: dict, before_ledgers: list[dict],
                after_ledgers: list[dict], payload_floors: dict,
                before_artifacts: dict, after_artifacts: dict,
                native_change_evidence: list, artifact_change_evidence: list) -> dict:
    """Replay a complete two-side transition without granting standing gaps credit.

    Classifications are exact observed deltas, supplied independently by the
    caller. This pure helper does not authenticate external files or sources.
    """
    if mode not in {"same-pin", "historical"}:
        raise ReadPolicyRefused("unknown transition read mode")
    fixtures = _verified_union(union)
    before, after = (_verified_ledgers(before_ledgers, fixtures),
                     _verified_ledgers(after_ledgers, fixtures))
    if (not isinstance(payload_floors, dict)
            or set(payload_floors) != {"before", "after"}
            or any(type(v) is not int or v <= 0 for v in payload_floors.values())):
        raise ReadPolicyRefused("positive native payload floors must be explicit")
    payload = {side: sum(row["payload_occurrences"] if row["native_status"] == 0
                         else 0 for row in rows.values())
               for side, rows in (("before", before), ("after", after))}
    if any(payload[side] < floor for side, floor in payload_floors.items()):
        raise ReadPolicyRefused("native payload floor was not reached")
    prior_artifacts, later_artifacts = (_artifact_map(before_artifacts),
                                        _artifact_map(after_artifacts))
    artifact_delta = [{"path": path, "before": prior_artifacts.get(path),
                       "after": later_artifacts.get(path)}
                      for path in sorted(prior_artifacts.keys() | later_artifacts.keys())
                      if prior_artifacts.get(path) != later_artifacts.get(path)]
    if mode == "same-pin":
        if artifact_delta or native_change_evidence or artifact_change_evidence:
            raise ReadPolicyRefused("same-pin artifacts or change evidence differ")
    else:
        required = {tuple(row[k] for k in sorted(("after", "before", "path")))
                    for row in artifact_delta}
        _evidence_index(artifact_change_evidence, required,
                        {"path", "before", "after", "reason"}, "artifact",
                        {"generated-artifact"})
    native_delta = []
    newly_supported_unread = []
    new_losses = []
    standing = Counter()
    for key in sorted(fixtures):
        old, new = before[key], after[key]
        allowed_new_missing = Counter()
        if mode == "same-pin":
            for field in ("native_status", "native", "candidate", "matched",
                          "discrepancies", "counts", "payload_occurrences"):
                if old[field] != new[field]:
                    raise ReadPolicyRefused(f"same-pin {field} differs for {key}")
        old_native = _keyed_values(old["native"], old["native_status"])
        new_native = _keyed_values(new["native"], new["native_status"])
        for oracle_key in sorted(old_native.keys() | new_native.keys()):
            prior, later = old_native.get(oracle_key), new_native.get(oracle_key)
            if prior != later:
                native_delta.append({"logical_name": key[0], "fixture_sha256": key[1],
                                     "oracle_key": oracle_key, "before": prior,
                                     "after": later})
        old_matched, new_matched = _matched_counter(old), _matched_counter(new)
        stable_native = {(k, v) for k, v in old_native.items()
                         if new_native.get(k) == v}
        lost = (old_matched - new_matched)
        for identity, count in lost.items():
            if identity in stable_native:
                new_losses.append({"logical_name": key[0], "fixture_sha256": key[1],
                                   "oracle_key": identity[0], "value": identity[1],
                                   "count": count})
        if new_losses:
            raise ReadPolicyRefused("previously matched native-supported read was lost")
        if mode == "historical":
            for oracle_key, value in new_native.items():
                if old_native.get(oracle_key) != value \
                        and new_matched[(oracle_key, value)] < 1:
                    if oracle_key in old_native:
                        raise ReadPolicyRefused("changed native value lacks its new match")
                    native_row = next(row for row in new["native"]
                                      if row["oracle_key"] == oracle_key)
                    missing_key = _exact_missing_key(new, native_row)
                    if missing_key is None:
                        raise ReadPolicyRefused("new native occurrence is not exact MISSING")
                    newly_supported_unread.append({
                        "logical_name": key[0], "fixture_sha256": key[1],
                        "oracle_key": oracle_key, "value": value,
                        "missing_key": missing_key})
                    group, raw_value = new["discrepancies"]["missing"][missing_key]
                    allowed_new_missing[_missing_gap_id(missing_key, group,
                                                        raw_value)] += 1
            for oracle_key, value in old_native.items():
                if new_native.get(oracle_key) != value:
                    name = conformance.split_oracle_key(oracle_key)[1]
                    if not any(row["name"] == name and row["normalized"] == value
                               for row in new["native"]):
                        if any(row["name"] == name and row["normalized"] == value
                                   for row in new["candidate"]):
                            raise ReadPolicyRefused("removed native value retained by candidate")
            if _extra_counter(new) - _extra_counter(old):
                raise ReadPolicyRefused("new unclassified candidate EXTRA")
            if _gap_counter(new) - _gap_counter(old) - allowed_new_missing:
                raise ReadPolicyRefused("new unclassified candidate read gap")
        standing.update(old["counts"])
    if mode == "same-pin":
        if native_delta:
            raise ReadPolicyRefused("same-pin native observations differ")
    else:
        required = {tuple(row[k] for k in sorted(("after", "before", "fixture_sha256",
                                                  "logical_name", "oracle_key")))
                    for row in native_delta}
        classified = _evidence_index(
            native_change_evidence, required,
            {"logical_name", "fixture_sha256", "oracle_key", "before",
             "after", "reason"}, "native", {"native-version", "native-new-unread"})
        unread_keys = {(row["logical_name"], row["fixture_sha256"],
                        row["oracle_key"]) for row in newly_supported_unread}
        for row in native_delta:
            evidence_key = tuple(row[k] for k in sorted(
                ("after", "before", "fixture_sha256", "logical_name", "oracle_key")))
            actual_reason = classified[evidence_key]["reason"]
            expected_reason = ("native-new-unread" if
                               (row["logical_name"], row["fixture_sha256"],
                                row["oracle_key"]) in unread_keys else "native-version")
            if actual_reason != expected_reason:
                raise ReadPolicyRefused("native change disposition differs")
    proof = {"schema": PAIR_SCHEMA, "mode": mode, "status": "passed",
             "union_sha256": union["union_sha256"],
             "manifest_sha256": union["manifest_sha256"],
             "ledger_sha256": {"before": [before[key]["ledger_sha256"] for key in sorted(fixtures)],
                               "after": [after[key]["ledger_sha256"] for key in sorted(fixtures)]},
             "payload_occurrences": payload, "payload_floors": payload_floors,
             "artifact_manifest_sha256": {"before": _sha(_canonical_bytes(prior_artifacts)),
                                          "after": _sha(_canonical_bytes(later_artifacts))},
             "artifact_delta": artifact_delta, "native_delta": native_delta,
             "newly_supported_unread": newly_supported_unread,
             "native_error_carriers": {side: [list(key) for key, row in sorted(rows.items())
                                               if row["native_status"] != 0]
                                       for side, rows in (("before", before), ("after", after))},
             "standing": dict(standing), "new_losses": new_losses}
    return {**proof, "pair_sha256": _sha(_canonical_bytes(proof))}
