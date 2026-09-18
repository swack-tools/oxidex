"""module_absence.prove_entry_absent: one tag id absent from a present table.

The third granularity, after a whole module and a named table. 11.78's
Sony::Main lacks 0x2032..0x2039, 0x204a and 0x205c; 12.64's lacks 0x204a and
0x205c. The absence is recorded only when the release's own perl, loading the
release's own lib/, has no such key in the table AND the id is named nowhere
in the module's source. An id present in either, changed or not, refuses; a
missing table or module refuses as the coarser case. The version label is
never the proof.
"""

import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from module_absence import (  # noqa: E402
    ENTRY_SCHEMA, NotAbsent, loaded_tag_ids_sha256, prove_entry_absent, prove_table_absent,
    validate_entry_absence_record)

PERL = os.environ.get("EXIFTOOL_PERL") or shutil.which("perl")
SOURCES = Path(os.environ.get(
    "OXIDEX_TEST_EXIFTOOL_SOURCES",
    "/Users/allen/Documents/Codex/2026-09-10/oxidex-worktree-cleanup-audit/handoff-continuation/"
    "sony-plain-recovery-z2ml2k_w/shared-pilot/write-upgrade-integration-20260913/"
    "first-random-pair-20260913/source-attempt-01/sources"))
LIB_1178 = SOURCES / "exiftool-11.78-ca8685788f5763c547349f239764bd19cf1952da/lib"
LIB_1264 = SOURCES / "exiftool-12.64-d35e9e26e0a8b443dae307f55d0a4a067d311a16/lib"

MAIN = """package Image::ExifTool::Sony;
%Image::ExifTool::Sony::Main = (
    GROUPS => { 0 => 'MakerNotes' },
    0x2031 => { Name => 'SerialNumber' },
    0xb041 => { Name => 'ExposureMode' },
{extra});
{after}
1;
"""


def make_lib(root: Path, sony: str | None = MAIN, extra: str = "", after: str = "") -> Path:
    """A loadable release lib/: Image::ExifTool plus, optionally, a Sony module."""
    lib = root / "lib"
    (lib / "Image/ExifTool").mkdir(parents=True)
    (lib / "Image/ExifTool.pm").write_text("package Image::ExifTool;\n$VERSION = '11.78';\n1;\n")
    if sony is not None:
        (lib / "Image/ExifTool/Sony.pm").write_text(sony.replace("{extra}", extra).replace("{after}", after))
    return lib


