"""Pinned-native and mutation checks for the staged word-directory compiler."""

import copy
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
import unittest

import native_reader_contract
import word_directory
from test_native_reader_contract import snapshot


PINNED = os.environ.get("OXIDEX_PINNED_EXIFTOOL")


def _deparse(root, fallback=None):
    lib = Path(root) / "lib"
    include = [f"-I{lib}"]
    if fallback:
        include.append(f"-I{Path(fallback) / 'lib'}")
    command = [
        "/usr/bin/perl", *include, "-MB::Deparse", "-MImage::ExifTool::CanonCustom", "-e",
        "print B::Deparse->new('-p')->coderef2text(\\&Image::ExifTool::CanonCustom::ProcessCanonCustom)",
    ]
    return subprocess.run(command, check=True, text=True, capture_output=True).stdout


def processor(root, fallback=None):
    source = Path(root) / "lib" / "Image" / "ExifTool" / "CanonCustom.pm"
    state = snapshot()
    return {
        "__perl": "CODE",
        "__name": "Image::ExifTool::CanonCustom::ProcessCanonCustom",
        "resolved": True,
        "__deparse": _deparse(root, fallback),
        "source_file": "Image/ExifTool/CanonCustom.pm",
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "dependencies": {"Image::ExifTool::Get16u": copy.deepcopy(state["loaded_functions"]["get16u"])},
    }


@unittest.skipUnless(PINNED, "set OXIDEX_PINNED_EXIFTOOL to run pinned-native word-directory checks")
class WordDirectory(unittest.TestCase):
    def setUp(self):
        self.reader = snapshot()
        self.processor = processor(PINNED)

    def compile(self, record=None, reader=None):
        return word_directory.compile_word_directory(
            record or self.processor, {"unsigned16": reader or self.reader}
        )

    def copied_source_processor(self, before, after):
        """Mutate the pinned source, then take a fresh B::Deparse observation."""
        tmp = TemporaryDirectory()
        target = Path(tmp.name) / "lib" / "Image" / "ExifTool"
        target.mkdir(parents=True)
        source = Path(PINNED) / "lib" / "Image" / "ExifTool" / "CanonCustom.pm"
        text = source.read_text()
        marker = "sub ProcessCanonCustom($$$)\n{"
        prefix, process = text.split(marker, 1)
        self.assertIn(before, process)
        target_source = target / "CanonCustom.pm"
        target_source.write_text(prefix + marker + process.replace(before, after, 1))
        self.addCleanup(tmp.cleanup)
        return processor(tmp.name, fallback=PINNED)

    def test_pinned_deparse_compiles_all_source_operands(self):
        compiled = self.compile()
        self.assertEqual(
            (compiled.pair_start, compiled.pair_stride, compiled.key_shift, compiled.value_mask,
             compiled.header_adjustment, compiled.index_divisor, compiled.index_bias),
            (2, 2, 8, 255, 2, 2, 1),
        )
        self.assertEqual((compiled.value_format, compiled.value_count, compiled.value_size), ("int8u", 1, 1))
        self.assertIn("MemberRegex", compiled.model_condition)
        self.assertTrue(compiled.exact_length_first)
        self.assertTrue(compiled.missing_model_as_empty)
        self.assertTrue(compiled.short_u16_as_zero)
        self.assertEqual(compiled.invalid_warning, "Invalid CanonCustom data")
        self.assertEqual(compiled.verbose_directory, "CanonCustom")
        self.assertEqual(compiled.reader_contract_sha256, native_reader_contract.fingerprint(self.reader))
        staged = compiled.rust(lambda value: value.replace('"', '\\"'))
        self.assertIn("key_shift: 8", staged)
        self.assertIn("value_mask: 255", staged)
        self.assertIn("value_format: \"int8u\"", staged)

    def test_source_mutations_change_generated_operands_or_refuse(self):
        baseline = self.compile()
        cases = [
            ("model predicate", "\\bD60\\b", "\\bD61\\b", "changed"),
            ("key shift", "$val >> 8", "$val >> 7", "changed"),
            ("value mask", "$val & 0xff", "$val & 0x7f", "changed"),
            ("value format", "Format => 'int8u'", "Format => 'int16u'", "refused"),
            ("unknown side effect", "    return 1;\n}",
             "    $et->Warn('extra');\n    return 1;\n}", "refused"),
        ]
        for label, before, after, expectation in cases:
            changed = self.copied_source_processor(before, after)
            with self.subTest(label=label):
                if expectation == "changed":
                    fresh = self.compile(changed)
                    self.assertNotEqual(fresh.fingerprint(), baseline.fingerprint())
                    self.assertEqual(word_directory.stale_reason(baseline, changed, {"unsigned16": self.reader}),
                                     "native processor operands or provenance differ")
                else:
                    with self.assertRaises(word_directory.WordDirectoryRefused):
                        self.compile(changed)
                    self.assertIn("no longer compilable", word_directory.stale_reason(
                        baseline, changed, {"unsigned16": self.reader}
                    ))

    def test_provenance_and_reader_binding_are_required(self):
        missing = copy.deepcopy(self.processor)
        del missing["source_sha256"]
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(missing)
        rebinding = copy.deepcopy(self.processor)
        rebinding["dependencies"]["Image::ExifTool::Get16u"]["__name"] = "Image::ExifTool::Get32u"
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(rebinding)
        changed_reader = snapshot()
        for where in (changed_reader["loaded_functions"], changed_reader["isolated_functions"]):
            where["get16u"]["__deparse"] = where["get16u"]["__deparse"].replace("'S'", "'L'")
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(reader=changed_reader)

    def test_stale_source_provenance_rejects_an_unchanged_deparse(self):
        baseline = self.compile()
        changed = copy.deepcopy(self.processor)
        changed["source_sha256"] = "3" * 64
        fresh = self.compile(changed)
        self.assertNotEqual(fresh.fingerprint(), baseline.fingerprint())
        self.assertEqual(word_directory.stale_reason(
            baseline, changed, {"unsigned16": self.reader}
        ), "native processor operands or provenance differ")


if __name__ == "__main__":
    unittest.main()
