#!/usr/bin/env python3
"""Prepare a commit-bound, fail-closed beta.1 benchmark evidence bundle.

This tool is deliberately a preflight wrapper.  It never invokes the
benchmark script and writes only a new, explicitly named evidence directory.
The result validator is kept separate so a historical benchmark result cannot
be mistaken for a measurement of the frozen candidate.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


EXPECTED_CANDIDATE_SHA = "32aaf737339ef3020d35128accda217990f1350b"
EXPECTED_EXIFTOOL_VERSION = "13.59"
EXPECTED_CORPUS_COUNT = 194
EXPECTED_WARMUPS = 5
EXPECTED_RUNS = 30
EXPECTED_SCENARIOS = (
    "single_file",
    "single_canon",
    "batch",
    "write",
    "detection",
    "corpus",
    "corpus_1thread",
)
RUN_ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{2,96}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class Refused(ValueError):
    """An input failed a release-evidence boundary."""


def _canonical_json(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def build_corpus_manifest(root: Path, *, expected_count: int = EXPECTED_CORPUS_COUNT) -> dict[str, Any]:
    """Hash the exact flat corpus; symlinks and count drift are refusals."""
    root = Path(root)
    if not root.is_absolute():
        raise Refused("corpus path must be absolute")
    if root.is_symlink():
        raise Refused("corpus root is a symlink")
    if not root.is_dir():
        raise Refused(f"corpus directory is missing: {root}")

    children = sorted(root.iterdir(), key=lambda path: path.name)
    rows: list[dict[str, Any]] = []
    for path in children:
        if path.is_symlink():
            raise Refused(f"corpus contains symlink: {path}")
        if not path.is_file():
            raise Refused(f"corpus must be flat regular files: {path}")
        rows.append({"path": path.name, "bytes": path.stat().st_size, "sha256": sha256_file(path)})
    if len(rows) != expected_count:
        raise Refused(f"corpus count {len(rows)} != expected {expected_count}")
    if [row["path"] for row in rows] != sorted(row["path"] for row in rows):
        raise Refused("corpus manifest is not sorted")

    body = {"file_count": len(rows), "files": rows}
    return {"root": str(root), **body, "manifest_sha256": hashlib.sha256(_canonical_json(body)).hexdigest()}


def build_tree_manifest(root: Path) -> dict[str, Any]:
    """Hash a pinned source tree without following any symlink."""
    root = Path(root)
    if root.is_symlink() or not root.is_dir():
        raise Refused(f"source tree is missing or symlinked: {root}")
    rows: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*"), key=lambda candidate: candidate.relative_to(root).as_posix()):
        if path.is_symlink():
            raise Refused(f"source tree contains symlink: {path}")
        if path.is_file():
            rows.append({"path": path.relative_to(root).as_posix(), "bytes": path.stat().st_size,
                         "sha256": sha256_file(path)})
        elif not path.is_dir():
            raise Refused(f"source tree contains a non-regular entry: {path}")
    body = {"file_count": len(rows), "files": rows}
    return {**body, "manifest_sha256": hashlib.sha256(_canonical_json(body)).hexdigest()}


def snapshot_corpus(source: Path, destination: Path) -> None:
    """Materialize an immutable evidence-local corpus snapshot."""
    if destination.exists() or destination.is_symlink():
        raise Refused(f"corpus snapshot already exists: {destination}")
    destination.mkdir(parents=True)
    for row in build_corpus_manifest(source)["files"]:
        source_file = Path(source) / row["path"]
        target_file = destination / row["path"]
        shutil.copyfile(source_file, target_file)
        target_file.chmod(0o444)
    destination.chmod(0o555)


def require_new_run_directory(evidence_dir: Path, run_id: str, repository: Path) -> Path:
    """Return a fresh run path without creating or reusing any prior run."""
    evidence_dir = Path(evidence_dir)
    repository = Path(repository).resolve()
    if not evidence_dir.is_absolute():
        raise Refused("evidence directory must be absolute")
    if not RUN_ID_RE.fullmatch(run_id) or "historical" in run_id.lower():
        raise Refused("run_id is invalid or historical")
    run_dir = evidence_dir / run_id
    try:
        run_dir.resolve().relative_to(repository)
    except ValueError:
        pass
    else:
        raise Refused("evidence run must be outside the repository")
    if run_dir.exists() or run_dir.is_symlink():
        raise Refused(f"evidence run already exists: {run_dir}")
    return run_dir


def _require_sha(value: str, label: str) -> str:
    if not SHA256_RE.fullmatch(value):
        raise Refused(f"{label} must be a lowercase SHA-256")
    return value


def validate_binary_identity(binary: Path, expected_sha256: str, target_dir: Path, repository: Path) -> dict[str, str]:
    """Require an explicit executable in a run-specific release target."""
    binary = Path(binary)
    target_dir = Path(target_dir)
    repository = Path(repository).resolve()
    if not binary.is_absolute() or not target_dir.is_absolute():
        raise Refused("binary and target directory must be absolute")
    if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
        raise Refused(f"release binary is not a regular executable: {binary}")
    if target_dir.resolve() == repository or target_dir.resolve().is_relative_to(repository):
        raise Refused("release target directory must be outside the repository")
    expected_path = target_dir / "release" / "oxidex"
    if binary.resolve() != expected_path.resolve():
        raise Refused("binary must be the explicit target-dir/release/oxidex")
    expected_sha256 = _require_sha(expected_sha256, "binary SHA-256")
    actual = sha256_file(binary)
    if actual != expected_sha256:
        raise Refused("release binary SHA-256 does not match the supplied identity")
    return {"path": str(binary), "sha256": actual}


def _run(command: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, check=False)
    if completed.returncode:
        detail = (completed.stderr or completed.stdout).strip()
        raise Refused(f"command failed ({completed.returncode}): {' '.join(command)}: {detail}")
    return completed.stdout.strip()


def git_identity(repository: Path, candidate_sha: str) -> dict[str, Any]:
    if candidate_sha != EXPECTED_CANDIDATE_SHA:
        raise Refused("requested candidate SHA is not the frozen beta.1 candidate")
    commit = _run(["git", "rev-parse", "HEAD"], cwd=repository)
    dirty = bool(_run(["git", "status", "--porcelain=v1"], cwd=repository))
    if commit != EXPECTED_CANDIDATE_SHA:
        raise Refused(f"candidate SHA {commit} != frozen candidate SHA {EXPECTED_CANDIDATE_SHA}")
    if dirty:
        raise Refused("candidate working tree is dirty")
    return {"sha": commit, "dirty": False}


def cargo_identity(repository: Path) -> dict[str, str]:
    with (repository / "Cargo.toml").open("rb") as stream:
        package = tomllib.load(stream)["package"]
    cargo_version = _run(["cargo", "-V"], cwd=repository)
    return {"package_version": str(package["version"]), "cargo_version": cargo_version}


def _clean_oracle_environment() -> dict[str, str]:
    environment = os.environ.copy()
    for name in ("EXIFTOOL", "PERL5LIB", "PERLLIB", "PERL5OPT"):
        environment.pop(name, None)
    environment["LC_ALL"] = "C"
    environment["TZ"] = "UTC"
    return environment


def oracle_identity(perl: Path, exiftool_dir: Path, docx: Path, repository: Path) -> dict[str, Any]:
    """Probe the pinned source tree through the caller's canonical Perl."""
    perl = Path(perl)
    exiftool_dir = Path(exiftool_dir)
    docx = Path(docx)
    if not perl.is_absolute() or perl.is_symlink() or not perl.is_file() or not os.access(perl, os.X_OK):
        raise Refused("canonical Perl must be an explicit regular executable")
    perl = perl.resolve()
    script = exiftool_dir / "exiftool"
    library = exiftool_dir / "lib"
    if (not exiftool_dir.is_absolute() or exiftool_dir.is_symlink() or script.is_symlink()
            or not script.is_file() or not library.is_dir()):
        raise Refused("ExifTool source tree is incomplete or symlinked")
    if not docx.is_absolute() or docx.is_symlink() or not docx.is_file():
        raise Refused("DOCX capability input must be an explicit regular file")
    pinned = (repository / ".exiftool-version").read_text(encoding="utf-8").strip()
    if pinned != EXPECTED_EXIFTOOL_VERSION:
        raise Refused(f"repository ExifTool pin is {pinned!r}, expected {EXPECTED_EXIFTOOL_VERSION}")
    environment = _clean_oracle_environment()
    _run([str(perl), "-MArchive::Zip", "-e", "1"], env=environment)
    version = _run([str(perl), "-I", str(library), str(script), "-ver"], env=environment)
    if version != pinned:
        raise Refused(f"ExifTool version {version!r} != pinned {pinned!r}")
    capability = _run([str(perl), "-I", str(library), str(script), "-FileType", "-s3", str(docx)], env=environment)
    if capability != "DOCX":
        raise Refused(f"DOCX capability probe returned {capability!r}")
    return {
        "version": version,
        "perl": str(perl),
        "archive_zip": True,
        "docx_probe": capability,
        "exiftool_dir": str(exiftool_dir.resolve()),
        "source_manifest_sha256": build_tree_manifest(exiftool_dir)["manifest_sha256"],
        "docx": str(docx),
    }


