"""Source-fact coverage for fully qualified SubDirectory.Validate helpers."""

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
DUMP = REPO_ROOT / "tools/exiftool-tables/dump_tables.pl"


class ValidateFunctionFacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = Path(self.tmp.name) / "lib"
        package = self.lib / "Image/ExifTool"
        package.mkdir(parents=True)
        (self.lib / "Image/ExifTool.pm").write_text(
            "package Image::ExifTool; our $VERSION = 'fixture'; 1;\n",
            encoding="utf-8",
        )
        self.module = package / "Fixture.pm"
        self.later = package / "Later.pm"
        self.write_later_module()
        self.write_module("return $data == $first ? 1 : undef;")

    def write_later_module(self):
        self.later.write_text(textwrap.dedent("""\
            package Image::ExifTool::Later;
            sub Validate { return 1; }
            1;
        """), encoding="utf-8")

    def write_module(self, body):
        self.module.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::Fixture;
            sub Validate {{
                my ($data, $offset, $first) = @_;
                {body}
            }}
            our %Main = (
                1 => {{ SubDirectory => {{
                    Validate => 'Image::ExifTool::Fixture::Validate($dirData,$subdirStart,$size)'
                }} }},
                2 => [
                    {{ SubDirectory => {{
                        Validate => 'Image::ExifTool::Fixture::Validate($dirData,$subdirStart,$size-2,$size)'
                    }} }},
                    {{ SubDirectory => {{
                        Validate => 'Image::ExifTool::Ghost::Missing($dirData,$subdirStart,$size)'
                    }} }},
                ],
                3 => {{ Flags => {{ SubDirectory => {{
                    Validate => 'Image::ExifTool::Fixture::Validate($dirData,$subdirStart,$size)'
                }} }} }},
                4 => {{ SubDirectory => {{
                    Validate => 'Image::ExifTool::Later::Validate($dirData,$subdirStart,$size)'
                }} }},
            );
            our @ArrayOnly = (
                {{ SubDirectory => {{
                    Validate => 'Image::ExifTool::Fixture::ArrayValidate($dirData,$subdirStart,$size)'
                }} }},
            );
            *ArrayValidate = \\&Validate;
            1;
        """), encoding="utf-8")

    def dump(self):
        result = subprocess.run(
            ["/usr/bin/perl", str(DUMP), str(self.lib), "Fixture", "Later"],
            check=True, text=True, capture_output=True,
        )
        return json.loads(result.stdout)

    def test_collects_loaded_helpers_from_scalar_variant_and_flags(self):
        dump = self.dump()
        facts = dump["subdirectory_validate_functions"]
        helper = facts["Image::ExifTool::Fixture::Validate"]
        self.assertEqual(helper["__perl"], "CODE")
        self.assertTrue(helper["__opaque"])
        self.assertEqual(helper["__name"], "Image::ExifTool::Fixture::Validate")
        self.assertTrue(helper["resolved"])
        self.assertIn("package Image::ExifTool::Fixture", helper["__deparse"])
        self.assertEqual(helper["source_file"], "Image/ExifTool/Fixture.pm")
        self.assertEqual(
            helper["source_sha256"],
            hashlib.sha256(self.module.read_bytes()).hexdigest(),
        )

        # The source alias is preserved as the map key while the fact records
        # the CODE ref's canonical name. This ARRAY package variable would be
        # invisible if discovery only walked hash table rows.
        array_alias = facts["Image::ExifTool::Fixture::ArrayValidate"]
        self.assertTrue(array_alias["resolved"])
        self.assertEqual(array_alias["__name"], "Image::ExifTool::Fixture::Validate")

        # Fixture is requested before Later. Resolution after all loads must
        # still capture Later's CODE ref instead of caching a false unresolved.
        later = facts["Image::ExifTool::Later::Validate"]
        self.assertTrue(later["resolved"])
        self.assertEqual(later["source_file"], "Image/ExifTool/Later.pm")

        unresolved = facts["Image::ExifTool::Ghost::Missing"]
        self.assertFalse(unresolved["resolved"])
        self.assertEqual(unresolved["reason"], "code_ref_unavailable")
        self.assertIsNone(unresolved["__deparse"])
        self.assertIsNone(unresolved["source_file"])
        self.assertIsNone(unresolved["source_sha256"])

    def test_helper_source_edit_changes_fact_not_loaded_table_data(self):
        before = self.dump()
        self.write_module("return $data != $first ? 1 : undef;")
        after = self.dump()
        name = "Image::ExifTool::Fixture::Validate"
        first = before["subdirectory_validate_functions"][name]
        second = after["subdirectory_validate_functions"][name]
        self.assertTrue(first["resolved"])
        self.assertTrue(second["resolved"])
        self.assertNotEqual(first["__deparse"], second["__deparse"])
        self.assertNotEqual(first["source_sha256"], second["source_sha256"])
        self.assertEqual(before["modules"], after["modules"])


if __name__ == "__main__":
    unittest.main()
