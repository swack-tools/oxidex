#!/usr/bin/env python3
"""Provision the locked Linux oracle, then run every workspace test on Spot."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    if len(args.source_sha) != 40 or any(c not in "0123456789abcdef" for c in args.source_sha):
        parser.error("--source-sha must be a full Git SHA")
    ops = TARGET / "ops"
    os.environ["OXIDEX_OPS_DIR"] = str(ops)
    os.environ["EXIFTOOL_CACHE_DIR"] = str(ops / "cache/exiftool" / (ROOT / ".exiftool-version").read_text().strip())
    os.environ["EXIFTOOL_PERL"] = str(ops / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2")
    os.environ["OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"] = "1"
    spec = importlib.util.spec_from_file_location("oxidex_spot_oracle_bootstrap",
                                                  ROOT / "tools/release/bootstrap_oracle.py")
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load locked oracle bootstrap")
    bootstrap = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bootstrap)
    manifest = bootstrap.provision(ops)
    report = json.loads(manifest.read_text())
    pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    rustc = subprocess.check_output(["rustc", "-vV"], text=True)
    cargo = subprocess.check_output(["cargo", "-V"], text=True).strip()
    if f"release: {pin}\n" not in rustc or not cargo.startswith(f"cargo {pin} "):
        raise RuntimeError("Spot test runner is not using the repository Rust pin")
    proof = {"schema": 1, "kind": "oxidex_spot_workspace_test", "source_commit": args.source_sha,
             "rust_pin": pin, "rustc_version": rustc, "cargo_version": cargo,
             "oracle_pin": (ROOT / ".exiftool-version").read_text().strip(),
             "bootstrap_manifest_sha256": file_sha(manifest),
             "perl_sha256": report["artifacts"]["perl_executable"]["sha256"],
             "exiftool_tree_sha256": report["artifacts"]["exiftool_tree"]["sha256"],
             "corpus_tree_sha256": report["artifacts"]["corpus_tree"]["sha256"],
             "corpus_files": report["probes"]["corpus_files"], "status": "provisioned"}
    print("=== pinned remote oracle ===", json.dumps(proof, sort_keys=True), flush=True)
    command = ["cargo", "test", "--workspace", "--all-features", "--locked", "--no-fail-fast"]
    outcome = subprocess.run(command, cwd=ROOT, env=os.environ.copy())
    proof["test_command"] = command
    proof["test_exit_code"] = outcome.returncode
    proof["status"] = "PASS" if outcome.returncode == 0 else "FAILED"
    TARGET.mkdir(parents=True, exist_ok=True)
    temporary = TARGET / ".remote-test.json.tmp"
    temporary.write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")
    temporary.replace(TARGET / "remote-test.json")
    return outcome.returncode


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"remote workspace test refused: {error}", file=sys.stderr)
        raise SystemExit(2)
