import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import join_source_artifacts as join
import keyed_directory


PROC = {"__perl": "CODE", "__name": "Image::ExifTool::CanonRaw::ProcessCanonRaw"}


def keyed_doc(tags):
    return {"modules": {"Any": {"tables": {
        "Main": {"meta": {"PROCESS_PROC": PROC, "GROUPS": {"0": "MakerNotes"}}, "tags": tags},
    }}}}


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
    def test_policy_comments_cannot_supply_declarations_or_terminators(self):
        text = '''// pub static ENABLED: &[(&str, &str)] = &[("Ghost", "Table")];
pub static ENABLED: &[(&str, &str)] = &[
    /* ] ; /* nested comment */ ("Ghost", "Table"), */
    ("Good//Module", "Table/*literal*/"), // ];
];'''
        self.assertEqual(join.parse_enabled(text, "ENABLED"),
                         {("Good//Module", "Table/*literal*/")})

    def test_policy_refuses_missing_separators_and_ambiguous_source(self):
        prefix = "pub static ENABLED: &[(&str, &str)] = &["
        for bad in ('("A", "B") ("C", "D")', '("A", "B"), ("A", "B")',
                    'include!("policy")', '/* unterminated', '("A", "unterminated'):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                join.parse_enabled(prefix + bad + "];", "ENABLED")
        with self.assertRaises(ValueError):
            join.parse_enabled(prefix + "];" + prefix + "];", "ENABLED")

    def test_enabled_parser_ignores_comments_and_refuses_unknown_syntax(self):
        text = 'pub static ENABLED: &[(&str, &str)] = &[\n// ("Bad", "Row"),\n("Good", "Row"),\n];'
        self.assertEqual(join.parse_enabled(text, "ENABLED"), {("Good", "Row")})
        with self.assertRaisesRegex(ValueError, "unrecognised"):
            join.parse_enabled('pub static ENABLED: &[(&str, &str)] = &[ UNKNOWN ];', "ENABLED")

    def test_records_gate_a_and_zero_emitted_rows_without_hiding_the_table(self):
        tables, accounting = join.parse_tables(
            artifact("binary", gate='&[("format", 2)]'), "binary"
        )
        row = tables[("Any", "Main")]
        self.assertEqual(accounting, {"table_definitions": 1, "registry_entries": 1})
        self.assertTrue(row["registry_listed"])
        self.assertEqual(row["gate_a_blocked_by"], [["format", 2]])
        self.assertEqual(row["emitted_row_literals"], 0)

    def test_parses_the_real_keyed_generator_registry_name_and_entry_shape(self):
        generated, _stats = keyed_directory.generate(keyed_doc({
            "0x1001": {"Name": "Value", "Format": "int16u"},
        }))
        tables, accounting = join.parse_tables(generated, "keyed")
        self.assertEqual(set(tables), {("Any", "Main")})
        self.assertEqual(accounting, {"table_definitions": 1, "registry_entries": 1})
        self.assertTrue(tables[("Any", "Main")]["registry_listed"])

    def test_rejects_leftover_registry_syntax_instead_of_partial_accounting(self):
        source = artifact("binary").replace("&BINARY_ANY_MAIN];", "&BINARY_ANY_MAIN, UNKNOWN];")
        with self.assertRaisesRegex(ValueError, "unrecognised syntax"):
            join.parse_tables(source, "binary")

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


class GitBlobTests(unittest.TestCase):
    def test_only_a_missing_tree_path_is_optional(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Join Test"], check=True)
            (repo / "present.rs").write_bytes(b"artifact")
            subprocess.run(["git", "-C", str(repo), "add", "present.rs"], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            commit = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            self.assertEqual(join.git_blob_or_none(repo, commit, "present.rs"), b"artifact")
            self.assertIsNone(join.git_blob_or_none(repo, commit, "missing.rs"))
            with self.assertRaisesRegex(RuntimeError, "cannot inspect artifact path"):
                join.git_blob_or_none(repo, "not-a-commit", "missing.rs")


class JoinConservationTests(unittest.TestCase):
    def test_missing_policy_is_unknown_and_gate_a_is_explicit(self):
        artifact = {"definition": "present", "registry_listed": True}
        self.assertEqual(join.runtime_evidence("binary", artifact)["reason"], "enablement_policy_missing")
        blocked = {**artifact, "gate_a_blocked_by": [["unknown_conversion", 1]]}
        result = join.runtime_evidence("binary", blocked, {("A", "B")}, ("A", "B"))
        self.assertEqual((result["state"], result["reason"]), ("not_enabled", "gate_a_blocked"))

    def test_runtime_evidence_distinguishes_absent_unenabled_and_unknown(self):
        self.assertEqual(join.runtime_evidence("binary", {"definition": "absent"})["state"], "unknown")
        self.assertEqual(join.runtime_evidence("binary", {"definition": "present", "registry_listed": False})["state"], "not_enabled")
        self.assertEqual(join.runtime_evidence("other_unclassified", {})["state"], "unknown")
        self.assertEqual(join.runtime_evidence("binary", {"definition": "present", "registry_listed": True}, set(), ("Any", "Main"))["state"], "not_enabled")
        self.assertEqual(join.runtime_evidence("binary", {"definition": "present", "registry_listed": True}, {("Any", "Main")}, ("Any", "Main"), False)["state"], "unverified")

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

    def test_keyed_artifact_does_not_reclassify_an_unknown_source_family(self):
        rows = [{"module": "Any", "table": "Main", "selection": "other_unclassified"}]
        parsed = {"binary": {}, "ifd": {}, "keyed": {("Any", "Main"): {}}}
        with self.assertRaisesRegex(ValueError, "disagree with source selection"):
            join.join_rows(rows, parsed, {kind: Counter() for kind in parsed})

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
