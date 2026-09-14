"""Actual interpreter primitive controls, independent of ExifTool fixtures."""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from checkexif_recipes import RecipeRefused
from utf8_primitive_contract import validate_join, validate_snapshot

BASE = Path(__file__).resolve().parent
PERL = os.environ.get("EXIFTOOL_PERL", "/usr/bin/perl")


def capture(override="", diagnostics=None):
    program = """
        use JSON::PP; use OxiDex::Utf8PrimitiveContract;
        require Encode;
        my $pristine = OxiDex::Utf8PrimitiveContract::capture_pristine($^X);
    """ + override + """
        my $final = OxiDex::Utf8PrimitiveContract::capture_final();
        print JSON::PP->new->canonical->encode({kind=>'utf8_primitive_join_v1',
            pristine=>$pristine, final=>$final});
    """
    env = {key: value for key, value in os.environ.items()
           if not (key.startswith("PERL5") or key in {"PERLLIB", "OXIDEX_UTF8_EVIDENCE_DIR"})}
    if diagnostics is not None:
        env["OXIDEX_UTF8_EVIDENCE_DIR"] = str(diagnostics)
    result = subprocess.run([PERL, "-I", str(BASE), "-e", program],
                            env=env, capture_output=True, timeout=45, check=True)
    return json.loads(result.stdout)


class NativeUtf8Contract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.baseline = capture()

    def test_actual_pristine_and_final_match(self):
        self.assertEqual(validate_join(self.baseline), self.baseline["final"])

    def test_overrides_in_the_final_process_refuse(self):
        for override in (
            "no warnings; *Encode::encode = sub { 'changed' };",
            "no warnings; *Encode::is_utf8 = sub { 0 };",
            "no warnings; *Encode::utf8::encode = sub { 'changed' };",
            "package Other; sub name { 'utf8' } sub encode { 'changed' } package main; "
            "$Encode::Encoding{'utf8'} = bless {}, 'Other';",
        ):
            with self.subTest(override=override), self.assertRaises(RecipeRefused):
                validate_join(capture(override))

    def test_empty_duplicate_and_changed_vectors_refuse(self):
        for mutation in ("empty", "duplicate", "undefined", "expected", "warning", "flag"):
            doc = deepcopy(self.baseline["final"])
            if mutation == "empty":
                doc["vectors"] = []
            elif mutation == "duplicate":
                doc["vectors"].append(doc["vectors"][0])
            elif mutation == "undefined":
                row = next(row for row in doc["vectors"] if row["name"] == "undefined")
                row["is_utf8"]["result"]["bytes_hex"] = "31"
            elif mutation == "expected":
                doc["vectors"][0]["expected_utf8_hex"] = "forged"
            elif mutation == "warning":
                doc["vectors"][0]["encode_utf8"]["warning"] = "unexpected"
            else:
                doc["vectors"][0]["input"]["utf8_flag"] = 1
            with self.subTest(mutation=mutation), self.assertRaises(RecipeRefused):
                validate_snapshot(doc)

    def test_missing_provider_refuses_without_loading_it(self):
        with self.assertRaises(RecipeRefused):
            validate_join(capture("delete $INC{'Encode.pm'};"))

    def test_raw_diagnostics_are_external_and_portable_data_has_no_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            doc = capture(diagnostics=directory)
            validate_join(doc)
            files = list(Path(directory).iterdir())
            self.assertEqual(len(files), 2)
            self.assertTrue(any("executable_sha256" in p.read_text() for p in files))
            rendered = json.dumps(doc)
            self.assertNotIn(str(BASE), rendered)
            self.assertNotIn("provider_file", rendered)
            self.assertNotIn("executable_sha256", rendered)
            self.assertNotIn("path", rendered)


if __name__ == "__main__":
    unittest.main()
