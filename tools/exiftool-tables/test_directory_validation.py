"""Closed helper grammar and source-driven call changes, before runtime wiring."""
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
import shutil
import subprocess

import directory_validation as validation
import keyed_directory
import verify


BODY = '''($$@) {
    package Image::ExifTool::Canon;
    use strict;
    (my($dataPt, $offset, @vals) = @_);
    (my $dataVal = &Image::ExifTool::Get16u($dataPt, $offset));
    my($val);
    foreach $val (@vals) {
        (($val == $dataVal) and (return 1));
    }
    (return (undef));
}'''
NAME = "Image::ExifTool::Canon::Validate"
CALL = NAME + "($dirData,$subdirStart,$size)"


def helper(body=BODY):
    return {"__perl": "CODE", "resolved": True, "__name": NAME,
            "__deparse": body, "source_file": "Image/ExifTool/Canon.pm",
            "source_sha256": "1" * 64}


class HelperBody(unittest.TestCase):
    def test_captured_body_and_deparser_scalar_binder_variation(self):
        validation.authenticate_body(BODY)
        validation.authenticate_body(BODY.replace("my $dataVal", "my($dataVal)"))

    def test_local_variable_and_package_names_are_not_the_contract(self):
        moved = BODY.replace("Image::ExifTool::Canon;", "Other::Package;")
        for before, after in (("$dataPt", "$bytes"), ("$offset", "$where"),
                              ("@vals", "@choices"), ("$dataVal", "$word"), ("$val", "$choice")):
            moved = moved.replace(before, after)
        validation.authenticate_body(moved)

    def test_changed_semantics_and_hidden_side_effects_are_refused(self):
        mutations = [
            BODY.replace("Get16u", "Get32u"),
            BODY.replace("return 1", "return 0"),
            BODY.replace("$val == $dataVal", "$val == $offset"),
            BODY.replace("(($val == $dataVal) and (return 1))", "($val == ($dataVal and (return 1)))"),
            BODY.replace("my($val);", "my($val); ($ENV{X} = 1);"),
            BODY.replace("my $dataVal", "my $offset"),
            BODY + " system('anything');",
        ]
        for body in mutations:
            with self.subTest(body=body), self.assertRaises(validation.ValidationRefused):
                validation.authenticate_body(body)


class CallSource(unittest.TestCase):
    def compile(self, call=CALL, facts=None):
        return validation.compile_validation(call, {NAME: helper()} if facts is None else facts)

    def test_source_arguments_supply_offset_alternatives_and_division(self):
        plain = self.compile()
        self.assertEqual((plain.offset, plain.expected), (0, (("Relative", 0),)))
        changed = self.compile(NAME + "( $dirData, $subdirStart + 2, $size - 2, $size, 0x2c, $size / 2 )")
        self.assertEqual(changed.offset, 2)
        self.assertEqual(changed.expected, (("Relative", -2), ("Relative", 0), ("Constant", 44), ("Quotient", 2)))

    def test_function_alias_still_requires_the_actual_captured_body(self):
        alias = "Different::Module::Check"
        result = validation.compile_validation(CALL.replace(NAME, alias), {alias: helper()})
        self.assertEqual(result.callee, alias)
        with self.assertRaises(validation.ValidationRefused):
            validation.compile_validation(CALL.replace(NAME, alias), {alias: helper(BODY.replace("return 1", "return 0"))})

    def test_missing_or_unresolved_provenance_never_admits_a_name(self):
        for facts in ({}, {NAME: {"__name": NAME}}, {NAME: {**helper(), "resolved": False}},
                      {NAME: {**helper(), "source_file": "../Canon.pm"}},
                      {NAME: {**helper(), "source_sha256": ""}}):
            with self.subTest(facts=facts), self.assertRaises(validation.ValidationRefused):
                self.compile(facts=facts)

    def test_context_scope_and_literal_domain_are_closed(self):
        bad = ["$size/0", "$size/012", "$size/2/2", "$si ze", "$size,", "012", "08", "-1",
               "$size+2147483648", "4294967296", "$size;system('anything')", "$count"]
        for expected in bad:
            with self.subTest(expected=expected), self.assertRaises(validation.ValidationRefused):
                self.compile(NAME + "($dirData,$subdirStart," + expected + ")")
        for pointer in ("$other", "$dir Data"):
            with self.assertRaises(validation.ValidationRefused):
                self.compile(CALL.replace("$dirData", pointer))


