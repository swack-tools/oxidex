#!/usr/bin/env python3
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from unittest import mock
from pathlib import Path

MATRIX = Path(__file__).with_name("native_write_matrix.py")
spec = importlib.util.spec_from_file_location("native_write_matrix", MATRIX)
module = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(module)
ROOT = Path(__file__).resolve().parents[2]


class NativeWriteMatrixTests(unittest.TestCase):
    def test_native_resolution_rationals_and_numeric_storage_are_preserved(self):
        import struct
        # One classic TIFF directory with inline and offset-stored values.
        # Include the RATIONAL type ExifTool adds to new JPEG EXIF blocks.
        for order, endian in (("little", "<"), ("big", ">")):
            with self.subTest(order=order):
                records = [
                    (282, 5, struct.pack(endian + "II", 72, 1)),
                    (65000, 6, bytes([255])),
                    (65001, 7, b"a\0b"),
                    (65002, 8, struct.pack(endian + "h", -42)),
                    (65003, 9, struct.pack(endian + "i", -100000)),
                    (65004, 10, struct.pack(endian + "ii", -1, 3)),
                    (65005, 11, struct.pack(endian + "f", 1.5)),
                    (65006, 12, struct.pack(endian + "d", -2.25)),
                ]
                header = (b"II" if order == "little" else b"MM") + struct.pack(endian + "HI", 42, 8)
                tail_offset = 8 + 2 + len(records) * 12 + 4
                entries, tail = bytearray(), bytearray()
                for tag, kind, value in records:
                    count = len(value) if kind == 7 else 1
                    slot = (value.ljust(4, b"\0") if len(value) <= 4 else
                            struct.pack(endian + "I", tail_offset + len(tail)))
                    entries += struct.pack(endian + "HHI", tag, kind, count) + slot
                    if len(value) > 4:
                        tail += value
                data = header + struct.pack(endian + "H", len(records)) + entries + bytes(4) + tail
                result = module.parse_tiff(data, require_strip=False)
                for tag, kind, value in records:
                    self.assertEqual(result["tags"][str(tag)]["value_hex"], value.hex())
                    self.assertEqual(result["tags"][str(tag)]["type"], kind)
                with self.assertRaisesRegex(ValueError, "out of bounds"):
                    module.parse_tiff(data[:-1], require_strip=False)
                unknown = bytearray(data)
                unknown[12:14] = struct.pack(endian + "H", 15)
                with self.assertRaisesRegex(ValueError, "unsupported TIFF type 15"):
                    module.parse_tiff(unknown, require_strip=False)

    def test_repository_upgrade_refuses_stale_baseline_even_with_old_library(self):
        with tempfile.TemporaryDirectory() as temporary:
            pin = Path(temporary) / ".exiftool-version"
            pin.write_text("13.60\n")
            with self.assertRaisesRegex(RuntimeError, "baseline is stale"):
                module.assert_contract_version({"result": {"exiftool_version": "13.59"}}, pin)

    def test_dirty_checkout_refuses_before_native_calls_or_output_creation(self):
        state = module.git_state(ROOT)
        state.dirty = True
        state.dirty_files = ["native_write_matrix.py"]
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(module, "git_state", return_value=state), \
             mock.patch.object(module, "native_identity") as native, mock.patch.dict(os.environ, {"OXIDEX_ALLOW_DIRTY_TREE": "0"}):
            output = Path(temporary) / "result.json"
            with self.assertRaisesRegex(SystemExit, "refusing to measure"):
                module.main(["--perl", "perl", "--lib", temporary, "--jpeg-base", temporary, "--output", str(output)])
            native.assert_not_called()
            self.assertFalse(output.exists())
            self.assertFalse((output.parent / "native-write-matrix-files").exists())

    def test_partial_library_cannot_fall_back_to_ambient_main_module(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(module.subprocess, "run") as run:
            library = Path(temporary)
            writer = library / "Image/ExifTool/Writer.pl"
            writer.parent.mkdir(parents=True)
            writer.write_text("1;\n")
            with self.assertRaisesRegex(ValueError, "Image/ExifTool.pm absent"):
                module.native_identity(Path("perl"), library)
            run.assert_not_called()

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL"), "requires explicit Perl")
    def test_native_inc_guard_rejects_external_writer_despite_local_files(self):
        # The selected tree exists, but its main module arranges a mixed %INC.
        # This exercises the native guard, beyond the Python existence check.
        with tempfile.TemporaryDirectory() as temporary:
            library = Path(temporary) / "lib"
            writer = library / "Image/ExifTool/Writer.pl"
            writer.parent.mkdir(parents=True)
            writer.write_text("1;\n")
            external = Path(temporary) / "outside.pl"
            external.write_text("1;\n")
            # Resolve the actual outside path without interpolating it into Perl.
            (library / "Image/ExifTool.pm").write_text(
                "package Image::ExifTool; our $VERSION='13.59'; use File::Basename ();\n"
                "$INC{'Image/ExifTool/Writer.pl'} = File::Basename::dirname(__FILE__) . '/../../outside.pl'; 1;\n")
            with self.assertRaisesRegex(RuntimeError, "outside selected library"):
                module.native_identity(module.resolve_perl(Path(os.environ["EXIFTOOL_PERL"])), library)

    def test_tiff_parser_rejects_truncated_ifd_and_value(self):
        with self.assertRaises(ValueError):
            module.parse_tiff(b"II\x2a\x00\x08\x00\x00\x00\x01")
        # A complete one-entry IFD whose five-byte string points outside the
        # file. Keep the count/header valid so this reaches value bounds.
        pack = module.pack
        bad = (b"II" + pack(42, 2, "little") + pack(8, 4, "little")
               + pack(1, 2, "little") + pack(316, 2, "little")
               + pack(2, 2, "little") + pack(5, 4, "little")
               + pack(0x7fffffff, 4, "little") + pack(0, 4, "little"))
        with self.assertRaisesRegex(ValueError, "value.*bounds|value.*outside"):
            module.parse_tiff(bad)


    def test_little_and_big_tiff_carriers_are_parseable(self):
        with tempfile.TemporaryDirectory() as temporary:
            for order in ("little", "big"):
                path = Path(temporary) / f"{order}.tif"
                module.make_tiff(path, order)
                inspected = module.parse_tiff(path.read_bytes())
                self.assertEqual(inspected["byte_order"], order)
                self.assertEqual(inspected["image_payload_hex"], "ff")

    def test_exact_native_host_contract_rejects_each_mutated_dimension(self):
        expected = module.expected_host("embedded_nul")
        self.assertEqual(expected, {"type": 2, "count": 4, "value_hex": "61006200"})
        module.assert_host_contract(dict(expected), expected)
        for dimension, wrong in (("type", 1), ("count", 3), ("value_hex", "61006300")):
            actual = dict(expected)
            actual[dimension] = wrong
            with self.subTest(dimension=dimension):
                with self.assertRaisesRegex(AssertionError, dimension):
                    module.assert_host_contract(actual, expected)

    def test_contract_version_rejects_a_different_native_release(self):
        wrong = {"result": {"exiftool_version": "13.60"}}
        with self.assertRaisesRegex(RuntimeError, "13.59 native-write acceptance baseline"):
            module.assert_contract_version(wrong)

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                         "requires explicit pinned native Perl and ExifTool library")
    def test_all_declared_native_rows_execute_and_preserve_carriers(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "matrix.json"
            subprocess.run(["python3", str(MATRIX), "--perl", os.environ["EXIFTOOL_PERL"], "--lib", os.environ["OXIDEX_PINNED_EXIFTOOL"],
                            "--jpeg-base", str(ROOT / "tests/fixtures/jpeg/tag_matrix_base.jpg"), "--output", str(output)],
                           env=os.environ | {"OXIDEX_ALLOW_DIRTY_TREE": "1"}, check=True, timeout=120)
            document = json.loads(output.read_text())
        self.assertEqual(document["declared_rows"], 48)
        self.assertEqual(document["source"]["source_commit"], module.git_state(ROOT).commit)
        self.assertEqual(document["source"]["dirty_override"], document["source"]["dirty"])
        self.assertEqual(document["source"]["repository_exiftool_pin"], (ROOT / ".exiftool-version").read_text().strip())
        self.assertEqual(document["source"]["instrument_sha256"], module.hashlib.sha256(MATRIX.read_bytes()).hexdigest())
        self.assertIn("Image/ExifTool.pm", document["native_identity"]["result"]["loaded_modules"])
        self.assertEqual(document["executed_rows"], 48)
        self.assertEqual(document["contract_exiftool_release"], "13.59")
        self.assertEqual(document["native_identity"]["result"]["exiftool_version"], "13.59")
        self.assertEqual(len(document["rows"]), 48)
        self.assertEqual({row["carrier"] for row in document["rows"]}, {"tiff_little", "tiff_big", "jpeg"})
        self.assertEqual({row["requested_name"] for row in document["rows"]}, set(module.NAMES))
        self.assertEqual({row["operation"] for row in document["rows"]}, set(module.CASES))
        for row in document["rows"]:
            self.assertEqual(row["operation_call"]["returncode"], 0)
            self.assertTrue(row["verification"]["carrier_image_preserved"])
            self.assertTrue(row["verification"]["artist_preserved"])
            self.assertFalse(row["verification"]["no_op_detected"])
            after = row["verification"]["host_after"]
            self.assertEqual(module.host_storage(after), row["verification"]["expected_host"])
            if after is not None and after["count"] <= 4:
                self.assertTrue(after["inline"])


if __name__ == "__main__":
    unittest.main()