def validate_result_document(
    document: dict[str, Any],
    *,
    candidate_sha: str,
    binary_path: Path,
    binary_sha256: str,
    cargo_version: str,
    exiftool_version: str,
    corpus_manifest: dict[str, Any],
    warmups: int,
    runs: int,
    cache_policy: str = "warm-cache",
    expected_corpus_count: int = EXPECTED_CORPUS_COUNT,
) -> dict[str, Any]:
    """Validate existing hyperfine JSON without writing or executing anything."""
    if candidate_sha != EXPECTED_CANDIDATE_SHA:
        raise Refused("candidate SHA is not the frozen beta.1 candidate")
    if exiftool_version != EXPECTED_EXIFTOOL_VERSION:
        raise Refused("result validation requires the pinned ExifTool version")
    if cache_policy != "warm-cache":
        raise Refused("release qualification only accepts the explicit warm-cache label")
    if not Path(binary_path).is_absolute():
        raise Refused("result validation requires an absolute release binary path")
    instrument = document.get("instrument")
    if not isinstance(instrument, dict):
        raise Refused("benchmark result has no instrument identity")
    if instrument.get("commit") != candidate_sha:
        raise Refused("benchmark result candidate SHA is stale")
    if instrument.get("dirty") is not False:
        raise Refused("benchmark result was produced from a dirty tree")
    if instrument.get("staleness_note"):
        raise Refused("benchmark result carries a binary staleness note")
    if instrument.get("oxidex") != str(binary_path):
        raise Refused("benchmark result uses a different release binary path")
    if instrument.get("oxidex_sha256") != binary_sha256:
        raise Refused("benchmark result uses a different release binary hash")
    if instrument.get("oxidex_version") != f"oxidex {cargo_version}":
        raise Refused("benchmark result uses a different Cargo package version")
    if instrument.get("exiftool_version") != exiftool_version:
        raise Refused("benchmark result uses a different ExifTool pin")
    if warmups != EXPECTED_WARMUPS or runs != EXPECTED_RUNS:
        raise Refused("release qualification requires five warmups and 30 timed runs")
    if corpus_manifest.get("file_count") != expected_corpus_count:
        raise Refused(f"result validation requires the exact {expected_corpus_count}-file corpus manifest")

    actual_scenarios = tuple(key for key in document if key != "instrument")
    if set(actual_scenarios) != set(EXPECTED_SCENARIOS) or len(actual_scenarios) != len(EXPECTED_SCENARIOS):
        raise Refused("benchmark result does not contain exactly the expected seven scenarios")

    expected_corpus_paths = [str(Path(corpus_manifest["root"]) / row["path"]) for row in corpus_manifest["files"]]
    row_count = 0
    for name in EXPECTED_SCENARIOS:
        scenario = document.get(name)
        rows = scenario.get("results") if isinstance(scenario, dict) else None
        if not isinstance(rows, list) or len(rows) != 2:
            raise Refused(f"scenario {name} must contain exactly two result rows")
        candidate_rows = 0
        for row in rows:
            if not isinstance(row, dict) or not isinstance(row.get("command"), str):
                raise Refused(f"scenario {name} contains a malformed result row")
            if str(binary_path) in row["command"]:
                candidate_rows += 1
            if not isinstance(row.get("times"), list) or len(row["times"]) != runs:
                raise Refused(f"scenario {name} does not contain 30 timed samples")
            if row.get("exit_codes") != [0] * runs:
                raise Refused(f"scenario {name} contains a failed timed command")
            if name in ("corpus", "corpus_1thread") and not all(path in row["command"] for path in expected_corpus_paths):
                raise Refused(f"scenario {name} does not use every manifest file")
            row_count += 1
        if candidate_rows != 1:
            raise Refused(f"scenario {name} does not identify exactly one candidate command")
    return {
        "benchmark_status": "observed-input-validated",
        "scenario_count": len(EXPECTED_SCENARIOS),
        "result_row_count": row_count,
        "warmups": warmups,
        "timed_runs_per_row": runs,
    }


