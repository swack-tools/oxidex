#!/usr/bin/env python3
"""Fail-closed Task18 deletion chronology and no-new-manual-knowledge gate.

This records historical changes without retroactively granting their missing
pre-deletion approval. A historical output-parity receipt is not authorization.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Callable

import runtime_ownership as ownership
import version_rehearsal_clean_snapshot as clean_snapshot
from genshare import attribute

# Direct script invocation starts with tools/exiftool-tables on sys.path.
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from scripts import ops_paths

LEDGER = Path("docs/reference/generated-runtime-deletion-ledger.json")
INTEGRATION_COMMIT = "4f653243d33cf053107f4f19b8bd449329de611b"
GENERATED_BASELINE_BLOB = "a8af250fee51d8f12b93c6935b6f05b41e77cc67"
GENERATED_BASELINE_SHA256 = "d77ab9d6c74110280edf3069596b4345c718c82b2569445ffd4aa0f82293612c"
# The generated owner set at the already-recorded Task18 integration point.
# Candidate inventory changes cannot remove this independent reference.
GENERATED_BASELINE_BLOB = "a8af250fee51d8f12b93c6935b6f05b41e77cc67"
GENERATED_BASELINE_SHA256 = "d77ab9d6c74110280edf3069596b4345c718c82b2569445ffd4aa0f82293612c"
HISTORICAL = {
    "D1": ("src/core/exif_dir_engine.rs::DirEngineRows::drain", "test-only-alias-removal", "13ce49071830ca2b76cfcdfdececb537a21e3e9a"),
    "D2": ("src/core/tiff_helpers.rs::EXIF_IFD_SILENCE_EDGES", "constant-inline", "0ddcbb1b9cb7c67aa12d55059649e16bc7e4eb53"),
    "D3": ("src/core/tiff_helpers.rs::parse_ifd1_with_session", "duplicate-branch-merge", "3d83a2cbc4ff3c86c05f1f9d117798eb2392cd7e"),
    "D5": ("src/core/exif_dir_engine.rs::DirEngineRows::drain_ifd0", "production-alias-removal-with-embedded-caller-update", "bb826db3c18532cdc5db7ed3f9ad798581301216"),
    "B2": ("src/core/exif_dir_engine.rs::IFD0_HAND_KEPT", "five-id-generated-ownership-transfer-with-retained-on-decline-fallback", "f9fc0f74db6e0d17c2bca898c9a23ba1f24fe5ac"),
}
KEEP_COUNTS = {"A1-A4": 4, "B1": 1, "B3": 1, "B4": 1, "B5": 1,
               "C1": 1, "C2-C10": 27, "C11-C12": 2, "D4,D6-D10": 6, "E1-E5": 5}
# These ten overlap rows were already present before this gate. Future residual
# overlap needs a reviewed ownership change; the ledger cannot silently add it.
KNOWN_GENERATED_RESIDUALS = {
    ("ExifIFD/0x9400", "src/core/tiff_helpers.rs::EXIF_IFD_HAND_KEPT", "hand-kept"),
    *((f"IFD0/{value}", "src/core/exif_dir_engine.rs::IFD0_HAND_KEPT", "hand-kept")
      for value in ("0x85d8", "0x87af", "0x87b0", "0x87b1")),
    *((f"IFD0/0x9c9{suffix}", "src/core/exif_dir_engine.rs::IFD0_HAND_ON_DECLINE", "fallback-on-decline")
      for suffix in "bcdef"),
}
SYMBOL = re.compile(r"(?:src/[A-Za-z0-9_./-]+\.rs)::[A-Za-z_][A-Za-z0-9_:.-]*\Z")
# A packet cannot appoint its own authority. No Task18 controller signing
# key has been approved; tests inject an independently generated test key.
PRODUCTION_CONTROLLER_KEY: bytes | None = None
STRUCTURAL = {"process_keyed_directory", "process_serial_directory", "ifd0_walk",
              "take_ifd0", "finish_ifd0", "keep_hand_on_decline", "process_exif",
              "process_binary_data", "execute"}


class Refused(ValueError):
    pass


def validate_document(document: object) -> dict:
    required = {"schema", "status", "historical_changes", "retained_groups",
                "approved_finite_appendix", "controller_reconciliation_manifest", "prospective_entries"}
    if not isinstance(document, dict) or set(document) != required:
        raise Refused("deletion ledger has incomplete or unknown fields")
    if document["schema"] != "runtime-deletion-ledger/v1" or document["status"] != "historical-unqualified":
        raise Refused("historical deletion ledger cannot assert approval")
    changes = document["historical_changes"]
    if not isinstance(changes, list) or len(changes) != len(HISTORICAL):
        raise Refused("ledger must account for all five historical changes")
    seen = set()
    for row in changes:
        if not isinstance(row, dict) or set(row) != {"id", "old_symbol", "change_kind", "development_commit", "integration_commit", "qualification", "literal_deleted", "source_fields"}:
            raise Refused("historical change has malformed fields")
        key = row["id"]
        if key in seen or key not in HISTORICAL or tuple(row[name] for name in
                ("old_symbol", "change_kind", "development_commit")) != HISTORICAL[key] or row["integration_commit"] != INTEGRATION_COMMIT:
            raise Refused("historical change differs from retained source history")
        expected_fields = [f"Exif::Main:0x9c9{suffix}" for suffix in "bcdef"] if key == "B2" else []
        if (not SYMBOL.fullmatch(row["old_symbol"])
                or row["literal_deleted"] is not (key in {"D1", "D2", "D5"})
                or row["source_fields"] != expected_fields
                or row["qualification"] != "unqualified-no-pre-deletion-appendix"):
            raise Refused("historical change cannot claim retroactive qualification or a false literal deletion")
        seen.add(key)
    groups = document["retained_groups"]
    if not isinstance(groups, list) or len(groups) != len(KEEP_COUNTS):
        raise Refused("ledger must preserve all 49 KEEP dispositions")
    found = {}
    for group in groups:
        if (not isinstance(group, dict) or set(group) != {"id", "count", "reason"}
                or group["id"] in found or group["id"] not in KEEP_COUNTS
                or type(group["count"]) is not int or group["count"] != KEEP_COUNTS[group["id"]]
                or not isinstance(group["reason"], str) or not group["reason"].strip()):
            raise Refused("retained group is missing or has a false count/reason")
        found[group["id"]] = group["count"]
    if set(found) != set(KEEP_COUNTS) or sum(found.values()) != 49:
        raise Refused("ledger does not account for exactly 49 retained cases")
    if document["approved_finite_appendix"] is not None or document["controller_reconciliation_manifest"] is not None:
        raise Refused("unapproved appendix or reconciliation cannot be promoted by this historical ledger")
    if document["prospective_entries"] != []:
        raise Refused("prospective deletion requires a separately authenticated finite appendix")
    return document


def generated_baseline(root: Path) -> set[tuple[str, str, str]]:
    if git(root, "rev-parse", f"{INTEGRATION_COMMIT}:tools/exiftool-tables/runtime_ownership.json") != GENERATED_BASELINE_BLOB:
        raise Refused("pinned generated ownership baseline is not the integration inventory")
    raw = git_bytes(root, "cat-file", "blob", GENERATED_BASELINE_BLOB)
    if hashlib.sha256(raw).hexdigest() != GENERATED_BASELINE_SHA256:
        raise Refused("pinned generated ownership baseline differs")
    baseline = _strict_json(raw)
    if (not isinstance(baseline, dict) or baseline.get("schema") != 1
            or baseline.get("source_release") != "13.59" or not isinstance(baseline.get("rows"), list)):
        raise Refused("pinned generated ownership baseline malformed")
    ownership.verify_rows(baseline["rows"])
    generated = {(row["module"], row["table"], row["field"]["value"].split("/")[-1])
                 for row in baseline["rows"] if row["owner"] == "generated"}
    if len(generated) != 563 or ("Exif", "Main", "0x9c9b") not in generated:
        raise Refused("pinned generated ownership baseline incomplete")
    return generated


def no_new_manual(rows: list[dict], *, root: Path | None = None) -> None:
    ownership.verify_rows(rows)
    generated = generated_baseline(root or Path(__file__).resolve().parents[2])
    generated.update((row["module"], row["table"], row["field"]["value"].split("/")[-1])
                     for row in rows if row["owner"] == "generated")
    for row in rows:
        if row["owner"] != "residual":
            continue
        identity = row["field"]["value"]
        if (row["module"], row["table"], identity.split("/")[-1]) not in generated:
            continue
        if row["module"] != "Exif" or row["table"] != "Main" or (
                identity, row["symbol"], row.get("residual_disposition")) not in KNOWN_GENERATED_RESIDUALS:
            raise Refused(f"new manual owner for generated source construct: {row['module']}::{row['table']}:{identity}")


def _trusted_git() -> str:
    path = Path("/usr/bin/git")
    for component in (Path("/"), Path("/usr"), Path("/usr/bin"), path):
        try:
            info = component.lstat()
        except OSError as exc:
            raise Refused("trusted Git executable unavailable") from exc
        kind_ok = stat.S_ISREG(info.st_mode) if component == path else stat.S_ISDIR(info.st_mode)
        if not kind_ok or info.st_uid != 0 or info.st_mode & 0o022:
            raise Refused("trusted Git executable identity invalid")
    return str(path)


def _git_env() -> dict[str, str]:
    # No ambient GIT_DIR, index, config, namespace, object directory or PATH.
    return {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "HOME": "/nonexistent",
            "GIT_NO_REPLACE_OBJECTS": "1", "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": "/dev/null"}


def git_bytes(root: Path, *args: str) -> bytes:
    try:
        return subprocess.check_output([_trusted_git(), "-c", "core.fsmonitor=false",
                                        "-C", str(root), *args],
                                       stderr=subprocess.PIPE, timeout=30, env=_git_env())
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        raise Refused("candidate Git object or checkout verification failed") from exc


def git(root: Path, *args: str) -> str:
    return git_bytes(root, *args).decode("utf-8").strip()


def _is_ancestor(root: Path, ancestor: str, head: str) -> bool:
    return subprocess.run([_trusted_git(), "-c", "core.fsmonitor=false", "-C", str(root),
                           "merge-base", "--is-ancestor", ancestor, head],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30,
                          env=_git_env()).returncode == 0


def verify_history(root: Path) -> str:
    if git(root, "status", "--porcelain=v1", "--untracked-files=all"):
        raise Refused("deletion gate source must be clean")
    head = git(root, "rev-parse", "HEAD")
    if git(root, "write-tree") != git(root, "rev-parse", "HEAD^{tree}"):
        raise Refused("deletion gate index differs from committed tree")
    # The five development commits were squash-merged as PR #940. They are
    # evidence references, not ancestors of the integration branch.
    if not _is_ancestor(root, INTEGRATION_COMMIT, head):
        raise Refused("Task18 squash merge is not in candidate lineage")
    return head


def verify(root: Path, *, ops_root: Path | None = None) -> dict:
    root = root.resolve()
    document = validate_document(json.loads((root / LEDGER).read_text(encoding="utf-8")))
    head = verify_history(root)
    rows = ownership.load_rows(root, ops_root)
    no_new_manual(rows, root=root)
    return {"status": "BLOCKED", "candidate_head": head,
            "historical_unqualified": len(document["historical_changes"]),
            "retained": sum(group["count"] for group in document["retained_groups"]),
            "missing": ["approved finite appendix authenticated before each deletion",
                        "controller reconciliation and receipt attestations",
                        "current source/binary and generated-on/off attribution receipts",
                        "zero-reachability and oracle-capability receipts"]}


def _sha(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _no_symlink_ancestors(path: Path) -> None:
    cursor = Path(path.anchor)
    for component in path.parts[1:]:
        cursor /= component
        if cursor.is_symlink():
            raise Refused(f"evidence path has symlink ancestor: {cursor}")


def _ops_root(ops_root: Path, *, test_only_temporary_evidence: bool = False) -> Path:
    if not isinstance(ops_root, Path) or not ops_root.is_absolute():
        raise Refused("durable ops root must be absolute")
    try:
        if test_only_temporary_evidence:
            _no_symlink_ancestors(ops_root)
            resolved = ops_root.resolve()
        else:
            resolved = ops_paths.durable_root(ops_root, "Task18 evidence root")
    except ValueError as exc:
        raise Refused(f"durable ops root invalid: {exc}") from exc
    if not resolved.is_dir():
        raise Refused("durable ops root is not a directory")
    return resolved


def _file(path: object, ops_root: Path, *, test_only_temporary_evidence: bool = False) -> Path:
    root = _ops_root(ops_root, test_only_temporary_evidence=test_only_temporary_evidence)
    if not isinstance(path, str) or not Path(path).is_absolute():
        raise Refused("evidence path must be absolute")
    value = Path(path)
    _no_symlink_ancestors(value)
    if (not value.is_file() or not value.resolve().is_relative_to(root)
            or value.stat().st_size > 8_000_000):
        raise Refused("evidence must be bounded regular file below durable ops root")
    return value


def _trusted_ssh_keygen() -> str:
    """Use the system verifier only when its path is owned by the OS."""
    path = Path("/usr/bin/ssh-keygen")
    for component in (Path("/"), Path("/usr"), Path("/usr/bin"), path):
        try:
            info = component.lstat()
        except OSError as exc:
            raise Refused("BLOCKED_AUTHORITY: trusted SSH verifier unavailable") from exc
        kind_ok = stat.S_ISREG(info.st_mode) if component == path else stat.S_ISDIR(info.st_mode)
        if not kind_ok or info.st_uid != 0 or info.st_mode & 0o022:
            raise Refused("BLOCKED_AUTHORITY: trusted SSH verifier identity invalid")
    return str(path)


def _reject_non_json_constant(value: str) -> None:
    raise ValueError(f"non-JSON constant {value}")


def _strict_json(raw: bytes) -> object:
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=attribute._reject_duplicate_pairs,
                          parse_constant=_reject_non_json_constant)
    except (UnicodeError, ValueError) as exc:
        raise Refused("signed evidence JSON is ambiguous or invalid") from exc

def _signed(binding: object, ops_root: Path, key: bytes, *,
            test_only_temporary_evidence: bool = False) -> dict:
    if not isinstance(binding, dict) or set(binding) != {"path", "sha256", "signature_path"}:
        raise Refused("signed evidence binding incomplete")
    if not isinstance(binding["sha256"], str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", binding["sha256"]):
        raise Refused("signed evidence digest malformed")
    raw = _file(binding["path"], ops_root, test_only_temporary_evidence=test_only_temporary_evidence).read_bytes()
    sig = _file(binding["signature_path"], ops_root, test_only_temporary_evidence=test_only_temporary_evidence)
    if _sha(raw) != binding["sha256"]:
        raise Refused("signed evidence bytes differ from digest")
    if not key.startswith(b"ssh-ed25519 "):
        raise Refused("BLOCKED_AUTHORITY: trusted controller key unavailable")
    ssh = _trusted_ssh_keygen()
    with tempfile.TemporaryDirectory(prefix="task18-verify-") as directory:
        allowed = Path(directory) / "allowed"
        allowed.write_bytes(b"task18-controller " + key.strip() + b"\n")
        check = subprocess.run([ssh, "-Y", "verify", "-f", str(allowed), "-I", "task18-controller",
                                "-n", "oxidex-task18-controller", "-s", str(sig)],
                               input=raw, capture_output=True, timeout=30,
                               env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"})
    if check.returncode:
        raise Refused("controller signature invalid")
    document = _strict_json(raw)
    if not isinstance(document, dict):
        raise Refused("signed evidence is not an object")
    return document


def _identity(document: dict, kind: str, entry: dict, source: str, binary: str,
              integration: str, merge: str) -> None:
    expected = {"schema": "runtime-deletion-evidence/v1", "kind": kind, "task": "18",
                "old_symbol": entry["old_symbol"], "source_fields": entry["source_fields"],
                "source_sha256": source, "binary_sha256": binary,
                "integration_sha": integration, "merge_sha": merge}
    if any(document.get(name) != value for name, value in expected.items()):
        raise Refused(f"{kind} evidence identity mismatch")


def evaluate_prospective(packet: dict, *, root: Path, ops_root: Path,
                         binary_path: Path, controller_key: bytes | None,
                         test_only_attribution_bridge: Callable[[dict, list[str]], bool] | None = None) -> dict:
    """Replay finite packet bindings; production additionally requires Task8 proof.

    The test-only callback verifies packet plumbing, never production approval.
    """
    if controller_key is None:
        raise Refused("BLOCKED_AUTHORITY: no approved Task18 controller trust anchor")
    if not isinstance(packet, dict) or set(packet) != {"schema", "appendix", "manifest", "entries"} or packet["schema"] != "runtime-deletion-packet/v1":
        raise Refused("prospective packet schema malformed")
    if not isinstance(packet["entries"], list) or not packet["entries"]:
        raise Refused("empty prospective packet cannot pass")
    test_only_temporary_evidence = test_only_attribution_bridge is not None
    ops_root = _ops_root(ops_root, test_only_temporary_evidence=test_only_temporary_evidence)
    root = root.resolve()
    head = verify_history(root)
    source = "sha256:" + clean_snapshot.source_tree_sha256(root)
    if (not binary_path.is_absolute() or binary_path.is_symlink() or not binary_path.is_file()
            or binary_path.stat().st_size > 1_000_000_000):
        raise Refused("explicit release binary must be a bounded regular absolute file")
    binary = _sha(binary_path.read_bytes())
    appendix = _signed(packet["appendix"], ops_root, controller_key, test_only_temporary_evidence=test_only_temporary_evidence)
    manifest = _signed(packet["manifest"], ops_root, controller_key, test_only_temporary_evidence=test_only_temporary_evidence)
    integration, merge = appendix.get("integration_sha"), appendix.get("merge_sha")
    if (appendix.get("schema") != "runtime-deletion-appendix/v1" or appendix.get("task") != "18"
            or appendix.get("approval") != "prospective-approved"
            or not all(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value) for value in (integration, merge))
            or appendix.get("candidate_commit") != head or appendix.get("source_sha256") != source
            or appendix.get("binary_sha256") != binary or not isinstance(appendix.get("candidates"), list)
            or not appendix["candidates"]):
        raise Refused("appendix not approved for current candidate source/binary")
    for ancestor in (integration, merge):
        if not _is_ancestor(root, ancestor, head):
            raise Refused("appendix lineage not in source history")
    if (manifest.get("schema") != "runtime-deletion-manifest/v1" or manifest.get("task") != "18"
            or manifest.get("appendix_sha256") != packet["appendix"]["sha256"]
            or any(manifest.get(name) != value for name, value in
                   {"candidate_commit": head, "source_sha256": source, "binary_sha256": binary,
                    "integration_sha": integration, "merge_sha": merge}.items())):
        raise Refused("controller reconciliation manifest identity mismatch")
    rows = ownership.load_rows(root, ops_root)
    no_new_manual(rows, root=root)
    by_field = {ownership.StableFieldId.from_row(row).text(): row for row in rows}
    candidates = appendix["candidates"]
    if not all(isinstance(candidate, dict) and set(candidate) == {"old_symbol", "source_fields", "new_owner", "candidate_source_sha256", "candidate_binary_sha256"} for candidate in candidates):
        raise Refused("appendix candidate malformed")
    approved = {candidate["old_symbol"]: candidate for candidate in candidates}
    if len(approved) != len(candidates) or len(packet["entries"]) != len(candidates):
        raise Refused("appendix and ledger are not one finite set")
    seen, seen_fields, expected_receipts = set(), set(), {}
    for entry in packet["entries"]:
        expected_keys = {"old_symbol", "source_fields", "new_owner", "candidate_source_sha256", "candidate_binary_sha256", "receipt_task", "receipt_integration_sha", "receipt_merge_sha", "controller_reconciliation_manifest_sha256", "generated_on", "generated_off", "receipt_bindings", "deletion_commit"}
        if not isinstance(entry, dict) or set(entry) != expected_keys:
            raise Refused("ledger entry malformed")
        symbol, fields, owner = entry["old_symbol"], entry["source_fields"], entry["new_owner"]
        if not isinstance(symbol, str) or not SYMBOL.fullmatch(symbol) or symbol in seen or symbol not in approved:
            raise Refused("unlisted, duplicate, or nonliteral symbol")
        seen.add(symbol)
        if symbol.rsplit("::", 1)[-1] in STRUCTURAL:
            raise Refused("structural/live public owner is nondeletable")
        if (not isinstance(fields, list) or not fields or not all(isinstance(field, str) for field in fields)
                or len(set(fields)) != len(fields) or any(field not in by_field for field in fields)
                or seen_fields.intersection(fields)):
            raise Refused("source fields absent or mismatched with ownership")
        seen_fields.update(fields)
        if not isinstance(owner, str) or not owner or any(
                row["owner"] != ("generated" if owner == "generated" else "residual")
                or (owner != "generated" and row["symbol"] != owner)
                for row in (by_field[field] for field in fields)):
            raise Refused("replacement owner or duplicate owner mismatch")
        if {key: entry[key] for key in approved[symbol]} != approved[symbol]:
            raise Refused("ledger entry differs from authenticated appendix")
        if (entry["candidate_source_sha256"] != source or entry["candidate_binary_sha256"] != binary
                or entry["receipt_task"] != "18" or entry["receipt_integration_sha"] != integration
                or entry["receipt_merge_sha"] != merge
                or entry["controller_reconciliation_manifest_sha256"] != packet["manifest"]["sha256"]
                or entry["generated_on"] != "matched" or entry["generated_off"] != "missing-or-residual"
                or entry["deletion_commit"] not in (None, head)):
            raise Refused("candidate identity, generated on/off, or lineage mismatch")
        bindings = entry["receipt_bindings"]
        kinds = {"oracle", "attribution", "zero_reachability", "oracle_capability"}
        if not isinstance(bindings, dict) or set(bindings) != kinds:
            raise Refused("required authenticated receipt missing")
        evidence = {kind: _signed(binding, ops_root, controller_key, test_only_temporary_evidence=test_only_temporary_evidence) for kind, binding in bindings.items()}
        for kind, document in evidence.items():
            _identity(document, kind, entry, source, binary, integration, merge)
        cap = evidence["oracle_capability"]
        if cap.get("release") != "13.59" or cap.get("perl") != "5.38.2" or cap.get("capability_probe") != "PASS":
            raise Refused("pinned oracle capability absent")
        oracle = evidence["oracle"]
        if (oracle.get("capability_sha256") != bindings["oracle_capability"]["sha256"]
                or type(oracle.get("matched_occurrences")) is not int or oracle["matched_occurrences"] < 1
                or oracle.get("lost_occurrences") != 0 or oracle.get("new_value_rows") != 0):
            raise Refused("oracle replacement proof absent or regressed")
        attr = evidence["attribution"]
        if attr.get("generated_on") != "matched" or attr.get("generated_off") != "missing-or-residual" or attr.get("duplicate_owner") is not False:
            raise Refused("generated on/off or duplicate-owner proof absent")
        if test_only_attribution_bridge is not None:
            if test_only_attribution_bridge(attr, fields) is not True:
                raise Refused("BLOCKED_ATTRIBUTION: test bridge did not exercise every source field")
        else:
            task8 = attr.get("task8_v3_receipt")
            if not isinstance(task8, dict):
                raise Refused("BLOCKED_ATTRIBUTION: actual Task8 v3 receipt absent")
            from genshare import attribute as task8_validator
            retained = _signed(task8, ops_root, controller_key)
            receipt_path = Path(task8["path"])
            try:
                task8_validator.validate_main(["--receipt", str(receipt_path),
                                                "--require-success", "--recheck-live-inputs"])
            except (task8_validator.ReceiptError, OSError, ValueError) as exc:
                raise Refused("BLOCKED_ATTRIBUTION: Task8 v3 replay failed") from exc
            if (retained.get("source", {}).get("commit") != head
                    or retained.get("source", {}).get("tree") != git(root, "rev-parse", "HEAD^{tree}")
                    or retained.get("source", {}).get("clean") is not True
                    or retained.get("build", {}).get("binary", {}).get("sha256") != binary.removeprefix("sha256:")):
                raise Refused("BLOCKED_ATTRIBUTION: Task8 source/binary differs")
            from runtime_field_projection import BlockedAttribution, project
            try:
                project(retained, attr.get("field_projections"), fields, rows)
            except (BlockedAttribution, task8_validator.ReceiptError, OSError,
                    KeyError, TypeError, ValueError) as exc:
                raise Refused(f"BLOCKED_ATTRIBUTION: {exc}") from exc
            try:
                task8_validator.validate_main(["--receipt", str(receipt_path),
                                                "--require-success", "--recheck-live-inputs"])
                if _signed(task8, ops_root, controller_key) != retained:
                    raise Refused("BLOCKED_ATTRIBUTION: Task8 receipt changed during projection")
            except (task8_validator.ReceiptError, OSError, ValueError) as exc:
                raise Refused("BLOCKED_ATTRIBUTION: Task8 raw artifacts changed during projection") from exc
        reach = evidence["zero_reachability"]
        fixture_pattern = r"sha256:[0-9a-f]{64}"
        if not all(isinstance(document.get("fixture_sha256"), str)
                   and re.fullmatch(fixture_pattern, document["fixture_sha256"])
                   for document in (oracle, reach)):
            raise Refused("oracle and reachability fixture SHA-256 identities are required")
        if (reach.get("reachable") is not False or reach.get("remaining_callsites") != []
                or reach.get("definition_checked") is not True
                or reach.get("fixture_sha256") != oracle.get("fixture_sha256")
                or reach.get("capability_sha256") != bindings["oracle_capability"]["sha256"]):
            raise Refused("zero-reachability proof absent, live, or stale")
        expected_receipts[symbol] = {kind: binding["sha256"] for kind, binding in bindings.items()}
    if seen != set(approved) or manifest.get("receipts") != expected_receipts:
        raise Refused("controller manifest receipt set incomplete")
    if test_only_attribution_bridge is None:
        # Signed summaries bind identities but do not replay the capable
        # oracle invocation or resolve Rust/AST and dynamic call edges.
        raise Refused("BLOCKED_RAW_PROOF: oracle capability and zero-reachability raw replay absent")
    if (verify_history(root) != head or "sha256:" + clean_snapshot.source_tree_sha256(root) != source
            or _sha(binary_path.read_bytes()) != binary):
        raise Refused("candidate source or binary changed during receipt replay")
    return {"status": "PASS_TEST_PACKET_BINDINGS", "control": "prospective-deletion-packet-test-only", "candidate_head": head,
            "source_sha256": source, "binary_sha256": binary, "approved_symbols": sorted(seen)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("verify", "no-new-manual", "prospective"))
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--ops-root", type=Path)
    parser.add_argument("--packet", type=Path)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "prospective":
            if args.packet is None or args.binary is None or args.ops_root is None:
                raise Refused("prospective evaluation requires --packet, --binary, and --ops-root")
            _ops_root(args.ops_root)
            if PRODUCTION_CONTROLLER_KEY is None:
                raise Refused("BLOCKED_AUTHORITY: no approved Task18 controller trust anchor")
            packet = json.loads(_file(str(args.packet), args.ops_root).read_bytes())
            print(json.dumps(evaluate_prospective(packet, root=args.root, ops_root=args.ops_root,
                                                 binary_path=args.binary,
                                                 controller_key=PRODUCTION_CONTROLLER_KEY), sort_keys=True))
            return 0
        if args.command == "no-new-manual":
            root = args.root.resolve()
            rows = ownership.load_rows(root, args.ops_root)
            no_new_manual(rows, root=root)
            print(json.dumps({"status": "PASS", "control": "no-new-manual-knowledge", "rows": len(rows)}))
            return 0
        print(json.dumps(verify(args.root, ops_root=args.ops_root), sort_keys=True))
        return 2
    except (Refused, ownership.Refused, OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"runtime deletion gate BLOCKED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