@unittest.skipUnless(PERL, "needs a perl")
class ProveEntryAbsent(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def prove(self, lib, tag_id, table="Main"):
        return prove_entry_absent(lib, "Sony", table, tag_id, PERL)

    def test_absent_id_is_recorded_with_loaded_and_source_proof(self):
        lib = make_lib(self.tmp)
        record = self.prove(lib, 0x2032)
        self.assertEqual(record["kind"], ENTRY_SCHEMA)
        self.assertEqual((record["module"], record["table"], record["tag_id"], record["tag_id_hex"]),
                         ("Sony", "Main", 0x2032, "0x2032"))
        self.assertIs(record["module_file_present"], True)
        self.assertIs(record["id_in_loaded_table"], False)
        self.assertEqual(record["id_occurrences_in_module"], 0)
        self.assertEqual(record["table_declarations_in_module"], 1)
        # GROUPS is table metadata, not a tag id.
        self.assertEqual(record["loaded_table_tag_ids"], 2)
        self.assertEqual(record["loaded_table_tag_ids_sha256"], loaded_tag_ids_sha256([0x2031, 0xB041]))
        self.assertEqual((record["release_files_scanned"], record["release_pm_files_scanned"]), (2, 2))
        for key in ("exiftool_pm_sha256", "module_sha256", "release_inventory_sha256"):
            self.assertRegex(record[key], r"^[0-9a-f]{64}$")
        self.assertEqual(record["release_label_not_used_as_proof"], "11.78")
        validate_entry_absence_record(record, "Sony", "Main", 0x2032)

    def test_label_is_not_the_proof(self):
        # Same table, different label: the same proof. A lying label changes nothing.
        lib = make_lib(self.tmp)
        (lib / "Image/ExifTool.pm").write_text("package Image::ExifTool;\n$VERSION = '99.99';\n1;\n")
        self.assertEqual(self.prove(lib, 0x2032)["release_label_not_used_as_proof"], "99.99")
        with self.assertRaises(NotAbsent):
            self.prove(lib, 0x2031)

    def test_present_id_refuses_even_with_a_changed_definition(self):
        for spelling in ("0x2032 => { Name => 'Changed', Format => 'int8u' },",
                         "0X2032 => { Name => 'Upper' },", "0x02032 => { Name => 'Padded' },",
                         "8242 => { Name => 'Decimal' },", "# 0x2032 - 0x2039: new in a later release"):
            with self.subTest(spelling=spelling):
                root = self.tmp / str(abs(hash(spelling)))
                lib = make_lib(root, extra="    " + spelling + "\n")
                with self.assertRaisesRegex(NotAbsent, "present in Image/ExifTool/Sony.pm"):
                    self.prove(lib, 0x2032)

    def test_present_id_already_in_the_table_refuses(self):
        with self.assertRaisesRegex(NotAbsent, "present"):
            self.prove(make_lib(self.tmp), 0x2031)
        with self.assertRaisesRegex(NotAbsent, "present"):
            self.prove(make_lib(self.tmp / "b"), 0xB041)

    def test_id_added_at_load_time_refuses_on_the_loaded_hash(self):
        # The source never spells 0x2032 or 8242, but the loaded table has it.
        lib = make_lib(self.tmp, after="$Image::ExifTool::Sony::Main{hex('2032')} = { Name => 'Built' };")
        with self.assertRaisesRegex(NotAbsent, "present in the loaded"):
            self.prove(lib, 0x2032)

    def test_missing_table_refuses_as_a_table_level_case(self):
        lib = make_lib(self.tmp)
        with self.assertRaisesRegex(NotAbsent, "prove the table absent instead"):
            self.prove(lib, 0x2032, table="Tag9416")
        # ...and the table-level proof is the one that applies there.
        self.assertEqual(prove_table_absent(lib, "Sony", "Tag9416")["table"], "Tag9416")

    def test_declared_but_empty_table_refuses_as_a_table_level_case(self):
        lib = make_lib(self.tmp, sony="package Image::ExifTool::Sony;\n"
                       "%Image::ExifTool::Sony::Main = ( GROUPS => { 0 => 'MakerNotes' } );\n1;\n")
        with self.assertRaisesRegex(NotAbsent, "prove the table absent instead"):
            self.prove(lib, 0x2032)

    def test_missing_module_refuses_as_a_module_level_case(self):
        with self.assertRaisesRegex(NotAbsent, "prove the module absent instead"):
            self.prove(make_lib(self.tmp, sony=None), 0x2032)

    def test_module_that_fails_to_load_refuses(self):
        lib = make_lib(self.tmp, after="die 'broken';")
        with self.assertRaisesRegex(NotAbsent, "could not load"):
            self.prove(lib, 0x2032)

    def test_incomplete_tree_refuses(self):
        lib = make_lib(self.tmp)
        (lib / "Image/ExifTool.pm").unlink()
        with self.assertRaises(NotAbsent):
            self.prove(lib, 0x2032)

    def test_bad_arguments_are_errors(self):
        lib = make_lib(self.tmp)
        for args in (("Sony", "Main", "0x2032"), ("Sony", "Main", -1), ("Sony", "Main", True),
                     ("../Sony", "Main", 1), ("Sony", "Ma in", 1)):
            with self.subTest(args=args), self.assertRaises(ValueError):
                prove_entry_absent(lib, *args, PERL)

    def test_validate_refuses_an_incomplete_or_mismatched_record(self):
        record = self.prove(make_lib(self.tmp), 0x2032)
        with self.assertRaises(NotAbsent):
            validate_entry_absence_record(record, "Sony", "Main", 0x2033)
        for key, value in (("id_in_loaded_table", True), ("id_occurrences_in_module", 1),
                           ("loaded_table_tag_ids", 0), ("module_sha256", None), ("kind", "x"),
                           ("loaded_table_tag_ids_sha256", "0" * 63), ("table", "Other")):
            with self.subTest(key=key):
                with self.assertRaises(NotAbsent):
                    validate_entry_absence_record({**record, key: value}, "Sony", "Main", 0x2032)
        with self.assertRaises(NotAbsent):
            validate_entry_absence_record({k: v for k, v in record.items() if k != "module_sha256"},
                                          "Sony", "Main", 0x2032)


@unittest.skipUnless(PERL and LIB_1178.is_dir() and LIB_1264.is_dir(), "needs the 11.78 and 12.64 source trees")
class HistoricalReleases(unittest.TestCase):
    """Each release's own perl-loaded Sony::Main and Sony.pm text."""

    def test_1178_lacks_the_newer_ids(self):
        for tag_id in (0x2032, 0x2033, 0x2034, 0x2035, 0x2036, 0x2037, 0x2039, 0x204A, 0x205C):
            with self.subTest(tag_id=hex(tag_id)):
                record = prove_entry_absent(LIB_1178, "Sony", "Main", tag_id, PERL)
                self.assertEqual(record["loaded_table_tag_ids"], 103)
                validate_entry_absence_record(record, "Sony", "Main", tag_id)

    def test_1264_lacks_only_0x204a_and_0x205c(self):
        for tag_id in (0x204A, 0x205C):
            with self.subTest(tag_id=hex(tag_id)):
                self.assertEqual(prove_entry_absent(LIB_1264, "Sony", "Main", tag_id, PERL)
                                 ["loaded_table_tag_ids"], 111)
        # 12.64 added 0x2032..0x2039: present, so never an absence.
        for tag_id in (0x2032, 0x2039):
            with self.subTest(tag_id=hex(tag_id)), self.assertRaises(NotAbsent):
                prove_entry_absent(LIB_1264, "Sony", "Main", tag_id, PERL)

    def test_ids_the_release_has_refuse(self):
        for tag_id in (0x2031, 0xB041):
            with self.subTest(tag_id=hex(tag_id)), self.assertRaises(NotAbsent):
                prove_entry_absent(LIB_1178, "Sony", "Main", tag_id, PERL)


if __name__ == "__main__":
    unittest.main()
