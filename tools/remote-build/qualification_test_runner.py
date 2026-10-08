#!/usr/bin/env python3
"""Fixed remote-only Python controls for beta.1 qualification.

These are unit/control suites. Passing them does not run Task19 rows or a
corpus read gate, and cannot stand in for qualification receipts.
"""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
REMOTE = ROOT / "tools/remote-build"
TABLES = ROOT / "tools/exiftool-tables"
TABLE_MODULES = (
    "test_version_transition_qualification",
    "test_verify_task19_results",
    "test_qualification_bootstrap_boundary",
    "test_version_transition_read_policy",
    "test_version_rehearsal_executor",
    "test_version_rehearsal_clean_snapshot",
    "test_conformance",
)
SUITES = (
    (REMOTE, (sys.executable, "-m", "unittest", "discover", "-s", "tests",
              "-p", "test_qualification*.py")),
    (TABLES, (sys.executable, "-m", "unittest", *TABLE_MODULES)),
)


def main() -> int:
    # The private Just entry has the same guard; retain it if this file is
    # invoked directly by a caller in the signed worker checkout.
    import route
    route.main(["--require-local-context", "test-qualification"])
    head = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
    manifest = {"schema": 1, "kind": "qualification-python-controls", "source_head": head,
                "suites": ["tools/remote-build/tests/test_qualification*.py",
                           *(f"tools/exiftool-tables/{name}.py" for name in TABLE_MODULES)],
                "claim": "unit-controls-only; no Task19 rows or corpus gate"}
    print("QUALIFICATION_PYTHON_MANIFEST " + json.dumps(manifest, sort_keys=True), flush=True)
    failures = 0
    for cwd, command in SUITES:
        print("QUALIFICATION_PYTHON_COMMAND " + json.dumps({"cwd": str(cwd), "argv": command}),
              flush=True)
        result = subprocess.run(command, cwd=cwd, check=False)
        print("QUALIFICATION_PYTHON_EXIT " + json.dumps({"cwd": str(cwd),
                                                     "exit_code": result.returncode}), flush=True)
        failures += result.returncode != 0
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
