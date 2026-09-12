"""Regression controls for enum facts, excluding comment/string examples."""
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

import check_hand_enum_drift as drift


class HandEnumDriftTests(unittest.TestCase):
    def extract(self, source):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "facts.rs"
            path.write_text(source)
            return drift.extract_rust_pairs(path)

    def test_comment_examples_are_not_facts(self):
        self.assertEqual(self.extract('''
// (1, "Line example")
/// ("Canon EOS-1D", "Canon EOS-1Ds")
/* (2, "Block example") /* (3, "Nested comment") */ */
const PAIRS: &[(i32, &str)] = &[(4, "Real")];
'''), {("4", "Real")})

    def test_raw_string_examples_are_not_facts(self):
        self.assertEqual(self.extract('''
const HELP: &str = r#"(1, "Example")"#;
const DOC: &str = r###" ("key", "Example") "###;
const PAIRS: &[(i32, &str)] = &[(2, "Real")];
'''), {("2", "Real")})

    def test_escaped_string_examples_are_not_facts(self):
        self.assertEqual(self.extract(r'''
const HELP: &str = "(1, \"Example\")";
const PAIRS: &[(i32, &str)] = &[(2, "Real")];
'''), {("2", "Real")})

    def test_real_integer_and_string_keys_keep_their_values(self):
        self.assertEqual(self.extract(r'''
const A: &[(i32, &str)] = &[
    (0x10u32, "Sixteen"), (-1, "Off"), (2, "Quoted \"value\""),
];
const B: &[(&str, &str)] = &[("03", "Three"), ("model", "Body")];
'''), {("16", "Sixteen"), ("-1", "Off"), ("2", 'Quoted "value"'),
       ("3", "Three"), ("model", "Body")})

    def test_table_lookup_arguments_are_not_facts(self):
        # canon.rs: find_ifd_table("Canon", "Main") (c7d7b71d) and two
        # find_table calls read as ("Canon", "Main")-style rust-only facts;
        # the first took the tip's canon.rs from 235 to 236 [REGRESSED].
        self.assertEqual(self.extract('''
const PAIRS: &[(i32, &str)] = &[(1, "Real")];
fn lookups() {
    let a = find_table("Canon", "FileInfo");
    let b = crate::exiftool_tables::find_ifd_table("Canon", "Main");
    let c = exiftool_tables::find_unemitted_table ("Canon", "Unemitted");
    let d = Some(find_table("Canon", "CameraSettings"));
}
'''), {("1", "Real")})

    def test_other_call_and_macro_arguments_still_count(self):
        # The exclusion is the table lookups by name, not "any call": a fact
        # written through a constructor or macro must stay visible, because
        # dropping one is silent while over-counting fails loudly.
        self.assertEqual(self.extract('''
const A: Pc = Pc::Entry(1, "Off");
const B: Pc = pc!(2, "On");
fn f() { not_find_table("Canon", "Main"); find_tables("Nikon", "Main"); }
'''), {("1", "Off"), ("2", "On"), ("Canon", "Main"), ("Nikon", "Main")})

    def run_check(self, source, expected_count=0):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "facts.rs").write_text(source)
            (root / "dump.json").write_text(json.dumps({"modules": {"Test": {
                "tables": {"Main": {"tags": {"1": {"PrintConv": {
                    "kind": "enum", "map": {"1": "Supported"},
                }}}}},
            }}}))
            baseline = json.dumps({"facts.rs": {"rust_only_count": expected_count}})
            (root / "baseline.json").write_text(baseline)
            result = subprocess.run([
                sys.executable, str(pathlib.Path(drift.__file__).resolve()),
                "--dump", "dump.json", "--baseline", "baseline.json",
                "facts.rs=Test",
            ], cwd=root, capture_output=True, text=True)
            self.assertEqual((root / "baseline.json").read_text(), baseline)
            return result

    def test_supported_actual_tuple_and_fake_examples_pass(self):
        result = self.run_check('''
// (2, "Fake")
const HELP: &str = r#"(3, "Fake")"#;
const PAIRS: &[(i32, &str)] = &[(1, "Supported")];
''')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("rust_pairs=1", result.stdout)

    def test_table_lookup_does_not_regress_the_drift_check(self):
        result = self.run_check('''
const PAIRS: &[(i32, &str)] = &[(1, "Supported")];
fn engine() { find_ifd_table("Test", "Main"); }
''')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("rust_pairs=1 dump_pairs=1 rust_only=0 [OK]", result.stdout)

    def test_actual_incorrect_tuple_still_fails_drift_check(self):
        result = self.run_check('''
// (2, "Fake")
const PAIRS: &[(i32, &str)] = &[(1, "Wrong")];
''')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("REGRESSED (baseline 0)", result.stdout)
        self.assertIn("('1', 'Wrong')", result.stdout)

    def test_extra_actual_tuple_above_reviewed_baseline_still_fails(self):
        result = self.run_check('''
const PAIRS: &[(i32, &str)] = &[(1, "Supported"), (2, "Existing"), (3, "New")];
''', expected_count=1)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("rust_only=2 [REGRESSED (baseline 1)]", result.stdout)


if __name__ == "__main__":
    unittest.main()
