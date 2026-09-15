"""Garmin FIT spec generation: acceptance, refusals and regeneration behavior.

The bounded source fixture is the Garmin module and ProcessFIT protocol fact
from the pinned full dump (`garmin_fit_specs.py --protocol-fact ... --write-bounded`). The
committed ledger and Rust must replay from it byte-for-byte (Rust up to
rustfmt), and an ordinary supported source row added to it must appear in
the generated specs with no hand edit to Rust.
"""
from __future__ import annotations

import copy
import json
import os
import subprocess
import unittest
from pathlib import Path

import garmin_fit_specs as specs
from join_catalog_hydrated import quicktime_rust_matches

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
BOUNDED = HERE / "fixtures" / "garmin_fit_source.json"
LEDGER = HERE / "garmin_fit_ledger.json"
RUST = ROOT / "src" / "exiftool_tables" / "fit_tables.rs"


def verified():
    return specs.unbound_verified_expressions(HERE / "expr_oracle_ledger.json")


class BoundedReplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = json.loads(BOUNDED.read_text())
        cls.rust, cls.result = specs.generate(cls.document, verified())

    def test_committed_ledger_replays(self):
        committed = json.loads(LEDGER.read_text())
        self.assertEqual(committed, json.loads(specs.serialized(self.result)))

    def test_committed_rust_replays(self):
        self.assertTrue(quicktime_rust_matches(self.rust, RUST.read_text()))

    def test_protocol_admitted_and_every_row_accounted(self):
        protocol = self.result["protocol"]
        self.assertTrue(protocol["admitted"], protocol["reasons"])
        counts = self.result["counts"]
        self.assertEqual(counts["source_rows"], counts["generated"] + counts["refused"])
        tables = self.document["modules"]["Garmin"]["tables"]
        self.assertEqual(counts["source_rows"], sum(len(table["tags"]) for table in tables.values()))

    def test_refusals_carry_exact_reasons(self):
        refused = {(row["identity"]["table"].rsplit("::", 1)[1], row["identity"]["raw_key"]): row["reasons"]
                   for row in self.result["rows"] if not row["generated"]}
        # ProcessFIT reads field numbers as one byte.
        self.assertEqual(refused[("TrainingSettings", "1001")], ["field_key_outside_u8_protocol"])
        # The one Garmin ValueConv outside the closed expression grammar.
        self.assertEqual(refused[("GPS", "7")], ["value_conv_uncompiled"])
        self.assertTrue(all(reasons for reasons in refused.values()))

    def test_default_mode_connection(self):
        by_table = {message["table"]: message for message in self.result["messages"] if message["table"]}
        self.assertFalse(by_table["Session"]["unknown"])
        self.assertTrue(by_table["FileID"]["unknown"])
        self.assertEqual(self.result["tables"]["Session"]["connection"], "default_mode_field_list")
        self.assertEqual(self.result["tables"]["FileID"]["connection"], "unknown_option_not_exposed")
        pad = [message for message in self.result["messages"] if message["name"] == "Pad"]
        self.assertEqual(pad, [{"num": 105, "name": "Pad", "table": None, "unknown": True, "withheld": []}])

    def test_conversions_carry_their_compiled_domain(self):
        session = {field["num"]: field["spec"] for field in self.result["tables"]["Session"]["fields"]}
        self.assertEqual(session[16]["print_conv"], {"kind": "typed", "expr": '"$val bpm"', "domain": "num"})
        common = {field["num"]: field["spec"] for field in self.result["tables"]["Common"]["fields"]}
        self.assertEqual(common[253]["value_conv"]["domain"], "num")
        self.assertEqual(common[253]["print_conv"]["domain"], "str")


