#!/usr/bin/env python3
"""`expr_coverage.py` must name its instrument before its first number.

AGENTS.md: "Every measurement script under `tools/exiftool-tables/` ... prints
an `=== instrument: <tool> ===` header before its first number", and "a dirty
tree refuses to measure at all unless `OXIDEX_ALLOW_DIRTY_TREE=1` is set, in
which case the header says so."

`expr_coverage.py` was exempt from both while being the tool that publishes
the headline coverage percentage (66.6% of uses translated, quoted in
docs/TAG_MACHINERY_RECONCILIATION.md's R2 row) -- a number attributable to no
commit and to no tree state. These tests pin the fix.

Deliberately NOT a `grep instrument` over the directory: `rg -l instrument`
returns 8 files here, two of them false positives (a string literal at
codegen.py:2334 and prose at triage_bump.py:238,240), which is the same
count-the-sentence error `reachability.py` records for `ricoh.rs:215`. These
tests run the tool instead.
"""

import contextlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("expr_coverage.py")
REPO_ROOT = MODULE_PATH.resolve().parents[2]


def _load():
    """Fresh module instance per test -- it mutates sys.path on import."""
    spec = importlib.util.spec_from_file_location("expr_coverage", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class _FakeGit:
    """Minimal stand-in for instrument.GitState -- only what the header and
    the refusal touch."""

    def __init__(self, dirty_files):
        self.repo_root = REPO_ROOT
        self.commit = "0" * 40
        self.describe = "fake"
        self.dirty = bool(dirty_files)
        self.dirty_files = list(dirty_files)
        self.head_time = None

    def short(self):
        return "fake (000000000000, %s)" % ("DIRTY" if self.dirty else "clean")


class InstrumentHeaderTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dump = Path(self.tmp.name) / "tables.json"
        # One tag, one expression exprs.py certainly translates, so the run
        # produces real numbers rather than dividing by zero.
        self.dump.write_text(json.dumps({
            "exiftool_version": "13.59",
            "modules": {"Test": {"tables": {"Main": {"tags": {
                "0": {"Name": "T", "PrintConv": {"kind": "expr", "expr": "$val / 10"}},
            }}}}},
        }), encoding="utf-8")

    def test_header_is_printed_before_the_first_number(self):
        env = dict(os.environ, OXIDEX_ALLOW_DIRTY_TREE="1")
        out = subprocess.run(
            [sys.executable, str(MODULE_PATH), str(self.dump)],
            capture_output=True, text=True, cwd=str(MODULE_PATH.parent), env=env,
            check=True,
        ).stdout
        lines = out.splitlines()
        self.assertEqual(lines[0], "=== instrument: expr_coverage.py ===")
        self.assertTrue(any(l.startswith("repo:") for l in lines[:6]), out[:400])
        self.assertTrue(any(l.startswith("tables:") for l in lines[:8]), out[:400])
        # ...and the header really does come first: no digit-bearing report
        # line before it.
        self.assertLess(out.index("=== instrument"), out.index("pinned release"))

    def test_dirty_tree_is_refused_without_the_override(self):
        module = _load()
        module.instrument.git_state = lambda *a, **k: _FakeGit(["src/lib.rs"])
        argv, environ = sys.argv, os.environ.get("OXIDEX_ALLOW_DIRTY_TREE")
        os.environ.pop("OXIDEX_ALLOW_DIRTY_TREE", None)
        sys.argv = ["expr_coverage.py", str(self.dump)]
        try:
            with self.assertRaises(SystemExit) as ctx:
                module.main()
            msg = str(ctx.exception)
            self.assertIn("refusing to measure against a dirty working tree", msg)
            self.assertIn("OXIDEX_ALLOW_DIRTY_TREE", msg)
        finally:
            sys.argv = argv
            if environ is not None:
                os.environ["OXIDEX_ALLOW_DIRTY_TREE"] = environ

    def test_override_is_recorded_in_the_header(self):
        module = _load()
        module.instrument.git_state = lambda *a, **k: _FakeGit(["src/lib.rs"])
        argv, environ = sys.argv, os.environ.get("OXIDEX_ALLOW_DIRTY_TREE")
        os.environ["OXIDEX_ALLOW_DIRTY_TREE"] = "1"
        sys.argv = ["expr_coverage.py", str(self.dump)]
        buf = io.StringIO()
        try:
            with contextlib.redirect_stdout(buf):
                module.main()
        finally:
            sys.argv = argv
            if environ is None:
                os.environ.pop("OXIDEX_ALLOW_DIRTY_TREE", None)
            else:
                os.environ["OXIDEX_ALLOW_DIRTY_TREE"] = environ
        out = buf.getvalue()
        # A measured-anyway run must say so in the header, or the number
        # reads exactly like one taken against a clean tree.
        self.assertIn("[OXIDEX_ALLOW_DIRTY_TREE=1: measuring anyway]", out)
        self.assertIn("dirty: src/lib.rs", out)
        self.assertIn("translated   uses", out)


if __name__ == "__main__":
    unittest.main()
