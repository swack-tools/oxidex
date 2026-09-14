"""Focused native evidence for fresh-JPEG preferred byte order."""
from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
from fresh_jpeg_byte_order_codegen import FreshByteOrderRefused, compile_recipe, generate

PERL = Path(os.environ.get("EXIFTOOL_PERL", "/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2"))
LIB = Path(os.environ.get("OXIDEX_PINNED_EXIFTOOL", "/tmp/oxidex-exiftool-cache/exiftool/lib"))
if (LIB / "lib").is_dir():
    LIB = LIB / "lib"
NATIVE_READY = PERL.is_file() and LIB.is_dir()

from fresh_jpeg_byte_order_native import capture



def native_document(library: Path = LIB) -> dict[str, object]:
    try:
        return capture(PERL, library)
    except RuntimeError as error:
        raise AssertionError(str(error)) from error



@unittest.skipUnless(NATIVE_READY, "requires canonical EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
class FreshJpegByteOrderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.document = native_document()

    def test_native_default_is_compiled_from_executable_not_hardcoded(self) -> None:
        recipe = compile_recipe(self.document)
        self.assertEqual(recipe.order, self.document["observations"]["fresh_ifd0_no_overrides"]["selected"])
        self.assertEqual(recipe.order, "MM")
        source, report = generate(self.document)
        self.assertEqual(report["state"], "resolved")
        self.assertIn("selected: FreshJpegExifByteOrder::BigEndian", source)
        self.assertIn(recipe.set_preferred_source_sha256, source)

    def test_native_fresh_jpeg_header_uses_observed_default(self) -> None:
        from native_write_matrix import run_native
        source_fixture = Path(__file__).resolve().parents[2] / "tests/fixtures/jpeg/edge_cases/orientation_2.jpg"
        self.assertNotIn(b"Exif\0\0", source_fixture.read_bytes())
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source, target = temp / "input.jpg", temp / "output.jpg"
            shutil.copyfile(source_fixture, source)
            native = run_native(PERL, LIB, source, target, "insert", "IFD0:HostComputer")
            self.assertEqual(native["returncode"], 0, native["stderr"])
            self.assertEqual(native["result"]["write_return"], 1, native["result"])
            output = target.read_bytes()
            offset = output.index(b"Exif\0\0") + len(b"Exif\0\0")
            self.assertEqual(output[offset:offset + 2], b"MM")
            self.assertEqual(output[offset + 2:offset + 4], b"\0*")

    def test_override_inputs_are_explicitly_not_admitted(self) -> None:
        for name in ("byte_order_option", "exif_byte_order", "maker_note_byte_order"):
            with self.subTest(name=name):
                observed = self.document["observations"][name]
                self.assertEqual(observed["selected"], "II")
                self.assertEqual(observed["reported"], "II")

    def test_copied_writer_default_change_propagates_from_loaded_executable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            copied = Path(temp) / "lib"
            shutil.copytree(LIB, copied)
            writer = copied / "Image/ExifTool/Writer.pl"
            source = writer.read_text(encoding="utf-8")
            self.assertEqual(source.count("|| 'MM'"), 2)
            writer.write_text(source.replace("|| 'MM'", "|| 'II'"), encoding="utf-8")
            changed = native_document(copied)
            self.assertEqual(changed["set_preferred_byte_order"]["source_file"], "Image/ExifTool/Writer.pl")
            self.assertNotEqual(changed["set_preferred_byte_order"]["source_sha256"], self.document["set_preferred_byte_order"]["source_sha256"])
            self.assertEqual(changed["observations"]["fresh_ifd0_no_overrides"]["selected"], "II")
            rendered, report = generate(changed)
            self.assertEqual(report["order"], "II")
            self.assertIn("selected: FreshJpegExifByteOrder::LittleEndian", rendered)

    def test_copied_executable_shape_change_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            copied = Path(temp) / "lib"
            shutil.copytree(LIB, copied)
            writer = copied / "Image/ExifTool/Writer.pl"
            source = writer.read_text(encoding="utf-8")
            self.assertIn("my ($self, $default) = @_;", source)
            writer.write_text(source.replace("my ($self, $default) = @_;", "my ($self, $default) = @_; my $injected = 1;", 1), encoding="utf-8")
            with self.assertRaisesRegex(FreshByteOrderRefused, "admitted grammar"):
                compile_recipe(native_document(copied))

    def test_copied_caller_block_change_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            copied = Path(temp) / "lib"
            shutil.copytree(LIB, copied)
            core = copied / "Image/ExifTool.pm"
            source = core.read_text(encoding="utf-8")
            needle = "my $defaultByteOrder;\n        if ($$dirInfo{DirName}"
            self.assertIn(needle, source)
            core.write_text(source.replace(needle, "my $defaultByteOrder;\n        my $injected = 1;\n        if ($$dirInfo{DirName}", 1), encoding="utf-8")
            with self.assertRaisesRegex(FreshByteOrderRefused, "new-header caller block"):
                compile_recipe(native_document(copied))

    def test_tampered_closure_or_observation_refuses(self) -> None:
        changed = copy.deepcopy(self.document)
        changed["capture_context"]["loaded_modules"]["Image/ExifTool.pm"] = "0" * 64
        with self.assertRaisesRegex(FreshByteOrderRefused, "closure"):
            compile_recipe(changed)
        changed = copy.deepcopy(self.document)
        changed["observations"]["fresh_ifd0_no_overrides"]["selected"] = "II"
        with self.assertRaisesRegex(FreshByteOrderRefused, "does not match"):
            compile_recipe(changed)

    def test_rendered_rule_refuses_override_at_runtime(self) -> None:
        source, _ = generate(self.document)
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            rule = temp / "rule.rs"
            rule.write_text(source, encoding="utf-8")
            driver = temp / "driver.rs"
            driver.write_text(f'''#[path = "{rule}"] mod rule;
fn main() {{
 assert_eq!(rule::fresh_jpeg_byte_order(&rule::FRESH_JPEG_BYTE_ORDER, rule::FreshJpegByteOrderInputs {{ byte_order_option: None, exif_byte_order: None, maker_note_byte_order: None }}).unwrap(), rule::FreshJpegExifByteOrder::BigEndian);
 assert!(rule::fresh_jpeg_byte_order(&rule::FRESH_JPEG_BYTE_ORDER, rule::FreshJpegByteOrderInputs {{ byte_order_option: Some("II"), exif_byte_order: None, maker_note_byte_order: None }}).is_err());
}}''', encoding="utf-8")
            binary = temp / "driver"
            subprocess.run(["rustc", "--edition=2021", str(driver), "-o", str(binary)], check=True, text=True, capture_output=True)
            subprocess.run([str(binary)], check=True, text=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
