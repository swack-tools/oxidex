"""Prepare exact-head Task19 inputs inside the restricted builder's /target mount.

The reference selection and floor policy are input evidence, never executable
plans. All path-bearing documents are generated afresh from the signed checkout.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/exiftool-tables"))
import version_rehearsal as rehearsal  # noqa: E402
import version_rehearsal_catalog as catalog_stage  # noqa: E402
import version_transition_qualification as qualification  # noqa: E402

PAIRS = ("13.59", "11.78-12.64")
RELEASES = {"13.59": ("13.59",), "11.78-12.64": ("11.78", "12.64")}
HEX = re.compile(r"[0-9a-f]{64}\Z")


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    if path.exists() or path.is_symlink():
        raise ValueError(f"qualification preparation refuses an existing document: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def logical_selection(document: dict) -> list[tuple[str, str, int]]:
    if (not isinstance(document, dict) or document.get("schema") != 1
            or document.get("kind") != "oxidex_version_rehearsal_fixture_manifest"
            or not isinstance(document.get("fixtures"), list) or not document["fixtures"]):
        raise ValueError("approved read selection has an invalid manifest")
    result = []
    names = set()
    for row in document["fixtures"]:
        if not isinstance(row, dict) or set(row) != {"path", "sha256", "bytes"}:
            raise ValueError("approved read selection has an invalid row")
        path = row["path"]
        if (not isinstance(path, str) or path.count("/t/images/") != 1
                or ".." in Path(path).parts or not isinstance(row["sha256"], str)
                or HEX.fullmatch(row["sha256"]) is None
                or type(row["bytes"]) is not int or row["bytes"] <= 0):
            raise ValueError("approved read selection has an invalid path or content identity")
        name = path.split("/t/images/", 1)[1]
        if not name or name in names:
            raise ValueError("approved read selection repeats a logical name")
        names.add(name)
        result.append((name, row["sha256"], row["bytes"]))
    return result


def rebind_carrier(row: dict, source: Path) -> dict:
    if not isinstance(row, dict) or set(row) != {"path", "sha256", "bytes"}:
        raise ValueError("reference carrier is malformed")
    raw = row["path"]
    if not isinstance(raw, str) or raw.count("/t/images/") != 1:
        raise ValueError("reference carrier has no unique t/images logical path")
    logical = raw.split("/t/images/", 1)[1]
    if not logical or any(part in {"", ".", ".."} for part in Path(logical).parts):
        raise ValueError("reference carrier escapes its image source")
    image_root = (source / "t/images").resolve(strict=True)
    path = (image_root / logical).resolve(strict=True)
    if not path.is_relative_to(image_root) or path.is_symlink() or not path.is_file():
        raise ValueError("reference carrier escapes the materialized image tree")
    if sha(path) != row["sha256"] or path.stat().st_size != row["bytes"]:
        raise ValueError(f"materialized carrier differs from approved content: {logical}")
    return {"path": str(path), "sha256": row["sha256"], "bytes": row["bytes"]}


def selected_source(materialization: dict, source_root: Path, release: str) -> Path:
    rows = [row for row in materialization["selected_releases"] if row["release"] == release]
    if len(rows) != 1 or rows[0].get("state") != "materialized":
        raise ValueError(f"materialized source is incomplete for {release}")
    source = (source_root / rows[0]["source_directory"]).resolve(strict=True)
    if not source.is_relative_to(source_root.resolve()) or not source.is_dir():
        raise ValueError("materialized source escaped its root")
    return source


def rebind_manifest(reference: Path, destination: Path, source: Path, kind: str) -> dict:
    document = json.loads(reference.read_text())
    if (not isinstance(document, dict) or set(document) != {"schema", "kind", "fixtures"}
            or document["schema"] != 1 or document["kind"] != kind
            or not isinstance(document["fixtures"], list) or not document["fixtures"]):
        raise ValueError(f"reference manifest is invalid: {reference}")
    rebound = {**document, "fixtures": [rebind_carrier(row, source) for row in document["fixtures"]]}
    if kind == "oxidex_version_rehearsal_fixture_manifest" and logical_selection(rebound) != logical_selection(document):
        raise ValueError("ordered approved read selection changed during rebinding")
    write_json(destination, rebound)
    return {"reference_sha256": sha(reference), "generated_sha256": sha(destination),
            "count": len(rebound["fixtures"])}


def rebind_cases(reference: Path, destination: Path, source: Path, approved_read: Path) -> dict:
    cases = json.loads(reference.read_text())
    if not isinstance(cases, list) or not cases:
        raise ValueError("reference native cases are missing")
    approved = json.loads(approved_read.read_text())
    selected = {name: (digest, size) for name, digest, size in logical_selection(approved)}
    rebound = []
    bindings = []
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("fixture"), str):
            raise ValueError("reference native case is malformed")
        old = Path(case["fixture"])
        if "/t/images/" not in str(old):
            raise ValueError("reference native case has no image identity")
        logical = str(old).split("/t/images/", 1)[1]
        path = (source / "t/images" / logical).resolve(strict=True)
        if not path.is_relative_to((source / "t/images").resolve()) or not path.is_file():
            raise ValueError("native case fixture escapes materialized source")
        expected = selected.get(logical)
        if expected is None or sha(path) != expected[0] or path.stat().st_size != expected[1]:
            raise ValueError("native case differs from approved carrier content")
        rebound.append({**case, "fixture": str(path)})
        bindings.append({"name": case.get("name"), "logical_name": logical,
                         "sha256": expected[0], "bytes": expected[1]})
    write_json(destination, rebound)
    return {"reference_sha256": sha(reference), "generated_sha256": sha(destination),
            "fixtures": bindings}


def prepare(reference: Path, output: Path, expected_head: str) -> dict:
    """Regenerate all six Task19 side inputs before allowing any row to start."""
    if not re.fullmatch(r"[0-9a-f]{40}", expected_head):
        raise ValueError("candidate HEAD must be a full Git object ID")
    if (subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip() != expected_head
            or subprocess.check_output(["git", "-C", str(ROOT), "status", "--porcelain"], text=True).strip()):
        raise ValueError("remote preparation requires the exact clean candidate HEAD")
    if output.exists() or output.is_symlink() or not output.is_absolute() or not output.is_relative_to(Path("/target/ops")):
        raise ValueError("new qualification output must be an unused /target/ops directory")
    if reference.is_symlink() or not reference.is_dir() or not reference.resolve().is_relative_to(Path("/src")):
        raise ValueError("approved reference must be staged below /src")
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if pin != "13.59":
        raise ValueError("this approved Task19 selection is pinned to ExifTool 13.59")
    output.mkdir(parents=True)
    policy_ref = reference / "read-policy-input.json"
    policy = output / "read-policy-input.json"
    shutil.copyfile(policy_ref, policy)
    if policy.read_bytes() != policy_ref.read_bytes():
        raise ValueError("standalone Task19 floor policy changed during transfer")
    matrix = qualification.materialize_matrix(
        qualification.load_matrix(qualification.CANONICAL_MATRIX, pin), output_root=output,
        target_root=Path("/target/targets"), run_id="preflight")
    qualification._read_policy_input(policy, {row["id"] for row in matrix["rows"]})
    evidence = {"schema": 1, "kind": "oxidex_remote_qualification_preparation", "head": expected_head,
                "pin": pin, "policy_reference_sha256": sha(policy_ref), "policy_sha256": sha(policy), "pairs": {}}
    for pair in PAIRS:
        source_ref = reference / "provisioned" / pair
        bundle = output / "provisioned" / pair
        bundle.mkdir(parents=True)
        capture = json.loads((source_ref / "capture.json").read_text())
        catalog = json.loads((source_ref / "catalog.json").read_text())
        catalog_stage.verify_capture_binding(capture, catalog)
        old_plan = json.loads((source_ref / "plan.json").read_text())
        rehearsal.verify_plan(old_plan, catalog)
        plan = rehearsal.make_plan(catalog, old_plan["seed"], old_plan["sample_index"],
                                   old_plan["pair_count"], expected_head)
        old_selection = [(side["release"], side["peeled_commit"])
                         for row in old_plan["pairs"] for side in (row["old"], row["new"])]
        new_selection = [(side["release"], side["peeled_commit"])
                         for row in plan["pairs"] for side in (row["old"], row["new"])]
        if new_selection != old_selection:
            raise ValueError("fresh exact-head plan selects different ExifTool identities")
        write_json(bundle / "capture.json", capture)
        write_json(bundle / "catalog.json", catalog)
        write_json(bundle / "plan.json", plan)
        cache = output / "provisioned" / "_cache" / pair
        sources = output / "provisioned" / "_sources" / pair
        resolution = catalog_stage.resolve_selected_archives(
            plan, catalog, capture, lambda url: catalog_stage.http_get(url, 30.0), cache)
        write_json(bundle / "resolution.json", resolution)
        materialization = catalog_stage.materialize_selected_sources(
            plan, catalog, capture, resolution, cache, sources)
        if not materialization["complete"]:
            raise ValueError(f"selected ExifTool sources could not be fully materialized for {pair}")
        write_json(bundle / "materialization.json", materialization)
        write_json(bundle / "locations.json", {"schema": 1,
                   "kind": "oxidex_version_transition_input_locations",
                   "archive_cache": str(cache), "source_root": str(sources)})
        pair_proof = {"reference_capture_sha256": sha(source_ref / "capture.json"),
                      "reference_catalog_sha256": sha(source_ref / "catalog.json"),
                      "reference_plan_sha256": sha(source_ref / "plan.json"),
                      "reference_resolution_sha256": sha(source_ref / "resolution.json"),
                      "reference_materialization_sha256": sha(source_ref / "materialization.json"),
                      "reference_locations_sha256": sha(source_ref / "locations.json"),
                      "plan_sha256": sha(bundle / "plan.json"),
                      "resolution_sha256": sha(bundle / "resolution.json"),
                      "materialization_sha256": sha(bundle / "materialization.json"), "releases": {}}
        for release in RELEASES[pair]:
            source = selected_source(materialization, sources, release)
            suffix = "" if pair == "13.59" else f"-{release}"
            read_name = f"read-fixtures{suffix}.json"
            write_name = f"write-fixtures{suffix}.json"
            cases_name = f"native-cases{suffix}.json"
            pair_proof["releases"][release] = {
                "read": rebind_manifest(source_ref / read_name, bundle / read_name, source,
                                        "oxidex_version_rehearsal_fixture_manifest"),
                "write": rebind_manifest(source_ref / write_name, bundle / write_name, source,
                                         "oxidex_version_rehearsal_write_fixture_manifest"),
                "cases": rebind_cases(source_ref / cases_name, bundle / cases_name, source,
                                      source_ref / read_name)}
        evidence["pairs"][pair] = pair_proof
    # Validate all six matrix sides and their archives, not only unique releases.
    for row in matrix["rows"]:
        for side in qualification.SIDES:
            frozen = qualification._freeze_side_inputs(row, side)
            if frozen["identity"]["documents"]["plan"].get("repository_commit") != expected_head:
                raise ValueError(f"{row['id']} {side} is not bound to the candidate HEAD")
    write_json(output / "preparation.json", evidence)
    return evidence


def verify_prepared(reference: Path, output: Path, expected_head: str) -> dict:
    """Recheck approval and fresh document bytes immediately before the first row."""
    proof = json.loads((output / "preparation.json").read_text())
    if (proof.get("schema") != 1 or proof.get("kind") != "oxidex_remote_qualification_preparation"
            or proof.get("head") != expected_head or proof.get("pin") != "13.59"
            or proof.get("policy_reference_sha256") != sha(reference / "read-policy-input.json")
            or proof.get("policy_sha256") != sha(output / "read-policy-input.json")
            or (output / "read-policy-input.json").read_bytes()
               != (reference / "read-policy-input.json").read_bytes()):
        raise ValueError("prepared candidate or standalone floor policy changed")
    for pair in PAIRS:
        pair_proof = proof["pairs"][pair]
        bundle = output / "provisioned" / pair
        approved = reference / "provisioned" / pair
        for name in ("capture", "catalog", "plan", "resolution", "materialization", "locations"):
            if sha(approved / f"{name}.json") != pair_proof[f"reference_{name}_sha256"]:
                raise ValueError(f"approved {pair} {name} source changed")
        for name in ("plan", "resolution", "materialization"):
            if sha(bundle / f"{name}.json") != pair_proof[f"{name}_sha256"]:
                raise ValueError(f"fresh {pair} {name} document changed")
        for release in RELEASES[pair]:
            suffix = "" if pair == "13.59" else f"-{release}"
            row = pair_proof["releases"][release]
            for kind, name in (("read", f"read-fixtures{suffix}.json"),
                               ("write", f"write-fixtures{suffix}.json"),
                               ("cases", f"native-cases{suffix}.json")):
                if (sha(approved / name) != row[kind]["reference_sha256"]
                        or sha(bundle / name) != row[kind]["generated_sha256"]):
                    raise ValueError(f"{pair} {release} {kind} binding changed")
            original = logical_selection(json.loads((approved / f"read-fixtures{suffix}.json").read_text()))
            current = logical_selection(json.loads((bundle / f"read-fixtures{suffix}.json").read_text()))
            if original != current:
                raise ValueError(f"{pair} {release} ordered approved read selection changed")
    return proof