class RegenerationTests(unittest.TestCase):
    def setUp(self):
        self.document = copy.deepcopy(json.loads(BOUNDED.read_text()))
        self.session = self.document["modules"]["Garmin"]["tables"]["Session"]["tags"]

    def test_ordinary_supported_row_appears_without_rust_edits(self):
        self.assertNotIn("240", self.session)
        self.session["240"] = {"Name": "SourceAddedHeartRate",
                               "PrintConv": {"kind": "expr", "expr": '"$val bpm"'}}
        rust, result = specs.generate(self.document, verified())
        row = [row for row in result["rows"]
               if row["identity"] == {"table": "Image::ExifTool::Garmin::Session", "raw_key": "240",
                                      "variant_index": 0}]
        self.assertEqual(len(row), 1)
        self.assertTrue(row[0]["generated"], row[0]["reasons"])
        self.assertIn('FitField { num: 240, name: "SourceAddedHeartRate", group2: None, raw_conv: None, '
                      'value_conv: None, print_conv: FitPrintConv::Typed(TypedConv { expr: ExprId::ValBpm49633A, '
                      'domain: ConvDomain::Num }), withheld: None }', rust)

    def test_refused_edge_stays_in_the_map_withheld(self):
        edges = self.document["modules"]["Garmin"]["tables"]["FIT"]["tags"]
        edges["18"]["Condition"] = {"kind": "expr", "expr": "1"}
        rust, result = specs.generate(self.document, verified())
        session = next(message for message in result["messages"] if message["num"] == 18)
        self.assertEqual(session["withheld"], ["unsupported_edge_property:Condition"])
        self.assertIsNone(session["table"])
        self.assertIn('FitMessage { num: 18, name: "Session", unknown: false, table: None, '
                      'withheld: Some("unsupported_edge_property:Condition") }', rust)

    def test_unverified_expression_is_refused(self):
        self.session["240"] = {"Name": "Novel", "PrintConv": {"kind": "expr", "expr": '"$val novel-unit"'}}
        _, result = specs.generate(self.document, verified())
        row = next(row for row in result["rows"] if row["identity"]["raw_key"] == "240"
                   and row["identity"]["table"].endswith("::Session"))
        self.assertEqual(row["reasons"], ["print_conv_not_oracle_verified"])

    def test_unmodeled_row_property_is_refused(self):
        self.session["240"] = {"Name": "Conditional", "Condition": {"kind": "expr", "expr": "1"}}
        _, result = specs.generate(self.document, verified())
        row = next(row for row in result["rows"] if row["identity"]["raw_key"] == "240"
                   and row["identity"]["table"].endswith("::Session"))
        self.assertEqual(row["reasons"], ["unsupported_source_property:Condition"])

    def test_changed_process_fit_refuses_the_protocol(self):
        fact = self.document["garmin_fit_reader_protocol"]["process_fit"]
        fact["__deparse"] += "\n;"
        rust, result = specs.generate(self.document, verified())
        self.assertFalse(result["protocol"]["admitted"])
        self.assertEqual(result["protocol"]["reasons"], ["missing_or_changed_reader_protocol:ProcessFIT"])
        self.assertEqual(result["counts"]["generated"], 0)
        self.assertIn('refusal: Some("missing_or_changed_reader_protocol:ProcessFIT")', rust)

    def test_unresolved_base_types_refuse_the_protocol(self):
        self.document["garmin_fit_reader_protocol"]["base_types"] = {"resolved": False, "reason": "x"}
        _, result = specs.generate(self.document, verified())
        self.assertIn("missing_or_changed_reader_protocol:base_types", result["protocol"]["reasons"])

    def test_undecodable_base_type_format_refuses_the_protocol(self):
        entries = self.document["garmin_fit_reader_protocol"]["base_types"]["entries"]
        entries["99"] = {"format": "rational64u", "fit_name": "novel", "invalid": "0"}
        _, result = specs.generate(self.document, verified())
        self.assertIn("missing_or_changed_reader_protocol:base_type_format", result["protocol"]["reasons"])

    def test_narrow_perl_integers_refuse_only_64_bit_types(self):
        self.document["garmin_fit_reader_protocol"]["perl_integer"]["ivsize"] = 4
        _, result = specs.generate(self.document, verified())
        refused = {row["fit_name"] for row in result["base_types"] if not row["admitted"]}
        self.assertEqual(refused, {"sint64", "uint64", "uint64z"})
        self.assertTrue(result["protocol"]["admitted"])


FRESH_INPUTS = ("OXIDEX_TABLES_JSON", "OXIDEX_PINNED_EXIFTOOL", "EXIFTOOL_PERL")


class FreshSourceTests(unittest.TestCase):
    """The bounded fixture is exactly the fresh pinned source, so the replay
    tests above prove the committed ledger and Rust are current. Skipped
    locally without the inputs; in CI a missing input is a failure, so a
    dropped export cannot silently turn this check off."""

    def test_bounded_fixture_is_the_fresh_projection(self):
        missing = [name for name in FRESH_INPUTS if not os.environ.get(name)]
        if missing:
            if os.environ.get("GITHUB_ACTIONS"):
                self.fail(f"fresh pinned-source inputs missing in CI: {missing}")
            self.skipTest(f"set {', '.join(FRESH_INPUTS)} for the pinned source")
        capture = subprocess.run(
            [os.environ["EXIFTOOL_PERL"], str(HERE / "capture_garmin_fit_fact.pl"),
             str(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]) / "lib")],
            capture_output=True, text=True)
        self.assertEqual(capture.returncode, 0, capture.stderr)
        fact = capture.stdout
        fresh = json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text())
        self.assertEqual(json.loads(BOUNDED.read_text()), specs.bounded_projection(fresh, json.loads(fact)))


if __name__ == "__main__":
    unittest.main()
