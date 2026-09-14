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
from fresh_jpeg_byte_order_codegen import FreshByteOrderRefused, _source_profile, compile_recipe, generate

PERL = Path(os.environ.get("EXIFTOOL_PERL", "/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2"))
LIB = Path(os.environ.get("OXIDEX_PINNED_EXIFTOOL", "/tmp/oxidex-exiftool-cache/exiftool/lib"))
if (LIB / "lib").is_dir():
    LIB = LIB / "lib"
NATIVE_READY = PERL.is_file() and LIB.is_dir()
ROOT = Path(__file__).resolve().parents[2]
DUMP = ROOT / "tools/exiftool-tables/dump_tables.pl"

from fresh_jpeg_byte_order_native import capture



def native_document(library: Path = LIB) -> dict[str, object]:
    try:
        return capture(PERL, library)
    except RuntimeError as error:
        raise AssertionError(str(error)) from error


def native_writer_document(library: Path = LIB) -> dict[str, object]:
    result = subprocess.run([str(PERL), str(DUMP), str(library), "Exif"],
                            text=True, capture_output=True, check=False, timeout=30)
    if result.returncode:
        raise AssertionError(f"bounded writer capture failed: {result.stderr}")
    return json.loads(result.stdout)



class HistoricalFreshJpegProfileTests(unittest.TestCase):
    """Portable source facts captured from the persisted selected releases."""

    def profile(self, release: str):
        document = json.loads((Path(__file__).parent / "testdata" / f"fresh_jpeg_byte_order_{release.replace('.', '_')}_fact.json").read_text())
        return _source_profile(document["set_preferred_byte_order"]["__deparse"],
                               document["set_byte_order"]["__deparse"],
                               document["get_byte_order"]["__deparse"],
                               document["new_jpeg_caller"]["__deparse"]), document

    def test_selected_release_profiles_extract_their_own_ifd0_operand(self):
        expected = {
            "11.78": ("no-default-argument-ifd0", "NoDefaultArgument"),
            "12.64": ("default-argument-ifd0-12-64", "Ifd0DefaultArgumentUndef"),
        }
        for release, (name, mode) in expected.items():
            with self.subTest(release=release):
                (profile, fallback), document = self.profile(release)
                self.assertEqual((profile.name, profile.caller_mode, fallback), (name, mode, "MM"))
                self.assertEqual(document["observations"]["fresh_ifd0_no_overrides"], {"selected": "MM", "reported": "MM"})

    def test_historical_method_fallback_mutation_reaches_selected_operand(self):
        (profile, _), document = self.profile("11.78")
        method = document["set_preferred_byte_order"]["__deparse"]
        self.assertEqual(method.count("'MM'"), 2)
        changed = method.replace("'MM'", "'II'")
        selected, fallback = _source_profile(changed, document["set_byte_order"]["__deparse"],
                                             document["get_byte_order"]["__deparse"],
                                             document["new_jpeg_caller"]["__deparse"])
        self.assertEqual((selected.name, fallback), (profile.name, "II"))

    def test_historical_caller_mutation_refuses_instead_of_reusing_current_mode(self):
        _, document = self.profile("12.64")
        caller = document["new_jpeg_caller"]["__deparse"]
        changed = caller.replace("SetPreferredByteOrder($defaultByteOrder)", "SetPreferredByteOrder()", 1)
        with self.assertRaisesRegex(FreshByteOrderRefused, "DoProcessTIFF complete caller body"):
            _source_profile(document["set_preferred_byte_order"]["__deparse"],
                            document["set_byte_order"]["__deparse"],
                            document["get_byte_order"]["__deparse"], changed)


