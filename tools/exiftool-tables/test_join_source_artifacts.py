import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import join_source_artifacts as join


def artifact(kind, *, module="Any", table="Main", gate='&[]', rows="", registry=True):
    table_type, registry_name, _sidecar = join.ARTIFACT_TYPES[kind]
    ident = f"{kind.upper()}_ANY_MAIN"
    source = f'''pub static {ident}: {table_type} = {table_type} {{
    module: "{module}",
    table: "{table}",
    gate_a: GateA {{ blocked_by: {gate} }},
    {rows}
}};
'''
    if registry:
        source += f"pub static {registry_name}: &[&{table_type}] = &[&{ident}];\n"
    return source


class ArtifactParserTests(unittest.TestCase):
    def test_records_gate_a_and_zero_emitted_rows_without_hiding_the_table(self):
        tables, accounting = join.parse_tables(
            artifact("binary", gate='&[("format", 2)]'), "binary"
        )
        row = tables[("Any", "Main")]
        self.assertEqual(accounting, {"table_definitions": 1, "registry_entries": 1})
        self.assertTrue(row["registry_listed"])
        self.assertEqual(row["gate_a_blocked_by"], [["format", 2]])
        self.assertEqual(row["emitted_row_literals"], 0)

    def test_rejects_a_static_missing_from_its_closed_registry(self):
        source = artifact("ifd", registry=False)
        with self.assertRaisesRegex(ValueError, "lacks ALL_IFD_TABLES"):
            join.parse_tables(source, "ifd")

    def test_rejects_unrecognised_gate_a_shape(self):
        source = artifact("binary", gate='&[UNKNOWN]')
        with self.assertRaisesRegex(ValueError, "unrecognised GateA blocker"):
            join.parse_tables(source, "binary")

    def test_binary_sidecar_requires_every_omission_to_parse(self):
        source = '''pub static OMITTED_NATIVE_FIELDS: &[OmittedNativeField] = &[
            OmittedNativeField { module: "Any", table: "Main", raw_id: "1" },
        ];
'''
        self.assertEqual(join.parse_sidecar_tables(source, "binary"), Counter({("Any", "Main"): 1}))
        bad = source.replace('table: "Main"', 'no_table: "Main"')
        with self.assertRaisesRegex(ValueError, "parsed 0"):
            join.parse_sidecar_tables(bad, "binary")


class JoinConservationTests(unittest.TestCase):
    def test_preserves_every_source_identity_including_unclassified_and_absent_artifacts(self):
        rows = [
            {"module": "Any", "table": "Binary", "selection": "binary"},
            {"module": "Any", "table": "Ifd", "selection": "ifd"},
            {"module": "Any", "table": "Other", "selection": "other_unclassified"},
        ]
        parsed = {
            "binary": {("Any", "Binary"): {"registry_listed": True, "emitted_row_literals": 0, "gate_a_blocked_by": [["format", 1]]}},
            "ifd": {("Any", "Ifd"): {"registry_listed": True, "emitted_row_literals": 2, "gate_a_blocked_by": []}},
            "keyed": {},
        }
        joined, totals = join.join_rows(rows, parsed, {"binary": Counter({("Any", "Binary"): 3}), "ifd": Counter(), "keyed": Counter()})
        self.assertEqual(len(joined), 3)
        self.assertEqual(joined[0]["artifacts"]["binary"]["sidecar_omitted_rows"], 3)
        self.assertEqual(joined[2]["artifacts"]["binary"]["definition"], "absent")
        self.assertEqual(joined[2]["artifacts"]["keyed"]["gate_a"], "unavailable")
        self.assertEqual(joined[2]["artifacts"]["keyed"]["artifact_state"], "present")
        self.assertEqual(totals["binary"]["definitions_present"], 1)
        self.assertEqual(totals["binary"]["source_selected_tables"], 1)
        self.assertEqual(totals["binary"]["zero_emitted_definitions"], 1)
        self.assertNotIn("definitions_absent", totals["binary"])

    def test_gate_a_summary_skips_unclassified_rows_without_dropping_them_from_join(self):
        joined = [
            {"selection": "binary", "artifacts": {"binary": {"gate_a_blocked_by": [["format", 2]]}}},
            {"selection": "other_unclassified", "artifacts": {}},
        ]
        self.assertEqual(join.gate_a_blocker_counts(joined), {"binary": Counter({"format": 2})})

    def test_rejects_generated_orphan_instead_of_selecting_artifacts_as_the_denominator(self):
        rows = [{"module": "Any", "table": "Main", "selection": "binary"}]
        parsed = {"binary": {("Ghost", "Main"): {}}, "ifd": {}, "keyed": {}}
        with self.assertRaisesRegex(ValueError, "absent from source"):
            join.join_rows(rows, parsed, {"binary": Counter(), "ifd": Counter(), "keyed": Counter()})

    def test_rejects_cross_family_definition(self):
        rows = [{"module": "Any", "table": "Main", "selection": "ifd"}]
        parsed = {"binary": {("Any", "Main"): {}}, "ifd": {}, "keyed": {}}
        with self.assertRaisesRegex(ValueError, "disagree with source selection"):
            join.join_rows(rows, parsed, {"binary": Counter(), "ifd": Counter(), "keyed": Counter()})

    def test_cli_requires_both_immutable_commit_expectations(self):
        with tempfile.TemporaryDirectory() as directory:
            dump = Path(directory) / "dump.json"
            dump.write_text("{}")
            proc = subprocess.run(
                [sys.executable, str(HERE / "join_source_artifacts.py"), "--dump", str(dump)],
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertEqual(proc.returncode, 2)
        self.assertIn("--expected-selector-commit", proc.stderr)
        self.assertIn("--expected-artifact-commit", proc.stderr)


if __name__ == "__main__":
    unittest.main()
