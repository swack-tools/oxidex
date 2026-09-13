#!/usr/bin/env python3
"""Offline planning and durable journaling for sampled ExifTool release rehearsals.

This module deliberately does not fetch a tag, download an archive, modify the
ExifTool pin, generate tables, build Rust, run a native oracle, or promote
anything.  It turns an already captured official tag catalog into an immutable,
seeded pair plan and an all-unrun journal.  A later runner must verify every
identity recorded here before it is allowed to perform those expensive stages.
"""
from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import os
import random
import re
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

SCHEMA = 1
SELECTOR = "python-random-mt19937-v1"
RELEASE_RE = re.compile(r"^[0-9]+\.[0-9]+$")
GIT_OID_RE = re.compile(r"^[0-9a-f]{40,64}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class Refused(ValueError):
    """The input cannot support an attributable rehearsal."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def sha256_json(value: Any) -> str:
    return hashlib.sha256(canonical_json(value)).hexdigest()


def atomic_json(path: Path, document: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(document, indent=2, sort_keys=True) + "\n"
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, prefix=f".{path.name}.", delete=False) as fh:
        fh.write(encoded)
        temp = Path(fh.name)
    try:
        os.replace(temp, path)
    finally:
        if temp.exists():
            temp.unlink()


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused(f"invalid JSON: {path}") from exc
    if not isinstance(value, dict):
        raise Refused(f"JSON object required: {path}")
    return value


def release_key(tag: str) -> tuple[int, int]:
    if not isinstance(tag, str) or not RELEASE_RE.fullmatch(tag):
        raise Refused(f"not a numeric ExifTool release: {tag!r}")
    major, minor = tag.split(".")
    return int(major), int(minor)


def _identity_reason(raw: dict[str, Any]) -> str | None:
    name = raw.get("name")
    tag_object = raw.get("tag_object")
    peeled_commit = raw.get("peeled_commit")
    archive = raw.get("archive")
    if not isinstance(tag_object, str) or not GIT_OID_RE.fullmatch(tag_object):
        return "missing_or_invalid_tag_object"
    if not isinstance(peeled_commit, str) or not GIT_OID_RE.fullmatch(peeled_commit):
        return "missing_or_invalid_peeled_commit"
    if not isinstance(archive, dict):
        return "missing_archive_identity"
    expected_url = f"https://github.com/exiftool/exiftool/archive/refs/tags/{name}.tar.gz"
    if archive.get("url") != expected_url:
        return "missing_or_invalid_archive_url"
    if not isinstance(archive.get("sha256"), str) or not SHA256_RE.fullmatch(archive["sha256"]):
        return "missing_or_invalid_archive_sha256"
    return None


def normalize_catalog(raw: dict[str, Any]) -> dict[str, Any]:
    """Normalize an offline official-tag catalog without silently dropping tags.

    Input requires an ``entries`` list. Every original entry is preserved under
    ``raw`` with either ``eligible`` or a concrete ``unclassified`` reason.
    Duplicate numeric names are deliberately unclassified rather than choosing
    whichever source happened to arrive first.
    """
    source = raw.get("catalog_source")
    pages = source.get("pages") if isinstance(source, dict) else None
    if (not isinstance(source, dict) or source.get("kind") != "official_exiftool_tag_catalog"
            or not isinstance(pages, list) or not pages
            or any(not isinstance(page, dict) or not isinstance(page.get("url"), str)
                   or not page["url"].startswith("https://api.github.com/repos/exiftool/exiftool/tags")
                   or not isinstance(page.get("sha256"), str) or not SHA256_RE.fullmatch(page["sha256"])
                   for page in pages)
            or not isinstance(raw.get("captured_at"), str)):
        raise Refused("catalog_source must identify an official ExifTool tag catalog")
    entries = raw.get("entries")
    if not isinstance(entries, list):
        raise Refused("catalog requires an entries list")
    names: dict[str, int] = {}
    for entry in entries:
        if isinstance(entry, dict) and isinstance(entry.get("name"), str) and RELEASE_RE.fullmatch(entry["name"]):
            names[entry["name"]] = names.get(entry["name"], 0) + 1
    normalized: list[dict[str, Any]] = []
    for index, raw_entry in enumerate(entries):
        if not isinstance(raw_entry, dict):
            normalized.append({"source_index": index, "raw": raw_entry, "classification": {"state": "unclassified", "reason": "entry_not_object"}})
            continue
        name = raw_entry.get("name")
        if not isinstance(name, str) or not RELEASE_RE.fullmatch(name):
            classification = {"state": "excluded", "reason": "tag_name_not_numeric_release"}
        elif names[name] > 1:
            classification = {"state": "unclassified", "reason": "ambiguous_duplicate_release"}
        else:
            reason = _identity_reason(raw_entry)
            classification = {"state": "eligible"} if reason is None else {"state": "unclassified", "reason": reason}
        normalized.append({"source_index": index, "raw": raw_entry, "classification": classification})
    payload = {
        "schema": SCHEMA,
        "catalog_source": source,
        "captured_at": raw.get("captured_at"),
        "entries": normalized,
    }
    return {**payload, "catalog_sha256": sha256_json(payload)}


def verify_catalog(catalog: dict[str, Any]) -> None:
    if catalog.get("schema") != SCHEMA or not isinstance(catalog.get("entries"), list):
        raise Refused("unsupported normalized catalog")
    expected = catalog.get("catalog_sha256")
    payload = {k: v for k, v in catalog.items() if k != "catalog_sha256"}
    if not isinstance(expected, str) or expected != sha256_json(payload):
        raise Refused("catalog identity changed or is malformed")


def eligible_releases(catalog: dict[str, Any]) -> list[dict[str, Any]]:
    verify_catalog(catalog)
    releases = [entry["raw"] for entry in catalog["entries"] if entry["classification"]["state"] == "eligible"]
    releases.sort(key=lambda entry: release_key(entry["name"]))
    if len(releases) < 2:
        raise Refused("at least two fully identified numeric releases are required")
    return releases


def parse_seed(value: str) -> int:
    try:
        seed = int(value, 10)
    except ValueError as exc:
        raise Refused("seed must be an unsigned decimal integer") from exc
    if not 0 <= seed < 2**64:
        raise Refused("seed must fit unsigned 64-bit range")
    return seed


def select_pairs(catalog: dict[str, Any], seed: int, sample_index: int, pair_count: int) -> list[tuple[dict[str, Any], dict[str, Any]]]:
    if sample_index < 0 or pair_count < 1:
        raise Refused("sample index must be nonnegative and pair count positive")
    releases = eligible_releases(catalog)
    pairs = list(itertools.combinations(releases, 2))
    if pair_count > len(pairs):
        raise Refused(f"pair count {pair_count} exceeds {len(pairs)} available unordered pairs")
    rng = random.Random(f"{SELECTOR}:{seed}:{sample_index}")
    selected = rng.sample(pairs, pair_count)
    # itertools combinations has each pair in increasing release order; verify
    # it rather than relying on implementation details in a future refactor.
    for old, new in selected:
        if release_key(old["name"]) >= release_key(new["name"]):
            raise Refused("selected same-version or reversed pair")
    return selected


def _release_identity(entry: dict[str, Any]) -> dict[str, Any]:
    archive = entry["archive"]
    return {
        "release": entry["name"],
        "tag_object": entry["tag_object"],
        "peeled_commit": entry["peeled_commit"],
        "archive": {"url": archive["url"], "sha256": archive["sha256"]},
    }


def make_plan(catalog: dict[str, Any], seed: int, sample_index: int, pair_count: int, repository_commit: str) -> dict[str, Any]:
    verify_catalog(catalog)
    if not isinstance(repository_commit, str) or not GIT_OID_RE.fullmatch(repository_commit):
        raise Refused("repository commit must be a full git object id")
    selected = select_pairs(catalog, seed, sample_index, pair_count)
    chosen_names = {entry["name"] for pair in selected for entry in pair}
    untested = []
    for entry in catalog["entries"]:
        raw = entry["raw"]
        if entry["classification"]["state"] == "eligible" and raw["name"] not in chosen_names:
            untested.append({"release": raw["name"], "reason": "not_selected_by_seeded_pair_plan"})
    payload = {
        "schema": SCHEMA,
        "kind": "oxidex_exiftool_version_rehearsal_plan",
        "selector": SELECTOR,
        "repository_commit": repository_commit,
        "catalog_sha256": catalog["catalog_sha256"],
        "seed": seed,
        "sample_index": sample_index,
        "pair_count": pair_count,
        "pairs": [
            {"pair_index": index, "old": _release_identity(old), "new": _release_identity(new)}
            for index, (old, new) in enumerate(selected)
        ],
        "untested_eligible_releases": sorted(untested, key=lambda row: release_key(row["release"])),
        "catalog_noneligible_entries": [
            {"source_index": entry["source_index"], "name": entry["raw"].get("name") if isinstance(entry["raw"], dict) else None,
             "classification": entry["classification"]}
            for entry in catalog["entries"] if entry["classification"]["state"] != "eligible"
        ],
        "execution": {
            "state": "plan_only",
            "read": "unrun",
            "write": "unrun",
            "explicit_limit": "selection is not native read/write proof or upgrade success",
        },
    }
    return {**payload, "plan_sha256": sha256_json(payload)}


def verify_plan(plan: dict[str, Any], catalog: dict[str, Any] | None = None) -> None:
    if plan.get("schema") != SCHEMA or plan.get("kind") != "oxidex_exiftool_version_rehearsal_plan":
        raise Refused("unsupported run plan")
    expected = plan.get("plan_sha256")
    payload = {k: v for k, v in plan.items() if k != "plan_sha256"}
    if not isinstance(expected, str) or expected != sha256_json(payload):
        raise Refused("plan identity changed or is malformed")
    eligible_by_release: dict[str, dict[str, Any]] | None = None
    if catalog is not None:
        verify_catalog(catalog)
        if plan.get("catalog_sha256") != catalog["catalog_sha256"]:
            raise Refused("catalog identity differs from recorded plan")
        eligible_by_release = {entry["raw"]["name"]: entry["raw"] for entry in catalog["entries"]
                               if entry["classification"]["state"] == "eligible"}
    seen: set[str] = set()
    for pair in plan.get("pairs", []):
        if not isinstance(pair, dict):
            raise Refused("malformed pair")
        old, new = pair.get("old"), pair.get("new")
        if not isinstance(old, dict) or not isinstance(new, dict):
            raise Refused("pair lacks release identities")
        for release in (old, new):
            # Reapply the same exact requirements before future execution.
            reason = _identity_reason({"name": release.get("release"), **release})
            if not isinstance(release.get("release"), str) or not RELEASE_RE.fullmatch(release["release"]) or reason:
                raise Refused("pair has missing or ambiguous release identity")
            if eligible_by_release is not None:
                actual = eligible_by_release.get(release["release"])
                expected = {"release": actual["name"], "tag_object": actual["tag_object"],
                            "peeled_commit": actual["peeled_commit"],
                            "archive": {"url": actual["archive"]["url"], "sha256": actual["archive"]["sha256"]}} if actual else None
                if expected != release:
                    raise Refused("pair release identity differs from catalog")
        if release_key(old["release"]) >= release_key(new["release"]):
            raise Refused("pair must have distinct old/new releases in numeric order")
        pair_key = f"{old['release']}->{new['release']}"
        if pair_key in seen:
            raise Refused("duplicate pair in plan")
        seen.add(pair_key)


def _run_id(plan: dict[str, Any]) -> str:
    return f"rehearsal-{plan['plan_sha256'][:16]}"


def initial_journal(plan: dict[str, Any]) -> dict[str, Any]:
    verify_plan(plan)
    variants: dict[str, dict[str, str]] = {}
    for pair in plan["pairs"]:
        for release in (pair["old"], pair["new"]):
            variants.setdefault(release["release"], {"read": "unrun", "write": "unrun", "state": "unrun"})
    return {
        "schema": SCHEMA,
        "kind": "oxidex_exiftool_version_rehearsal_journal",
        "run_id": _run_id(plan),
        "plan_sha256": plan["plan_sha256"],
        "phase": "planned",
        "active": None,
        "events": [{"event": "plan_created", "at_unix": time.time()}],
        "pairs": [{"pair_index": pair["pair_index"], "state": "unrun", "failure": None} for pair in plan["pairs"]],
        "releases": variants,
        "untested_eligible_releases": plan["untested_eligible_releases"],
        "execution_limit": "all read/write states are unrun; planning is not proof",
    }


def create_run(plan: dict[str, Any], catalog: dict[str, Any], run_dir: Path) -> tuple[Path, Path]:
    verify_plan(plan, catalog)
    try:
        run_dir.mkdir(parents=True, exist_ok=False)
    except FileExistsError as exc:
        raise Refused(f"run output already exists: {run_dir}") from exc
    catalog_path = run_dir / "catalog.normalized.json"
    plan_path = run_dir / "run-plan.json"
    journal_path = run_dir / "status.json"
    atomic_json(catalog_path, catalog)
    atomic_json(plan_path, plan)
    atomic_json(journal_path, initial_journal(plan))
    return plan_path, journal_path


def load_verified_run(run_dir: Path, catalog: dict[str, Any] | None = None) -> tuple[dict[str, Any], dict[str, Any]]:
    if catalog is None:
        catalog = read_json(run_dir / "catalog.normalized.json")
    plan = read_json(run_dir / "run-plan.json")
    verify_plan(plan, catalog)
    journal = read_json(run_dir / "status.json")
    if journal.get("schema") != SCHEMA or journal.get("kind") != "oxidex_exiftool_version_rehearsal_journal":
        raise Refused("unsupported journal")
    if journal.get("plan_sha256") != plan["plan_sha256"] or journal.get("run_id") != _run_id(plan):
        raise Refused("journal does not belong to immutable plan")
    if journal.get("phase") not in {"planned", "running", "interrupted", "failed"}:
        raise Refused("journal has unsupported phase")
    for release, state in journal.get("releases", {}).items():
        if not isinstance(release, str) or not isinstance(state, dict):
            raise Refused("journal has malformed release state")
        if state.get("read") not in {"unrun", "failed"} or state.get("write") not in {"unrun", "failed"}:
            raise Refused("planning journal cannot claim read/write success")
        expected_state = "failed" if "failed" in {state.get("read"), state.get("write")} else "unrun"
        if state.get("state") != expected_state:
            raise Refused("journal release aggregate does not match read/write states")
    for pair in journal.get("pairs", []):
        if not isinstance(pair, dict) or pair.get("state") not in {"unrun", "failed"}:
            raise Refused("planning journal cannot claim pair success")
    return plan, journal


def start_pair(run_dir: Path, pair_index: int) -> dict[str, Any]:
    """Record an active pair for a future runner; it never marks a pass."""
    plan, journal = load_verified_run(run_dir)
    if journal["phase"] not in {"planned", "interrupted"}:
        raise Refused("journal is not available to start a pair")
    if not any(item["pair_index"] == pair_index and item["state"] == "unrun" for item in journal["pairs"]):
        raise Refused("requested unrun pair not found")
    journal["phase"] = "running"
    journal["active"] = {"pair_index": pair_index, "stage": "not_started"}
    journal["events"].append({"event": "pair_started", "pair_index": pair_index, "at_unix": time.time()})
    atomic_json(run_dir / "status.json", journal)
    return journal


def record_failure(run_dir: Path, pair_index: int, release: str, operation: str, detail: str) -> dict[str, Any]:
    """Record an observed future-run failure without allowing a synthetic pass."""
    if operation not in {"read", "write"}:
        raise Refused("failure operation must be read or write")
    plan, journal = load_verified_run(run_dir)
    if journal["phase"] not in {"running", "interrupted"}:
        raise Refused("only a running or recovered pair can record a failure")
    if release not in journal["releases"]:
        raise Refused("release is not selected by this run")
    journal["releases"][release][operation] = "failed"
    journal["releases"][release]["state"] = "failed"
    selected_pair = next((pair for pair in plan["pairs"] if pair["pair_index"] == pair_index), None)
    if selected_pair is None or release not in {selected_pair["old"]["release"], selected_pair["new"]["release"]}:
        raise Refused("release does not belong to selected pair")
    for pair in journal["pairs"]:
        if pair["pair_index"] == pair_index:
            pair.update(state="failed", failure={"release": release, "operation": operation, "detail": detail})
            break
    else:
        raise Refused("pair is not selected by this run")
    journal["phase"] = "failed"
    journal["active"] = None
    journal["events"].append({"event": "failure", "pair_index": pair_index, "release": release, "operation": operation, "detail": detail, "at_unix": time.time()})
    atomic_json(run_dir / "status.json", journal)
    return journal


def recover_interrupted(run_dir: Path, catalog: dict[str, Any] | None = None) -> dict[str, Any]:
    """Close an interrupted pre-execution journal while preserving all unrun states."""
    _, journal = load_verified_run(run_dir, catalog)
    if journal["phase"] != "running" or not isinstance(journal.get("active"), dict):
        raise Refused("only an active interrupted journal can be recovered")
    active = journal["active"]
    journal["phase"] = "interrupted"
    journal["active"] = None
    journal["events"].append({"event": "interrupted_recovered", "previous_active": active, "at_unix": time.time()})
    atomic_json(run_dir / "status.json", journal)
    return journal


def _cmd_plan(args: argparse.Namespace) -> int:
    catalog = normalize_catalog(read_json(Path(args.catalog)))
    seed = parse_seed(args.seed)
    plan = make_plan(catalog, seed, args.sample_index, args.pair_count, args.repository_commit)
    run_dir = Path(args.run_dir).resolve()
    plan_path, status_path = create_run(plan, catalog, run_dir)
    print(json.dumps({"run_id": _run_id(plan), "plan": str(plan_path), "status": str(status_path), "selected_pairs": len(plan["pairs"]), "read": "unrun", "write": "unrun"}, sort_keys=True))
    return 0


def _cmd_recover(args: argparse.Namespace) -> int:
    run_dir = Path(args.run_dir).resolve()
    catalog = normalize_catalog(read_json(Path(args.catalog))) if args.catalog else None
    journal = recover_interrupted(run_dir, catalog)
    print(json.dumps({"run_id": journal["run_id"], "phase": journal["phase"], "read": "unrun", "write": "unrun"}, sort_keys=True))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    plan = sub.add_parser("plan", help="write an immutable plan and all-unrun journal; performs no remote or build work")
    plan.add_argument("--catalog", required=True, help="offline captured official-tag catalog JSON")
    plan.add_argument("--seed", required=True, help="unsigned 64-bit decimal seed")
    plan.add_argument("--sample-index", type=int, default=0)
    plan.add_argument("--pair-count", type=int, default=1)
    plan.add_argument("--repository-commit", required=True)
    plan.add_argument("--run-dir", required=True, help="must not already exist")
    plan.set_defaults(func=_cmd_plan)
    recover = sub.add_parser("recover", help="preserve an interrupted planning journal as interrupted")
    recover.add_argument("--run-dir", required=True)
    recover.add_argument("--catalog", help="optional normalized-input check before recovery")
    recover.set_defaults(func=_cmd_recover)
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except Refused as exc:
        print(f"version rehearsal refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
