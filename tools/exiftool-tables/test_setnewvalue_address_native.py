"""Optional canonical-native proof for ordinary EXIF addressing source gates."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from checkexif_recipes import RecipeRefused
from setnewvalue_addressing import _find_tag_info_source
from setnewvalue_convinv_recipes import compile_setnewvalue_convinv


ROOT = Path(__file__).resolve().parents[2]
PERL = os.environ.get("EXIFTOOL_PERL")
LIB = os.environ.get("OXIDEX_PINNED_EXIFTOOL")


@unittest.skipUnless(PERL and LIB and Path(PERL).is_file() and Path(LIB).is_dir(),
                     "requires explicit canonical EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
class NativeSetNewValueAddressingTests(unittest.TestCase):
    def perl_body(self, lib: Path, symbol: str, prepend: Path | None = None) -> str:
        inc = ([f"-I{prepend}"] if prepend else []) + [f"-I{lib}"]
        result = subprocess.run(
            [PERL, *inc, "-MB::Deparse", "-MImage::ExifTool", "-e",
             f"require q(Image/ExifTool/Writer.pl); print B::Deparse->new('-p','-sC')->coderef2text(\\&{symbol})"],
            check=True, capture_output=True, text=True)
        return result.stdout

    def code_fact(self, name: str, body: str, source_file: str, source_bytes: bytes, *, requested: str | None = None):
        value = {"__perl": "CODE", "resolved": True, "__name": name, "__deparse": body,
                 "source_file": source_file, "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
                 "dependencies": {}}
        if requested:
            value["requested_binding"] = requested
        return value

    def test_native_case_and_exif_ifd0_selection(self):
        tags = ["HostComputer", "hOsTcOmPuTeR", "EXIF:HostComputer", "IFD0:HostComputer",
                "Software", "IFD0:Software"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tags.json"
            path.write_text(json.dumps(tags))
            result = subprocess.run(
                [PERL, str(ROOT / "tools/exiftool-tables/probe_setnewvalue_addressing_native.pl"), LIB, str(path)],
                check=True, capture_output=True, text=True)
        observed = {item["tag"]: item for item in json.loads(result.stdout)}
        for tag in tags[:4]:
            with self.subTest(tag=tag):
                self.assertEqual(observed[tag]["count"], 1)
                self.assertEqual(observed[tag]["rows"], [{"name": "HostComputer", "raw_id": "316",
                                                           "table_name": "Image::ExifTool::Exif::Main",
                                                           "write_group": "IFD0"}])
        self.assertEqual(observed["Software"]["count"], 8)
        self.assertEqual(observed["IFD0:Software"]["count"], 1)

    def test_copied_lookup_case_and_writer_group_source_changes_refuse(self):
        lib = Path(LIB)
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / "Image/ExifTool"
            copied.mkdir(parents=True)
            lookup_source = (lib / "Image/ExifTool/TagLookup.pm").read_bytes()
            changed_lookup = lookup_source.replace(b"my $lcTag = lc($tag);", b"my $lcTag = $tag;", 1)
            self.assertNotEqual(changed_lookup, lookup_source)
            (copied / "TagLookup.pm").write_bytes(changed_lookup)
            body = self.perl_body(lib, "Image::ExifTool::TagLookup::FindTagInfo", Path(directory))
            fact = self.code_fact("Image::ExifTool::TagLookup::FindTagInfo", body,
                                  "Image/ExifTool/TagLookup.pm", changed_lookup,
                                  requested="Image::ExifTool::TagLookup::FindTagInfo")
            with self.assertRaisesRegex(RecipeRefused, "FindTagInfo"):
                _find_tag_info_source({"native_write_helpers": {"find_tag_info": fact}})

            writer_source = (lib / "Image/ExifTool/Writer.pl").read_bytes()
            changed_writer = writer_source.replace(
                b"($options{Group}, $tag) = ($1, $2) if $tag =~ /(.*):(.+)/;",
                b"($options{Group}, $tag) = ($2, $1) if $tag =~ /(.*):(.+)/;", 1)
            self.assertNotEqual(changed_writer, writer_source)
            (copied / "Writer.pl").write_bytes(changed_writer)
            setnew_body = self.perl_body(lib, "Image::ExifTool::SetNewValue", Path(directory))
            conv_body = self.perl_body(lib, "Image::ExifTool::ConvInv", Path(directory))
            conv = self.code_fact("Image::ExifTool::ConvInv", conv_body,
                                  "Image/ExifTool/Writer.pl", changed_writer)
            setnew = self.code_fact("Image::ExifTool::SetNewValue", setnew_body,
                                    "Image/ExifTool/Writer.pl", changed_writer,
                                    requested="Image::ExifTool::SetNewValue")
            setnew["dependencies"] = {"Image::ExifTool::ConvInv": conv}
            with self.assertRaisesRegex(RecipeRefused, "caller control flow"):
                compile_setnewvalue_convinv(setnew)


if __name__ == "__main__":
    unittest.main()