@unittest.skipUnless(NATIVE_READY, "requires canonical EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
class FreshJpegByteOrderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.document = native_document()
        cls.writer_document = native_writer_document()

    def test_native_default_is_compiled_from_executable_not_hardcoded(self) -> None:
        recipe = compile_recipe(self.document, self.writer_document)
        self.assertEqual(recipe.order, self.document["observations"]["fresh_ifd0_no_overrides"]["selected"])
        self.assertEqual(recipe.order, "MM")
        self.assertEqual(recipe.caller_mode, "Ifd0DefaultArgumentUndef")
        source, report = generate(self.document, self.writer_document)
        self.assertEqual(report["state"], "resolved")
        self.assertIn("selected: FreshJpegExifByteOrder::BigEndian", source)
        self.assertIn(recipe.set_preferred_source_sha256, source)
        self.assertIn(recipe.set_byte_order_body_sha256, source)
        self.assertIn(recipe.get_byte_order_body_sha256, source)

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
            changed_writer = native_writer_document(copied)
            self.assertEqual(changed["set_preferred_byte_order"]["source_file"], "Image/ExifTool/Writer.pl")
            self.assertNotEqual(changed["set_preferred_byte_order"]["source_sha256"], self.document["set_preferred_byte_order"]["source_sha256"])
            self.assertEqual(changed["observations"]["fresh_ifd0_no_overrides"]["selected"], "II")
            rendered, report = generate(changed, changed_writer)
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
                compile_recipe(native_document(copied), native_writer_document(copied))

    def test_copied_caller_block_change_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            copied = Path(temp) / "lib"
            shutil.copytree(LIB, copied)
            core = copied / "Image/ExifTool.pm"
            source = core.read_text(encoding="utf-8")
            needle = "my $defaultByteOrder;\n        if ($$dirInfo{DirName}"
            self.assertIn(needle, source)
            core.write_text(source.replace(needle, "my $defaultByteOrder;\n        my $injected = 1;\n        if ($$dirInfo{DirName}", 1), encoding="utf-8")
            with self.assertRaisesRegex(FreshByteOrderRefused, "DoProcessTIFF complete caller body"):
                compile_recipe(native_document(copied), native_writer_document(copied))

    def test_copied_post_block_exif_write_changes_native_outcome_and_refuses(self) -> None:
        # This write is deliberately *after* the matched new-header block.
        # The previous substring-only admission accepted it even though native
        # fresh JPEG output becomes Intel ordered.
        from native_write_matrix import run_native
        source_fixture = Path(__file__).resolve().parents[2] / "tests/fixtures/jpeg/edge_cases/orientation_2.jpg"
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            copied = temp / "lib"
            shutil.copytree(LIB, copied)
            core = copied / "Image/ExifTool.pm"
            source = core.read_text(encoding="utf-8")
            boundary = "    }\n    $$self{EXIF_POS} = $base + $$self{BASE};"
            self.assertEqual(source.count(boundary), 1)
            core.write_text(source.replace(
                boundary,
                "    }\n    $$self{EXIF_DATA} = \"II\\0\\x2a\\0\\x08\\0\\0\\0\";\n    $$self{EXIF_POS} = $base + $$self{BASE};",
                1), encoding="utf-8")
            target = temp / "output.jpg"
            native = run_native(PERL, copied, source_fixture, target, "insert", "IFD0:HostComputer")
            self.assertEqual(native["returncode"], 0, native["stderr"])
            # The replacement is post-branch and produces a malformed IFD0 in
            # the actual native writer; it must be rejected before a generated
            # rule can ignore that changed control path.
            self.assertEqual(native["result"]["write_return"], 0, native["result"])
            self.assertIn("Bad IFD0 directory", native["result"]["error"])
            self.assertFalse(target.exists())
            with self.assertRaisesRegex(FreshByteOrderRefused, "complete caller body"):
                compile_recipe(native_document(copied), native_writer_document(copied))

    def test_copied_reachable_get_byte_order_change_refuses(self) -> None:
        # SetPreferredByteOrder returns GetByteOrder, so changing that helper
        # changes the fresh-header branch despite leaving its own body intact.
        from native_write_matrix import run_native
        source_fixture = Path(__file__).resolve().parents[2] / "tests/fixtures/jpeg/edge_cases/orientation_2.jpg"
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            copied = temp / "lib"
            shutil.copytree(LIB, copied)
            core = copied / "Image/ExifTool.pm"
            source = core.read_text(encoding="utf-8")
            old = "sub GetByteOrder() { return $currentByteOrder; }"
            self.assertEqual(source.count(old), 1)
            core.write_text(source.replace(old, "sub GetByteOrder() { return 'II'; }", 1), encoding="utf-8")
            target = temp / "output.jpg"
            native = run_native(PERL, copied, source_fixture, target, "insert", "IFD0:HostComputer")
            self.assertEqual(native["returncode"], 0, native["stderr"])
            offset = target.read_bytes().index(b"Exif\0\0") + len(b"Exif\0\0")
            self.assertEqual(target.read_bytes()[offset:offset + 4], b"II*\0")
            with self.assertRaisesRegex(FreshByteOrderRefused, "GetByteOrder body"):
                compile_recipe(native_document(copied), native_writer_document(copied))

    def test_full_writer_or_reader_capture_mismatch_refuses(self) -> None:
        changed = copy.deepcopy(self.writer_document)
        changed["native_write_capture_context"]["loaded_modules"]["Image/ExifTool/Writer.pl"] = "0" * 64
        with self.assertRaisesRegex(FreshByteOrderRefused, "full writer capture"):
            compile_recipe(self.document, changed)
        changed = copy.deepcopy(self.writer_document)
        modules = changed["native_capture_context"]["loaded_closure"]["modules"]
        for item in modules:
            if item["inc"] == "Image/ExifTool.pm":
                item["source_sha256"] = "0" * 64
                break
        with self.assertRaisesRegex(FreshByteOrderRefused, "reader capture"):
            compile_recipe(self.document, changed)

    def test_tampered_closure_or_observation_refuses(self) -> None:
        changed = copy.deepcopy(self.document)
        changed["capture_context"]["loaded_modules"]["Image/ExifTool.pm"] = "0" * 64
        with self.assertRaisesRegex(FreshByteOrderRefused, "closure"):
            compile_recipe(changed, self.writer_document)
        changed = copy.deepcopy(self.document)
        changed["set_byte_order"]["source_sha256"] = "0" * 64
        with self.assertRaisesRegex(FreshByteOrderRefused, "helper source does not join"):
            compile_recipe(changed, self.writer_document)
        changed = copy.deepcopy(self.document)
        changed["observations"]["fresh_ifd0_no_overrides"]["selected"] = "II"
        with self.assertRaisesRegex(FreshByteOrderRefused, "does not match"):
            compile_recipe(changed, self.writer_document)

    def test_rendered_rule_refuses_override_at_runtime(self) -> None:
        source, _ = generate(self.document, self.writer_document)
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
