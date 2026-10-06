#!/usr/bin/env python3
"""Provision the locked Linux oracle, then run every workspace test on Spot."""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", "/target"))


def file_sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def pinned_rust_identity(pin: str, expected_commit: str) -> tuple[str, str]:
    """Recheck the compiler immediately before Cargo, including rustup's pin."""
    pinned_rustc = subprocess.check_output(
        ["rustup", "which", "--toolchain", pin, "rustc"], text=True).strip()
    pinned_cargo = subprocess.check_output(
        ["rustup", "which", "--toolchain", pin, "cargo"], text=True).strip()
    active_rustc = subprocess.check_output(["rustc", "-vV"], text=True)
    rustup_rustc = subprocess.check_output([pinned_rustc, "-vV"], text=True)
    cargo_rustc = subprocess.check_output([os.environ.get("RUSTC", "rustc"), "-vV"], text=True)
    active_cargo = subprocess.check_output(["cargo", "-V"], text=True).strip()
    rustup_cargo = subprocess.check_output([pinned_cargo, "-V"], text=True).strip()
    commit_pattern = r"^commit-hash: ([0-9a-f]{40})$"
    active_commit = re.search(commit_pattern, active_rustc, re.M)
    rustup_commit = re.search(commit_pattern, rustup_rustc, re.M)
    cargo_commit = re.search(commit_pattern, cargo_rustc, re.M)
    if (not active_commit or not rustup_commit or not cargo_commit
            or active_commit[1] != expected_commit or rustup_commit[1] != expected_commit
            or cargo_commit[1] != expected_commit
            or f"release: {pin}\n" not in active_rustc
            or f"release: {pin}\n" not in rustup_rustc
            or f"release: {pin}\n" not in cargo_rustc
            or active_cargo != rustup_cargo or not active_cargo.startswith(f"cargo {pin} ")):
        raise RuntimeError("Spot test runner compiler or Cargo differs from rustup's pinned identity")
    return active_rustc, active_cargo


def oracle_cache_root(lock_path: Path, cargo_home: Path) -> Path:
    return cargo_home / "oxidex-oracle" / file_sha(lock_path)


def configure_oracle_environment(cache: Path, pin: str) -> None:
    """Keep bootstrap and every oracle consumer inside the same durable root."""
    os.environ["OXIDEX_OPS_DIR"] = str(cache)
    os.environ["EXIFTOOL_CACHE_DIR"] = str(cache / "cache/exiftool" / pin)
    os.environ["EXIFTOOL_PERL"] = str(cache / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2")


def load_oracle_bootstrap(cache: Path):
    if os.environ.get("OXIDEX_OPS_DIR") != str(cache):
        raise RuntimeError("oracle environment and bootstrap cache root differ")
    spec = importlib.util.spec_from_file_location(
        "oxidex_spot_oracle_bootstrap", ROOT / "tools/release/bootstrap_oracle.py")
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load locked oracle bootstrap")
    bootstrap = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bootstrap)
    return bootstrap


def provision_cached_oracle(bootstrap, lock_path: Path, cargo_home: Path) -> tuple[Path, Path]:
    """Reuse the locked oracle while serializing downloads and verification."""
    cache = oracle_cache_root(lock_path, cargo_home)
    cache.mkdir(parents=True, exist_ok=True)
    with (cache / ".provision.lock").open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            manifest = bootstrap.provision(cache)
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    return cache, manifest


