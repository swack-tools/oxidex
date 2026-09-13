"""Mutation checks for independent word artifact parsing and authentication."""

import copy
import tempfile
from pathlib import Path
import unittest

import verify
import verify_word_directory as audit
import word_directory
from test_native_reader_contract import snapshot
from test_word_directory import fixture_processor


class WordArtifact(unittest.TestCase):
    def setUp(self):
        self.reader = snapshot()
        self.processor = fixture_processor()
        self.compiled = word_directory.compile_word_directory(
            self.processor, {"unsigned16": self.reader}
        )
        # Compiler use is confined to test input generation. The verifier
        # imports no recognizer and re-reads only the emitted artifact.
        self.literal = "KeyedLayout::LengthPrefixedU16Pairs(" + self.compiled.rust(
            lambda text: text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")
        ) + ")"
        self.parsed = audit.parse_rust(self.literal, verify.unescape)

    def test_complete_artifact_and_source_bindings(self):
        self.assertIsNone(audit.source_mismatch(self.parsed, self.processor, {"unsigned16": self.reader}))
        self.assertEqual(self.parsed.fields["value_mask"], 255)
        self.assertIsNone(audit.parse_rust("KeyedLayout::Ciff10", verify.unescape))

    def test_numeric_value_is_not_truncated_to_handler_format(self):
        parsed = audit.parse_rust(self.literal.replace("value_mask: 255", "value_mask: 511"), verify.unescape)
        self.assertEqual(parsed.fields["value_mask"], 511)
        self.assertEqual(parsed.fields["value_format"], "int8u")
        self.assertNotEqual(parsed.fingerprint(), self.parsed.fingerprint())

    def test_source_identity_is_not_operand_execution_proof(self):
        changed = audit.parse_rust(self.literal.replace("value_mask: 255", "value_mask: 511"), verify.unescape)
        # This narrowly named check authenticates identity. The separate
        # native replay must catch changed executable operands.
        self.assertIsNone(audit.source_mismatch(changed, self.processor, {"unsigned16": self.reader}))
        self.assertNotEqual(changed.fingerprint(), self.parsed.fingerprint())

    def test_schema_drift_duplicate_fields_and_invalid_domains_fail(self):
        changes = (
            ("pair_start: 2", "pair_start: 0"),
            ("pair_stride: 2", "pair_stride: 0"),
            ("key_shift: 8", "key_shift: 16"),
            ("value_mask: 255", "value_mask: 65536"),
            ("index_divisor: 2", "index_divisor: 0"),
            ("index_bias: 1", "index_bias: 2"),
            ("pair_stride: 2", "pair_stride: 3"),
            ("exact_length_first: true", "exact_length_first: false"),
            ("missing_model_as_empty: true", "missing_model_as_empty: false"),
            ("value_format: Fmt::Int8u", "value_format: Fmt::Int16u"),
            ("value_count: 1", "value_count: 0"),
            ("pair_start: 2", "pair_start: 2, pair_start: 2"),
            ("pair_start: 2", "unrecognized: 2"),
            ('member: "Model"', 'member: "Make"'),
            ('source_file: "Image/ExifTool/Fixture.pm"', 'source_file: "../Fixture.pm"'),
        )
        for before, after in changes:
            with self.subTest(before=before, after=after), self.assertRaises(audit.WordVerificationError):
                audit.parse_rust(self.literal.replace(before, after), verify.unescape)

    def test_commas_braces_and_escaped_quotes_in_strings(self):
        changed = self.literal.replace('invalid_warning: "Invalid word data"',
                                       'invalid_warning: "Bad, {word} \\"quoted\\""')
        self.assertEqual(audit.parse_rust(changed, verify.unescape).fields["invalid_warning"], 'Bad, {word} "quoted"')

    def test_rustfmt_layout_and_trailing_commas(self):
        formatted = self.literal.replace("WordDirectory { ", "WordDirectory {\n    ")
        formatted = formatted[:-2] + ",\n},\n)"
        self.assertEqual(audit.parse_rust(formatted, verify.unescape), self.parsed)

    def test_unresolved_changed_source_and_wrong_package_binding_fail(self):
        for key, value in (("resolved", False), ("source_sha256", "b" * 64),
                           ("source_file", "Image/ExifTool/Other.pm"),
                           ("__deparse", self.processor["__deparse"] + "\n"),
                           ("dependencies", {})):
            changed = copy.deepcopy(self.processor)
            changed[key] = value
            with self.subTest(key=key):
                self.assertIsNotNone(audit.source_mismatch(self.parsed, changed, {"unsigned16": self.reader}))

    def test_same_body_rebound_reader_is_detected(self):
        changed = copy.deepcopy(self.processor)
        changed["dependencies"]["Image::ExifTool::Fixture::Get16u"]["__name"] = "Image::ExifTool::Get32u"
        self.assertEqual(changed["__deparse"], self.processor["__deparse"])
        self.assertIn("rebound", audit.source_mismatch(self.parsed, changed, {"unsigned16": self.reader}))

    def test_bad_native_execution_observation_is_not_accepted(self):
        changed = copy.deepcopy(self.reader)
        changed["get16u_probe"]["failure_count"] = 1
        self.assertIsNotNone(audit.source_mismatch(self.parsed, self.processor, {"unsigned16": changed}))

    def test_artifact_table_parser_preserves_layout_and_zero_row_table(self):
        text = ('pub static WORD: KeyedDirectoryTable = KeyedDirectoryTable { '
                'module: "Fixture", table: "Empty", group0: "Fixture", group1: "", group2: "Other", '
                f'layout: {self.literal}, gate_a: GateA {{ blocked_by: &[] }}, tags: &[], variants: &[] }};')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "keyed.rs"
            path.write_text(text)
            parsed = verify.parse_keyed_rust(path)
            self.assertEqual(parsed.table_scope, {("Fixture", "Empty")})
            self.assertEqual(parsed.layouts[("Fixture", "Empty")], self.parsed)
            self.assertEqual(parsed.table_gates[("Fixture", "Empty")], ())
            path.write_text(text + "\n" + text)
            with self.assertRaisesRegex(SystemExit, "duplicate generated keyed table"):
                verify.parse_keyed_rust(path)


if __name__ == "__main__":
    unittest.main()
