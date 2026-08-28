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


class CommittedLedgerTests(unittest.TestCase):
    """The committed `expr_oracle_ledger.json` must actually load.

    The tests above all validate synthetic ledgers, which is why the real
    committed artifact could sit at `schema: 1` -- refused by
    `load_oracle_ledger` on every host, digest match or not -- with nothing
    saying so: `rg -n 'expr_oracle_ledger' justfile .github/workflows/*.yml`
    returned zero hits, and `regen.sh:63-67` rewrites the ledger before
    `codegen.py` ever reads it, so the normal pipeline self-heals over a
    dead artifact. These tests grade the committed bytes instead.

    `tables_sha256` is deliberately the one field substituted below. It is
    host-dependent BY DESIGN -- Perl 5.34.1 (this Mac) and Perl 5.38.2 (the
    i7, and the ledger's own recorded provenance) serialize ExifTool's
    tables to genuinely different bytes, measured: `perl
    tools/exiftool-tables/dump_tables.pl <13.59 lib>` gives
    `79412ee8...` under 5.34.1 and `6b6bd4f8...` under 5.38.2, the latter
    being exactly the digest the committed ledger records. A test that
    demanded a digest match would therefore fail on every host but one,
    which is not the property worth pinning. Every OTHER field is the
    committed one, checked by the real loader.
    """

    LEDGER_PATH = Path(__file__).with_name("expr_oracle_ledger.json")
    PIN_PATH = Path(__file__).resolve().parents[2] / ".exiftool-version"

    def setUp(self):
        self.ledger = json.loads(self.LEDGER_PATH.read_text(encoding="utf-8"))
        self.pin = self.PIN_PATH.read_text(encoding="utf-8").strip()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tables_path = Path(self.tmp.name) / "tables.json"
        self.tables_path.write_text(
            json.dumps({"exiftool_version": self.pin, "modules": {}}), encoding="utf-8"
        )
        self.local_digest = hashlib.sha256(self.tables_path.read_bytes()).hexdigest()

    def _rehosted_copy(self, **overrides):
        """The committed ledger with only `tables_sha256` re-pointed at a
        local file, so the real loader can be run on a host whose Perl is not
        the ledger's."""
        doc = dict(self.ledger)
        doc["tables_sha256"] = self.local_digest
        doc.update(overrides)
        path = Path(self.tmp.name) / "rehosted.json"
        path.write_text(json.dumps(doc), encoding="utf-8")
        return path

    def test_committed_ledger_loads_under_the_committed_codegen(self):
        result = codegen.load_oracle_ledger(
            str(self._rehosted_copy()), str(self.tables_path), self.pin
        )
        self.assertEqual(result, set(self.ledger["verified_expressions"]))
        self.assertGreater(len(result), 0, "a ledger approving nothing gates everything")

    def test_this_check_would_have_caught_the_schema_1_defect(self):
        # Negative control. Before this fix the committed file WAS schema 1.
        with self.assertRaises(SystemExit) as ctx:
            codegen.load_oracle_ledger(
                str(self._rehosted_copy(schema=1)), str(self.tables_path), self.pin
            )
        self.assertIn("schema 1", str(ctx.exception))

    def test_committed_ledger_matches_the_repo_pin(self):
        self.assertEqual(self.ledger["exiftool_version"], self.pin)

    def test_committed_ledger_records_its_perl_provenance(self):
        # The entire point of schema 2: a digest with no interpreter behind
        # it cannot be re-derived by anyone.
        self.assertEqual(self.ledger["schema"], codegen.LEDGER_SCHEMA)
        self.assertTrue(self.ledger.get("perl_version"))
        self.assertEqual(self.ledger["instrument"]["perl_version"],
                         self.ledger["perl_version"])
        self.assertRegex(self.ledger["tables_sha256"], r"^[0-9a-f]{64}$")

    def test_committed_ledger_records_a_passing_oracle_run(self):
        self.assertEqual(self.ledger["probe_counts"]["fail"], 0)
        self.assertGreater(self.ledger["probe_counts"]["pass"], 0)

    def test_verified_expressions_are_normalized_sorted_and_unique(self):
        # `conv_for`/`value_conv_for` look each expression up by
        # `exprs.normalize(...)`, so an unnormalized entry approves nothing
        # while still inflating the ledger's own counts.
        verified = self.ledger["verified_expressions"]
        self.assertEqual(verified, sorted(verified))
        self.assertEqual(len(verified), len(set(verified)))
        unnormalized = [e for e in verified if codegen.exprs.normalize(e) != e]
        self.assertEqual(unnormalized, [], "entries that no lookup can ever match")

    def test_counts_agree_with_the_expression_list(self):
        self.assertEqual(self.ledger["expression_counts"]["verified"],
                         len(self.ledger["verified_expressions"]))


if __name__ == "__main__":
    unittest.main()