def _machine_identity() -> dict[str, Any]:
    try:
        memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    except (AttributeError, OSError, ValueError):
        memory = "unverified"
    try:
        affinity: Any = sorted(os.sched_getaffinity(0))
    except (AttributeError, OSError):
        affinity = "unverified"
    return {
        "host": platform.node() or "unverified",
        "os": f"{platform.system()} {platform.release()} {platform.machine()}",
        "cpu": platform.processor() or "unverified",
        "logical_cores": os.cpu_count() or "unverified",
        "memory_bytes": memory,
        "affinity": affinity,
        "load": list(os.getloadavg()) if hasattr(os, "getloadavg") else "unverified",
        "power_thermal_state": "unverified",
        "cache_policy": "warm-cache; no cold-cache claim",
    }


def build_unrun_receipt(
    *, run_id: str, evidence_path: Path, candidate: dict[str, Any], binary: dict[str, Any],
    cargo: dict[str, str], oracle: dict[str, Any], corpus: dict[str, Any], machine: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": "oxidex.frozen-candidate-benchmark-qualification/v1",
        "status": "unverified",
        "benchmark_status": "not_run",
        "statement": "This preflight did not run a benchmark and contains no timing result.",
        "candidate": candidate,
        "binary": binary,
        "cargo": cargo,
        "oracle": oracle,
        "corpus": corpus,
        "machine": machine,
        "sampling": {"warmups": EXPECTED_WARMUPS, "timed_runs_per_row": EXPECTED_RUNS},
        "scenarios": list(EXPECTED_SCENARIOS),
        "cache_policy": "warm-cache; no cold-cache claim",
        "run_id": run_id,
        "evidence_path": str(evidence_path),
        "measured_at": None,
        "historical_results_untouched": True,
        "unresolved": ["benchmark not run; current candidate timing evidence is not present"],
    }


