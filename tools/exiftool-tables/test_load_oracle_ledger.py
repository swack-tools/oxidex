#!/usr/bin/env python3
"""Focused regression tests for codegen.py's `load_oracle_ledger`.

Defect (docs/FLEET.md's 2026-08-14 addenda, "regen.sh has a hidden host
dependency"): a `tables.json` digest mismatch -- e.g. the committed
`expr_oracle_ledger.json` was written from the i7's Perl 5.38.2, but a
different host's Perl serializes ExifTool's tables to different bytes --
used to make `load_oracle_ledger` return `None`, indistinguishable from "no
ledger requested at all". Every caller (`conv_for`, `value_conv_for`) treats
`None` as "refuse every oracle-gated expression", so a stale/foreign ledger
silently zeroed out `binary_tables.rs`'s ExprId coverage with no diagnostic
(observed: 248 -> 7 variants, docs/FLEET.md).

These tests pin the fix: `path is None` (no `--expr-ledger` given) is the
only case that still returns `None` -- an explicit caller opt-out. A `path`
that IS given but fails to validate now raises `SystemExit` naming what
failed, instead of returning `None` and letting the failure hide.
"""

import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("codegen.py")
SPEC = importlib.util.spec_from_file_location("codegen", MODULE_PATH)
codegen = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(codegen)


class LoadOracleLedgerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tables_path = Path(self.tmp.name) / "tables.json"
        self.tables_path.write_text(
            json.dumps({"exiftool_version": "13.59", "modules": {}}), encoding="utf-8"
        )
        self.digest = hashlib.sha256(self.tables_path.read_bytes()).hexdigest()
        self.ledger_path = Path(self.tmp.name) / "ledger.json"

    def _write_ledger(self, **overrides):
        base = {
            "schema": codegen.LEDGER_SCHEMA,
            "exiftool_version": "13.59",
            "perl_version": "v5.38.2",
            "tables_sha256": self.digest,
            "probe_counts": {"pass": 10, "fail": 0, "skip": 0},
            "verified_expressions": ["$val / 10"],
        }
        base.update(overrides)
        self.ledger_path.write_text(json.dumps(base), encoding="utf-8")

    def test_no_ledger_requested_returns_none_silently(self):
        self.assertIsNone(
            codegen.load_oracle_ledger(None, str(self.tables_path), "13.59")
        )

    def test_valid_ledger_returns_verified_set(self):
        self._write_ledger()
        result = codegen.load_oracle_ledger(
            str(self.ledger_path), str(self.tables_path), "13.59"
        )
        self.assertEqual(result, {"$val / 10"})

    def test_digest_mismatch_aborts_loudly_naming_both_digests(self):
        self._write_ledger(tables_sha256="0" * 64)
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(self.ledger_path), str(self.tables_path), "13.59"
            )
        msg = str(ctx.exception)
        self.assertIn("tables_sha256 mismatch", msg)
        self.assertIn("0" * 64, msg)
        self.assertIn(self.digest, msg)
        self.assertIn("REFUSED", msg)

    def test_schema_1_aborts_loudly_naming_missing_provenance(self):
        self._write_ledger(schema=1)
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(self.ledger_path), str(self.tables_path), "13.59"
            )
        self.assertIn("schema 1", str(ctx.exception))

    def test_nonzero_probe_fail_count_aborts_loudly(self):
        self._write_ledger(probe_counts={"pass": 5, "fail": 2, "skip": 0})
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(self.ledger_path), str(self.tables_path), "13.59"
            )
        self.assertIn("probe_counts.fail=2", str(ctx.exception))

    def test_exiftool_version_mismatch_aborts_loudly(self):
        self._write_ledger(exiftool_version="13.55")
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(self.ledger_path), str(self.tables_path), "13.59"
            )
        self.assertIn("exiftool_version mismatch", str(ctx.exception))

    def test_missing_ledger_file_aborts_loudly_rather_than_returning_none(self):
        missing = Path(self.tmp.name) / "does-not-exist.json"
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(missing), str(self.tables_path), "13.59"
            )
        self.assertIn("unreadable", str(ctx.exception))

    def test_malformed_json_aborts_loudly(self):
        self.ledger_path.write_text("{not json", encoding="utf-8")
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(self.ledger_path), str(self.tables_path), "13.59"
            )
        self.assertIn("not valid JSON", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