class GeneratedValidation(unittest.TestCase):
    def artifact(self, root, call=CALL, fact=None):
        native = {"subdirectory_validate_functions": {NAME: helper() if fact is None else fact},
                  "modules": {"Any": {"tables": {
                      "Main": {"meta": {"PROCESS_PROC": {"__perl": "CODE", "__name": "Image::ExifTool::CanonRaw::ProcessCanonRaw"}},
                               "tags": {"0x1001": {"Name": "Child", "SubDirectory": {
                                   "TagTable": "Image::ExifTool::Any::Child", "Validate": call}}}},
                      "Child": {"meta": {"PROCESS_PROC": {"__perl": "CODE", "__name": "Image::ExifTool::ProcessBinaryData"}}, "tags": {}},
                  }}}}
        path = root / "keyed.rs"
        path.write_text(keyed_directory.generate(native)[0])
        return path

    def audit(self, path, call=CALL, sha="1" * 64):
        source = "\n".join([
            "KEYED\tAny\tMain\t\tTGROUPS\t\t\t",
            "KEYED\tAny\tMain\t4097\tNAME\tChild",
            "KEYED\tAny\tMain\t4097\tFLAGS\t0\t0\t0\t0\t0\t-",
            "KEYED\tAny\tMain\t4097\tSUBDIR\tImage::ExifTool::Any::Child\t\t1\t",
            "KEYED\tAny\tMain\t4097\tVALIDATION\t" + "\t".join((call, NAME, "Image/ExifTool/Canon.pm", sha)),
        ])
        return verify.keyed_native_inventory(
            verify.parse_keyed_rust(path), verify.parse_omitted_keyed_native_rows(path),
            *verify.parse_keyed_oracle(source))

    def test_real_generated_edge_roundtrips_and_source_mutations_reject_stale_artifact(self):
        with TemporaryDirectory() as tmp:
            path = self.artifact(Path(tmp))
            self.assertEqual(self.audit(path).keyed_fact_mismatches, ())
            changed = NAME + "($dirData,$subdirStart+2,$size-2,$size,0x2c,$size/2)"
            self.assertTrue(self.audit(path, call=changed).keyed_fact_mismatches)
            self.assertTrue(self.audit(path, sha="2" * 64).keyed_fact_mismatches)
            path = self.artifact(Path(tmp), call=changed)
            self.assertEqual(self.audit(path, call=changed).keyed_fact_mismatches, ())
            if shutil.which("rustfmt"):
                before = verify.parse_keyed_rust(path)
                subprocess.run(["rustfmt", "--edition", "2024", str(path)], check=True)
                self.assertEqual(verify.parse_keyed_rust(path), before)
                self.assertEqual(self.audit(path, call=changed).keyed_fact_mismatches, ())

    def test_absent_or_changed_helper_keeps_child_explicitly_unwalked(self):
        for fact in ({}, helper(BODY.replace("Get16u", "Get32u"))):
            with self.subTest(fact=fact), TemporaryDirectory() as tmp:
                path = self.artifact(Path(tmp), fact=fact)
                edge = verify.parse_keyed_rust(path).facts[("Any", "Main", "4097")].edge
                self.assertIn("validate", edge[3])
                self.assertIsNone(edge[4])
                self.assertEqual(self.audit(path).keyed_fact_mismatches, ())

    def test_artifact_operand_and_unconsumed_expectation_are_rejected(self):
        with TemporaryDirectory() as tmp:
            path = self.artifact(Path(tmp))
            source = path.read_text()
            path.write_text(source.replace("SizeExpectation::Relative(0)", "SizeExpectation::Relative(1)"))
            self.assertTrue(self.audit(path).keyed_fact_mismatches)
            path.write_text(source.replace("SizeExpectation::Relative(0)", "SizeExpectation::Relative(0), surprise()"))
            with self.assertRaises(SystemExit):
                verify.parse_keyed_rust(path)

    def test_outer_helper_proof_never_clears_the_native_reader_blocker(self):
        with TemporaryDirectory() as tmp:
            path = self.artifact(Path(tmp))
            source = path.read_text()
            edge = verify.parse_keyed_rust(path).facts[("Any", "Main", "4097")].edge
            self.assertIn("validate_reader_contract", edge[3])
            path.write_text(source.replace('"validate_reader_contract"', ''))
            failures = self.audit(path).keyed_fact_mismatches
            self.assertTrue(any("validate_reader_contract" in why for _, why in failures))


if __name__ == "__main__":
    unittest.main()