def prepare(args: argparse.Namespace) -> Path:
    repository = Path(args.repository).resolve()
    git = git_identity(repository, args.candidate_sha)
    cargo = cargo_identity(repository)
    if cargo["package_version"] != "2.0.0-beta.1":
        raise Refused(f"Cargo package version is {cargo['package_version']!r}, expected beta.1")
    binary = validate_binary_identity(Path(args.binary), args.binary_sha256, Path(args.target_dir), repository)
    version = _run([binary["path"], "--version"])
    if version != f"oxidex {cargo['package_version']}":
        raise Refused(f"release binary reports {version!r}, expected oxidex {cargo['package_version']!r}")
    oracle = oracle_identity(Path(args.perl), Path(args.exiftool_dir), Path(args.docx), repository)
    source_manifest = build_corpus_manifest(Path(args.corpus))
    run_dir = require_new_run_directory(Path(args.evidence_dir), args.run_id, repository)
    run_dir.mkdir(parents=True)
    snapshot_corpus(Path(args.corpus), run_dir / "corpus")
    manifest = build_corpus_manifest(run_dir / "corpus")
    if manifest["files"] != source_manifest["files"]:
        raise Refused("corpus changed while creating the evidence snapshot")
    (run_dir / "corpus-manifest.json").write_bytes(_canonical_json(manifest))
    receipt = build_unrun_receipt(
        run_id=args.run_id,
        evidence_path=run_dir,
        candidate=git,
        binary={**binary, "version": version, "target_dir": str(Path(args.target_dir))},
        cargo=cargo,
        oracle=oracle,
        corpus=manifest,
        machine=_machine_identity(),
    )
    (run_dir / "qualification.json").write_bytes(_canonical_json(receipt))
    return run_dir


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    for name, help_text in (
        ("repository", "clean frozen candidate checkout"),
        ("candidate-sha", "full frozen candidate commit SHA"),
        ("binary", "explicit fresh release binary"),
        ("binary-sha256", "SHA-256 of the explicit release binary"),
        ("target-dir", "fresh target directory used for the release build"),
        ("perl", "canonical capable Perl executable"),
        ("exiftool-dir", "pinned ExifTool source directory"),
        ("docx", "DOCX capability probe input"),
        ("corpus", "immutable 13.59 t/images corpus snapshot"),
        ("evidence-dir", "new evidence directory outside the repository"),
        ("run-id", "unique non-historical run identity"),
    ):
        result.add_argument(f"--{name}", required=True, help=help_text)
    return result


def main(argv: list[str] | None = None) -> int:
    try:
        run_dir = prepare(parser().parse_args(argv))
    except (OSError, Refused, KeyError, tomllib.TOMLDecodeError) as exc:
        print(f"refused: {exc}", file=sys.stderr)
        return 2
    print(f"prepared unrun qualification evidence: {run_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
