"""Independent mutations for emitted ProcessSerialData facts.

The compiler is used only to make a realistic Rust input.  The verifier under
test parses that text and re-reads the native document without importing the
emitter or its recognizer.
"""

import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import serial_directory
import verify_serial_directory as audit
from test_serial_directory import table


PINNED_DUMP = os.environ.get("OXIDEX_TABLES_JSON")


def fixture_document():
    return {"modules": {"Fixture": {"tables": {"Serial": table()}}}}


def emitted(document):
    return serial_directory.serial_rust_source(serial_directory.compile_serial_population(document))


class SerialArtifactVerifier(unittest.TestCase):
    def setUp(self):
        self.document = fixture_document()
        self.source = emitted(self.document)

    def audit_text(self, source):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(source, encoding="utf-8")
            return audit.audit(self.document, audit.parse_artifact(path))

    def assert_rejected(self, source, fragment):
        result = self.audit_text(source)
        self.assertFalse(result.ok)
        self.assertTrue(any(fragment in problem for problem in result.mismatches), result.mismatches)

    def test_baseline_accounts_for_all_native_alternatives(self):
        result = self.audit_text(self.source)
        self.assertTrue(result.ok, result.mismatches)
        self.assertEqual((result.expected_tables, result.expected_alternatives), (1, 6))
        self.assertEqual((result.emitted_tables, result.emitted_alternatives, result.omitted_alternatives), (1, 2, 4))

    def test_empty_artifact_cannot_choose_its_own_denominator(self):
        empty = """use super::*;
pub static ALL_SERIAL_TABLES: &[&SerialTable] = &[];
pub static OMITTED_SERIAL_NATIVE_ROWS: &[OmittedSerialNativeRow] = &[];
pub static OMITTED_SERIAL_NATIVE_TABLES: &[OmittedSerialNativeTable] = &[];
"""
        self.assert_rejected(empty, "selected native table absent")

    def test_cli_rejects_forged_descriptor_refusal_for_every_native_table(self):
        forged = """use super::*;
pub static ALL_SERIAL_TABLES: &[&SerialTable] = &[];
pub static OMITTED_SERIAL_NATIVE_ROWS: &[OmittedSerialNativeRow] = &[];
pub static OMITTED_SERIAL_NATIVE_TABLES: &[OmittedSerialNativeTable] = &[
    OmittedSerialNativeTable { module: "Fixture", table: "Serial", reasons: &["serial_descriptor_refused"] },
];
"""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source_dump = root / "native.json"
            artifact = root / "serial_tables.rs"
            source_dump.write_text(json.dumps(self.document), encoding="utf-8")
            artifact.write_text(forged, encoding="utf-8")
            run = subprocess.run(
                [sys.executable, str(Path(audit.__file__)), str(artifact), str(source_dump)],
                text=True, capture_output=True, timeout=15,
            )
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertIn("wholly omitted without independent native proof", run.stderr)

    def test_omission_for_nonexistent_native_row_fails(self):
        needle = "];\npub static OMITTED_SERIAL_NATIVE_TABLES"
        self.assertIn(needle, self.source)
        forged = self.source.replace(
            needle,
            "    OmittedSerialNativeRow { module: \"Bogus\", table: \"Nope\", serial_index: 0, variant: false, alternative: 0, name: Some(\"Bogus\"), reasons: &[\"serial_row_shape\"] },\n];\npub static OMITTED_SERIAL_NATIVE_TABLES",
            1,
        )
        self.assert_rejected(forged, "has no selected native source alternative")

    def test_removed_emitted_row_fails_against_native_population(self):
        start = self.source.index("SerialEntry { serial_index: 0")
        end = audit._span(self.source, start, {","})
        self.assert_rejected(self.source[:start] + self.source[end + 1:], "supported native alternative is absent")

    def test_count_groups_and_processor_identity_mutations_fail(self):
        changes = (
            ("count: SerialCount::Fixed { value: 1 }", "count: SerialCount::Fixed { value: 2 }", "name/format/count"),
            ('group1: "Fixture"', 'group1: "Wrong"', "effective groups"),
            ('source_sha256: "' + "a" * 64 + '"', 'source_sha256: "' + "b" * 64 + '"', "processor source identity"),
            ('name: Some("Signed")', 'name: Some("Wrong")', "omission name"),
        )
        for before, after, message in changes:
            with self.subTest(message=message):
                self.assertIn(before, self.source)
                self.assert_rejected(self.source.replace(before, after, 1), message)

    def test_native_row_mutation_rejects_stale_artifact(self):
        changed = copy.deepcopy(self.document)
        changed["modules"]["Fixture"]["tables"]["Serial"]["tags"]["0"]["Name"] = "Renamed"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(self.source, encoding="utf-8")
            result = audit.audit(changed, audit.parse_artifact(path))
        self.assertFalse(result.ok)
        self.assertTrue(any("name/format/count" in problem for problem in result.mismatches), result.mismatches)

    def test_nested_module_uses_native_first_component_group_default(self):
        document = fixture_document()
        nested = document["modules"].pop("Fixture")
        document["modules"]["Fixture::Nested"] = nested
        source = emitted(document)
        self.assertIn('group1: "Fixture"', source)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(source, encoding="utf-8")
            result = audit.audit(document, audit.parse_artifact(path))
        self.assertTrue(result.ok, result.mismatches)

    def test_native_processor_identity_and_single_variant_mutations_reject_stale_artifact(self):
        processor_changed = copy.deepcopy(self.document)
        processor_changed["modules"]["Fixture"]["tables"]["Serial"]["meta"]["PROCESS_PROC"]["__deparse"] += "\n"
        single_variant = copy.deepcopy(self.document)
        direct = single_variant["modules"]["Fixture"]["tables"]["Serial"]["tags"]["0"]
        single_variant["modules"]["Fixture"]["tables"]["Serial"]["tags"]["0"] = {"_variants": [direct]}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(self.source, encoding="utf-8")
            parsed = audit.parse_artifact(path)
            processor_result = audit.audit(processor_changed, parsed)
            variant_result = audit.audit(single_variant, parsed)
        self.assertTrue(any("processor source identity" in problem for problem in processor_result.mismatches), processor_result.mismatches)
        self.assertTrue(any("variant identity" in problem for problem in variant_result.mismatches), variant_result.mismatches)

    def test_unknown_artifact_schema_fails_loudly(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(self.source.replace("default_format:", "unexpected:", 1), encoding="utf-8")
            with self.assertRaisesRegex(audit.SerialVerificationError, "schema drift"):
                audit.parse_artifact(path)


@unittest.skipUnless(PINNED_DUMP, "set OXIDEX_TABLES_JSON to replay the full recorded serial population")
class RecordedSerialArtifactVerifier(unittest.TestCase):
    def test_fresh_emitted_artifact_matches_full_recorded_population(self):
        document = json.loads(Path(PINNED_DUMP).read_text(encoding="utf-8"))
        source = emitted(document)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(source, encoding="utf-8")
            result = audit.audit(document, audit.parse_artifact(path))
        self.assertTrue(result.ok, result.mismatches)
        self.assertEqual((result.expected_tables, result.expected_alternatives), (8, 132))
        self.assertEqual((result.emitted_tables, result.emitted_alternatives, result.omitted_alternatives), (8, 106, 26))

    def test_cli_rejects_recorded_population_forged_as_descriptor_refusals(self):
        document = json.loads(Path(PINNED_DUMP).read_text(encoding="utf-8"))
        native = audit.native_population(document)
        sidecars = "\n".join(
            f'    OmittedSerialNativeTable {{ module: "{module}", table: "{table}", reasons: &["serial_descriptor_refused"] }},'
            for module, table in sorted(native)
        )
        forged = """use super::*;
pub static ALL_SERIAL_TABLES: &[&SerialTable] = &[];
pub static OMITTED_SERIAL_NATIVE_ROWS: &[OmittedSerialNativeRow] = &[];
pub static OMITTED_SERIAL_NATIVE_TABLES: &[OmittedSerialNativeTable] = &[
""" + sidecars + "\n];\n"
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "serial_tables.rs"
            artifact.write_text(forged, encoding="utf-8")
            run = subprocess.run(
                [sys.executable, str(Path(audit.__file__)), str(artifact), PINNED_DUMP],
                text=True, capture_output=True, timeout=15,
            )
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertEqual(run.stdout.count("\"expected_tables\": 8"), 1)
        self.assertEqual(run.stderr.count("wholly omitted without independent native proof"), 8)


if __name__ == "__main__":
    unittest.main()
