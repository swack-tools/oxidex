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
        self.assertEqual((result.emitted_tables, result.emitted_alternatives, result.omitted_alternatives), (1, 6, 0))

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
            ('name: "Signed"', 'name: "Wrong"', "name/format/count"),
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

    def test_condition_raw_effect_and_print_conversion_mutations_reject_stale_artifact(self):
        changes = (
            ("condition", "3:0", "Condition", '$self->{Model} ne "EOS"'),
            ("raw conversion", "0", "RawConv", {"kind": "expr", "expr": "$$self{Other}=$val"}),
            ("print conversion", "2", "PrintConv", {
                "kind": "expr", "expr": "Image::ExifTool::DecodeBits($val, undef, 32)",
            }),
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(self.source, encoding="utf-8")
            parsed = audit.parse_artifact(path)
            for label, index, key, value in changes:
                with self.subTest(label=label):
                    changed = copy.deepcopy(self.document)
                    tag = changed["modules"]["Fixture"]["tables"]["Serial"]["tags"]
                    if ":" in index:
                        raw_index, alternative = index.split(":", 1)
                        tag[raw_index]["_variants"][int(alternative)][key] = value
                    else:
                        tag[index][key] = value
                    result = audit.audit(changed, parsed)
                    self.assertFalse(result.ok, result.mismatches)
                    self.assertTrue(any("differ" in problem or "refused" in problem
                                        for problem in result.mismatches), result.mismatches)

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
    def _audit_text(self, document, source):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "serial_tables.rs"
            path.write_text(source, encoding="utf-8")
            return audit.audit(document, audit.parse_artifact(path))

    def test_fresh_emitted_artifact_matches_full_recorded_population(self):
        document = json.loads(Path(PINNED_DUMP).read_text(encoding="utf-8"))
        source = emitted(document)
        result = self._audit_text(document, source)
        self.assertTrue(result.ok, result.mismatches)
        self.assertEqual((result.expected_tables, result.expected_alternatives), (8, 132))
        self.assertEqual((result.emitted_tables, result.emitted_alternatives, result.omitted_alternatives), (8, 122, 10))

    def test_afinfo2_source_operand_mutations_regenerate_and_reject_stale_artifact(self):
        """Fresh source facts adapt; the independent native audit rejects stale Rust.

        Each mutation changes an actual AFInfo2 operand accepted by the
        compiler.  The test therefore proves more than a generator literal:
        `verify_serial_directory` reinterprets native source and catches the
        old artifact without importing the emitter.
        """
        document = json.loads(Path(PINNED_DUMP).read_text(encoding="utf-8"))
        baseline = emitted(document)
        tag = document["modules"]["Canon"]["tables"]["AFInfo2"]["tags"]
        mutations = (
            ("integer enum known value", lambda tags: tags["1"].__setitem__("PrintConv", {
                "kind": "enum", "directives": None,
                "map": {**tag["1"]["PrintConv"]["map"], "2": "Changed mode"},
            })),
            ("SetMember destination", lambda tags: tags["2"].__setitem__("RawConv", {
                "kind": "expr", "expr": "$$self{OtherCount} = $val",
            })),
            ("trailing count addition", lambda tags: tags["13"]["_variants"][1].__setitem__(
                "Format", "int16s[int(($val{2}+15)/16)+2]",
            )),
        )
        for label, mutate in mutations:
            with self.subTest(label=label):
                changed = copy.deepcopy(document)
                changed_tag = changed["modules"]["Canon"]["tables"]["AFInfo2"]["tags"]
                mutate(changed_tag)
                # The compiler is expected to regenerate a different, still
                # independently auditable artifact for each supported operand.
                fresh = emitted(changed)
                self.assertNotEqual(fresh, baseline)
                fresh_result = self._audit_text(changed, fresh)
                self.assertTrue(fresh_result.ok, fresh_result.mismatches)
                stale_result = self._audit_text(changed, baseline)
                self.assertFalse(stale_result.ok, stale_result.mismatches)
                self.assertTrue(
                    any("differ" in problem for problem in stale_result.mismatches),
                    stale_result.mismatches,
                )

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
