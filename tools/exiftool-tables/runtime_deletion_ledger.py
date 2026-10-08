#!/usr/bin/env python3
"""Fail-closed Task18 deletion chronology and no-new-manual-knowledge gate.

This records historical changes without retroactively granting their missing
pre-deletion approval. A historical output-parity receipt is not authorization.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import runtime_ownership as ownership

LEDGER = Path("docs/reference/generated-runtime-deletion-ledger.json")
INTEGRATION_COMMIT = "4f653243d33cf053107f4f19b8bd449329de611b"
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


def no_new_manual(rows: list[dict]) -> None:
    ownership.verify_rows(rows)
    generated = {(row["module"], row["table"], row["field"]["value"].split("/")[-1])
                 for row in rows if row["owner"] == "generated"}
    for row in rows:
        if row["owner"] != "residual":
            continue
        identity = row["field"]["value"]
        if (row["module"], row["table"], identity.split("/")[-1]) not in generated:
            continue
        if row["module"] != "Exif" or row["table"] != "Main" or (
                identity, row["symbol"], row.get("residual_disposition")) not in KNOWN_GENERATED_RESIDUALS:
            raise Refused(f"new manual owner for generated source construct: {row['module']}::{row['table']}:{identity}")


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True,
                                   stderr=subprocess.PIPE, timeout=30).strip()


def verify_history(root: Path) -> str:
    if git(root, "status", "--porcelain=v1", "--untracked-files=all"):
        raise Refused("deletion gate source must be clean")
    head = git(root, "rev-parse", "HEAD")
    if git(root, "write-tree") != git(root, "rev-parse", "HEAD^{tree}"):
        raise Refused("deletion gate index differs from committed tree")
    # The five development commits were squash-merged as PR #940. They are
    # evidence references, not ancestors of the integration branch.
    if subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor", INTEGRATION_COMMIT, head],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30).returncode != 0:
        raise Refused("Task18 squash merge is not in candidate lineage")
    return head


def verify(root: Path, *, ops_root: Path | None = None) -> dict:
    root = root.resolve()
    document = validate_document(json.loads((root / LEDGER).read_text(encoding="utf-8")))
    head = verify_history(root)
    rows = ownership.load_rows(root, ops_root)
    no_new_manual(rows)
    return {"status": "BLOCKED", "candidate_head": head,
            "historical_unqualified": len(document["historical_changes"]),
            "retained": sum(group["count"] for group in document["retained_groups"]),
            "missing": ["approved finite appendix authenticated before each deletion",
                        "controller reconciliation and receipt attestations",
                        "current source/binary and generated-on/off attribution receipts",
                        "zero-reachability and oracle-capability receipts"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("verify", "no-new-manual"))
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--ops-root", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "no-new-manual":
            rows = ownership.load_rows(args.root.resolve(), args.ops_root)
            no_new_manual(rows)
            print(json.dumps({"status": "PASS", "control": "no-new-manual-knowledge", "rows": len(rows)}))
            return 0
        print(json.dumps(verify(args.root, ops_root=args.ops_root), sort_keys=True))
        return 2
    except (Refused, ownership.Refused, OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"runtime deletion gate BLOCKED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