def prepare_generic_recipe_oracle() -> Path:
    """Verify the locked oracle before an original Just test recipe can run."""
    lock_path = ROOT / "tools/release/oracle-lock.json"
    cargo_home = Path(os.environ.get("CARGO_HOME", "/cargo"))
    pin = (ROOT / ".exiftool-version").read_text().strip()
    cache = oracle_cache_root(lock_path, cargo_home)
    # The Rust oracle gives EXIFTOOL priority over its pinned cache and permits
    # degraded probes when the skew override is set. Neither is valid here.
    os.environ.pop("EXIFTOOL", None)
    os.environ.pop("OXIDEX_ALLOW_EXIFTOOL_SKEW", None)
    os.environ["OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"] = "1"
    configure_oracle_environment(cache, pin)
    bootstrap = load_oracle_bootstrap(cache)
    _, manifest = provision_cached_oracle(bootstrap, lock_path, cargo_home)
    # provision() validates the lock, authenticates the materialized artifacts,
    # runs the Perl/ExifTool/DOCX/corpus probes, and writes this manifest.
    report = json.loads(manifest.read_text())
    print("=== pinned Just test oracle ===", json.dumps({
        "oracle_pin": pin,
        "lock_sha256": report["lock_sha256"],
        "bootstrap_manifest_sha256": file_sha(manifest),
        "perl_sha256": report["artifacts"]["perl_executable"]["sha256"],
        "exiftool_tree_sha256": report["artifacts"]["exiftool_tree"]["sha256"],
        "docx_probe": report["probes"]["docx"],
    }, sort_keys=True), flush=True)
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--rustc-commit", required=True)
    args = parser.parse_args()
    if len(args.source_sha) != 40 or any(c not in "0123456789abcdef" for c in args.source_sha):
        parser.error("--source-sha must be a full Git SHA")
    if not re.fullmatch(r"[0-9a-f]{40}", args.rustc_commit):
        parser.error("--rustc-commit must be a full Rust commit SHA")
    os.environ["OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"] = "1"
    lock_path = ROOT / "tools/release/oracle-lock.json"
    cargo_home = Path(os.environ.get("CARGO_HOME", "/cargo"))
    cache = oracle_cache_root(lock_path, cargo_home)
    configure_oracle_environment(cache, (ROOT / ".exiftool-version").read_text().strip())
    bootstrap = load_oracle_bootstrap(cache)
    oracle_ops, manifest = provision_cached_oracle(bootstrap, lock_path, cargo_home)
    report = json.loads(manifest.read_text())
    pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    rustc, cargo = pinned_rust_identity(pin, args.rustc_commit)
    proof = {"schema": 1, "kind": "oxidex_spot_workspace_test", "source_commit": args.source_sha,
             "rust_pin": pin, "rustc_commit": args.rustc_commit,
             "rustc_version": rustc, "cargo_version": cargo,
             "oracle_pin": (ROOT / ".exiftool-version").read_text().strip(),
             "bootstrap_manifest_sha256": file_sha(manifest),
             "perl_sha256": report["artifacts"]["perl_executable"]["sha256"],
             "exiftool_tree_sha256": report["artifacts"]["exiftool_tree"]["sha256"],
             "corpus_tree_sha256": report["artifacts"]["corpus_tree"]["sha256"],
             "corpus_files": report["probes"]["corpus_files"], "status": "provisioned"}
    print("=== pinned remote oracle ===", json.dumps(proof, sort_keys=True), flush=True)
    python_command = [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py"]
    python_outcome = subprocess.run(python_command, cwd=ROOT / "tools/remote-build", env=os.environ.copy())
    proof["python_command"] = python_command
    proof["python_exit_code"] = python_outcome.returncode
    proof["qualification_unit_exit_code"] = None
    if python_outcome.returncode:
        proof["test_exit_code"] = python_outcome.returncode
    else:
        qualification_command = [sys.executable, "-m", "unittest", "test_version_transition_qualification.py"]
        qualification_outcome = subprocess.run(qualification_command,
                                              cwd=ROOT / "tools/exiftool-tables", env=os.environ.copy())
        proof["qualification_unit_command"] = qualification_command
        proof["qualification_unit_exit_code"] = qualification_outcome.returncode
        if qualification_outcome.returncode:
            proof["test_exit_code"] = qualification_outcome.returncode
        else:
            # Bind the proof to the actual pre-Cargo identity, not a probe
            # made before the Python suites ran.
            rustc, cargo = pinned_rust_identity(pin, args.rustc_commit)
            proof["rustc_version"] = rustc
            proof["cargo_version"] = cargo
            command = ["cargo", "test", "--workspace", "--all-features", "--locked", "--no-fail-fast"]
            outcome = subprocess.run(command, cwd=ROOT, env=os.environ.copy())
            proof["test_command"] = command
            proof["test_exit_code"] = outcome.returncode
    proof["status"] = "PASS" if proof["test_exit_code"] == 0 else "FAILED"
    TARGET.mkdir(parents=True, exist_ok=True)
    temporary = TARGET / ".remote-test.json.tmp"
    temporary.write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")
    temporary.replace(TARGET / "remote-test.json")
    return proof["test_exit_code"]


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"remote workspace test refused: {error}", file=sys.stderr)
        raise SystemExit(2)
