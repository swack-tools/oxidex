"""Final-loaded EXIF format-registry facts for the native writer boundary."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]
DUMP = ROOT / "tools/exiftool-tables/dump_tables.pl"
from checkexif_native_differential import resolve_perl, resolve_library

PERL = resolve_perl(Path(os.environ.get("EXIFTOOL_PERL", "/usr/bin/perl")))


def clean_env():
    return {key: value for key, value in os.environ.items()
            if not (key.startswith("PERL5") or key == "PERLLIB")}



class FormatRegistryFixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = Path(self.tmp.name) / "lib"
        package = self.lib / "Image/ExifTool"
        package.mkdir(parents=True)
        (self.lib / "Image/ExifTool.pm").write_text(textwrap.dedent("""\
            package Image::ExifTool;
            our $VERSION = 'fixture';
            our %specialTags = map { $_ => 1 } qw(GROUPS FORMAT FIRST_ENTRY);
            1;
        """), encoding="utf-8")
        self.exif = package / "Exif.pm"
        self.write_exif(valid=True)

    def write_exif(self, *, valid: bool):
        numbers = "int8u => 1, string => 2, binary => 2, utf8 => 129"
        if not valid:
            numbers = "int8u => 1, string => 2, utf8 => 129, broken => 130"
        self.exif.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::Exif;
            our @formatName = (undef, 'int8u', 'string');
            $formatName[129] = 'utf8';
            our @formatSize = (undef, 1, 1);
            $formatSize[129] = 1;
            our %formatNumber = ({numbers});
            our %Main = ( GROUPS => {{ 0 => 'EXIF', 1 => 'IFD0' }},
                         0x13c => {{ Name => 'HostComputer', Writable => 'string' }} );
            1;
        """), encoding="utf-8")

    def dump(self):
        result = subprocess.run([str(PERL), str(DUMP), str(self.lib), "Exif"],
                                check=True, text=True, capture_output=True, env=clean_env(), timeout=60)
        return json.loads(result.stdout)["native_write_format_registry"]

    def test_preserves_sparse_final_loaded_arrays_aliases_and_source_provenance(self):
        registry = self.dump()
        self.assertEqual(registry["state"], "resolved")
        self.assertEqual(registry["source"]["library_relative_path"], "Image/ExifTool/Exif.pm")
        self.assertEqual(registry["source"]["sha256"], hashlib.sha256(self.exif.read_bytes()).hexdigest())
        self.assertIsNone(registry["format_name"][0])
        self.assertIsNone(registry["format_name"][3])
        self.assertEqual(registry["format_name"][129], "utf8")
        self.assertEqual(registry["format_size"][129], 1)
        self.assertEqual(registry["format_number"]["utf8"], 129)
        self.assertEqual(registry["format_number"]["binary"], 2)

    def test_unloaded_registry_is_reported_without_loading_exif(self):
        self.exif.write_text("die 'unexpected Exif load';", encoding="utf-8")
        probe = self.lib / "Image/ExifTool/Probe.pm"
        probe.write_text("package Image::ExifTool::Probe; our %Main = (1 => {Name => 'Sample'}); 1;", encoding="utf-8")
        result = subprocess.run([str(PERL), str(DUMP), str(self.lib), "Probe"],
                                check=True, text=True, capture_output=True, env=clean_env(), timeout=60)
        document = json.loads(result.stdout)
        self.assertEqual(document["native_write_format_registry"]["state"], "refused")
        self.assertEqual(document["native_write_format_registry"]["reason"], "format_registry_not_loaded")
        self.assertNotIn("unexpected Exif load", result.stderr)

    def test_inconsistent_loaded_registry_is_visibly_refused(self):
        self.write_exif(valid=False)
        registry = self.dump()
        self.assertEqual(registry["state"], "refused")
        self.assertEqual(registry["reason"], "format_number_target_name_missing")
        self.assertEqual(registry["source"]["library_relative_path"], "Image/ExifTool/Exif.pm")


@unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                     "requires explicit Perl and selected native ExifTool library")
class ActualNativeSourceMutation(unittest.TestCase):
    def dump(self, library):
        result = subprocess.run([str(PERL), str(DUMP), str(library), "Exif"],
                                check=True, text=True, capture_output=True, env=clean_env(), timeout=60)
        return json.loads(result.stdout)["native_write_format_registry"]

    def test_copied_native_registry_name_number_and_size_mutation_is_captured(self):
        library = resolve_library(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]))
        before = self.dump(library)
        self.assertEqual(before["state"], "resolved")
        slot = next(i for i, name in enumerate(before["format_name"]) if i > 0 and name)
        original_name = before["format_name"][slot]
        new_name = "oxidex_registry_mutated"
        self.assertNotIn(new_name, before["format_number"])
        alias = next(name for name, number in before["format_number"].items()
                     if name != before["format_name"][number] and number != slot)
        new_size = before["format_size"][slot] + 1
        def perl_literal(value):
            return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"
        mutation = ("\npackage Image::ExifTool::Exif;\n"
                    f"delete $formatNumber{{{perl_literal(original_name)}}};\n"
                    f"$formatName[{slot}] = {perl_literal(new_name)};\n"
                    f"$formatSize[{slot}] = {new_size};\n"
                    f"$formatNumber{{{perl_literal(new_name)}}} = {slot};\n"
                    f"$formatNumber{{{perl_literal(alias)}}} = {slot};\n1;\n")
        with tempfile.TemporaryDirectory() as temporary:
            copied = Path(temporary) / "lib"
            shutil.copytree(library, copied)
            source = copied / "Image/ExifTool/Exif.pm"
            text = source.read_text(encoding="utf-8")
            self.assertEqual(text.count("\n__END__"), 1)
            source.write_text(text.replace("\n__END__", mutation + "\n__END__", 1), encoding="utf-8")
            after = self.dump(copied)
        self.assertEqual(after["state"], "resolved")
        self.assertEqual(after["format_name"][slot], new_name)
        self.assertEqual(after["format_size"][slot], new_size)
        self.assertEqual(after["format_number"][new_name], slot)
        self.assertNotIn(original_name, after["format_number"])
        self.assertEqual(after["format_number"][alias], slot)
        self.assertNotEqual(before["source"]["sha256"], after["source"]["sha256"])



if __name__ == "__main__":
    unittest.main()
