"""Native source replay for the four currently omitted CanonRaw opaque scalars."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

PINNED = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
PERL = os.environ.get("EXIFTOOL_PERL")
CANONICAL_PERL = "v5.38.2"
PROBE = Path(__file__).with_name("probe_ciff_opaque.pl")


def request(order, entries, *, image_data_hash=False):
    return {
        "protocol": "oxidex.ciff_opaque.v1",
        "byte_order": order,
        "entries": entries,
        "image_data_hash": image_data_hash,
    }


def replay(requests, *, root=PINNED, fallback=None):
    command = [PERL, str(PROBE), "--lib", "lib"]
    if fallback is not None:
        command.extend(["--fallback-lib", fallback])
    payload = "".join(json.dumps(item, separators=(",", ":")) + "\n" for item in requests)
    result = subprocess.run(command, cwd=root, input=payload, text=True, capture_output=True,
                            timeout=15, check=True)
    if result.stderr:
        raise AssertionError(result.stderr)
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    if len(replies) != len(requests):
        raise AssertionError(f"expected {len(requests)} replies, got {len(replies)}")
    return replies


def copied_canonraw_source(mutate):
    """Copy only CanonRaw.pm; selected imports resolve from explicit fallback."""
    temporary = tempfile.TemporaryDirectory()
    root = Path(temporary.name)
    copied = root / "lib" / "Image" / "ExifTool"
    copied.mkdir(parents=True)
    source = Path(PINNED) / "lib" / "Image" / "ExifTool" / "CanonRaw.pm"
    target = copied / "CanonRaw.pm"
    shutil.copyfile(source, target)
    target.write_text(mutate(target.read_text()))
    (root / "fallback").symlink_to(Path(PINNED) / "lib", target_is_directory=True)
    return temporary, root


@unittest.skipUnless(PINNED and PERL, "set OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL")
class CiffOpaqueNativeReplay(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        version = subprocess.check_output([PERL, "-e", "print $^V"], text=True).strip()
        if version != CANONICAL_PERL:
            raise AssertionError(f"opaque replay requires canonical Perl {CANONICAL_PERL}, got {version}")
        if not (Path(PINNED) / "lib" / "Image" / "ExifTool" / "CanonRaw.pm").is_file():
            raise AssertionError("selected pinned source does not contain CanonRaw.pm")

    def assert_source(self, reply):
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["returned"]["numeric"], 1)
        process = reply["selection"]["process"]
        self.assertTrue(process["resolved"])
        self.assertEqual(process["name"], "Image::ExifTool::CanonRaw::ProcessCanonRaw")
        self.assertEqual(process["source_file"], "Image/ExifTool/CanonRaw.pm")
        self.assertEqual(process["source_root"], "lib")
        validate = reply["selection"]["validate_image"]
        self.assertTrue(validate["resolved"])
        self.assertEqual(validate["name"], "Image::ExifTool::ValidateImage")
        self.assertEqual(validate["source_file"], "Image/ExifTool.pm")

    def test_inline_explicit_undef_is_one_byte_and_unformatted_is_eight(self):
        replies = replay([
            request(order, [
                {"tag": 0x4001, "inline_hex": "0102030405060708"},
                {"tag": 0x6005, "inline_hex": "1112131415161718"},
            ])
            for order in ("II", "MM")
        ])
        for reply in replies:
            self.assert_source(reply)
            self.assertEqual(reply["found"][0]["name"]["string"], "FreeBytes")
            self.assertEqual(reply["selection"]["rows"][0]["format"]["string"], "undef")
            self.assertEqual(reply["selection"]["rows"][0]["binary"]["numeric"], 1)
            self.assertEqual(reply["found"][0]["stored_bytes"],
                             {"defined": True, "length": 1, "hex": "01"})
            self.assertEqual(reply["found"][1]["name"]["string"], "RawData")
            self.assertEqual(reply["found"][1]["stored_bytes"],
                             {"defined": True, "length": 8, "hex": "1112131415161718"})

    def test_external_raw_and_preview_payloads_keep_full_bytes_and_repair_jpeg(self):
        replies = replay([
            request(order, [
                {"tag": 0x2005, "offset": 80, "payload_hex": "0001ff80"},
                {"tag": 0x2007, "offset": 96, "payload_hex": "00d8ffdb99"},
                {"tag": 0x2008, "offset": 112, "payload_hex": "ffd8ffdb22"},
            ])
            for order in ("II", "MM")
        ])
        for reply in replies:
            self.assert_source(reply)
            raw, jpg, thumb = reply["found"]
            self.assertFalse(reply["selection"]["rows"][0]["format"]["defined"])
            self.assertEqual(reply["selection"]["rows"][1]["raw_conv"]["string"],
                             "$self->ValidateImage(\\$val,$tag)")
            self.assertEqual(raw["stored_bytes"]["hex"], "0001ff80")
            self.assertEqual(jpg["group2"]["string"], "Preview")
            self.assertEqual(thumb["group2"]["string"], "Preview")
            self.assertEqual(jpg["stored_bytes"]["hex"], "ffd8ffdb99")
            self.assertEqual(thumb["stored_bytes"]["hex"], "ffd8ffdb22")

    def test_rawdata_hash_receives_absolute_external_span_before_reporting(self):
        replies = replay([
            request(order, [{"tag": 0x2005, "offset": 123, "payload_hex": "0123456789abcdef"}],
                    image_data_hash=True)
            for order in ("II", "MM")
        ])
        for reply in replies:
            self.assert_source(reply)
            self.assertEqual(reply["found"][0]["stored_bytes"]["hex"], "0123456789abcdef")
            expected_hash = {
                "offset": {"defined": True, "numeric": 123, "string": "123"},
                "size": {"defined": True, "numeric": 8, "string": "8"},
                "mode": {"defined": True, "string": "raw"},
            }
            self.assertEqual(reply["image_data_hash"], [expected_hash])
            self.assertEqual([event["event"] for event in reply["trace"]],
                             ["ImageDataHash", "FoundTag"])
            self.assertEqual({key: reply["trace"][0][key] for key in expected_hash}, expected_hash)
            self.assertEqual(reply["trace"][1]["stored_bytes"], reply["found"][0]["stored_bytes"])

    def test_copied_source_main_process_rebinding_is_selected_and_fingerprinted(self):
        def rebind(source):
            marker = "sub ProcessCanonRaw($$$);\n"
            self.assertIn(marker, source)
            source = source.replace(marker, marker + "sub OxiDexReboundMain($$$);\n", 1)
            source = source.replace(r"PROCESS_PROC => \&ProcessCanonRaw,", r"PROCESS_PROC => \&OxiDexReboundMain,", 1)
            return source.replace("sub ProcessCanonRaw($$$)\n{", "sub OxiDexReboundMain($$$)\n{ return 37; }\n\nsub ProcessCanonRaw($$$)\n{", 1)

        temporary, root = copied_canonraw_source(rebind)
        with temporary:
            reply, = replay([request("II", [{"tag": 0x4001, "inline_hex": "0102030405060708"}])],
                              root=root, fallback="fallback")
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["returned"]["numeric"], 37)
        process = reply["selection"]["process"]
        self.assertTrue(process["resolved"])
        self.assertEqual(process["name"], "Image::ExifTool::CanonRaw::OxiDexReboundMain")
        self.assertEqual(process["source_root"], "lib")
        self.assertEqual(process["source_file"], "Image/ExifTool/CanonRaw.pm")
        self.assertEqual(reply["found"], [])

    def test_copied_source_noncode_main_process_is_rejected_before_byte_order_mutation(self):
        marker = r"PROCESS_PROC => \&ProcessCanonRaw,"
        for replacement in ("undef", "'not_a_code_ref'"):
            with self.subTest(replacement=replacement):
                def remove_process(source, replacement=replacement):
                    self.assertIn(marker, source)
                    source = source.replace(marker, f"PROCESS_PROC => {replacement},", 1)
                    # Execute the real rejection path with a native state-write
                    # trap, rather than inferring execution order from source.
                    return source + "\n{ no warnings qw(redefine prototype); " + \
                        "*Image::ExifTool::SetByteOrder = sub { " + \
                        "die 'byte order changed before rejecting processor'; }; }\n1;\n"

                temporary, root = copied_canonraw_source(remove_process)
                with temporary:
                    reply, = replay([request("MM", [{"tag": 0x4001, "inline_hex": "0102030405060708"}])],
                                      root=root, fallback="fallback")
                self.assertFalse(reply["ok"])
                self.assertEqual(reply["error"], "main_process_proc_unavailable")
                self.assertFalse(reply["selection"]["process"]["resolved"])
                self.assertEqual(reply["selection"]["process"]["reason"], "not_code")

    def test_copied_source_reordered_hash_is_observed_in_chronological_trace(self):
        def reorder(source):
            hash_line = "$raf->Seek($ptr, 0) and $et->ImageDataHash($raf, $size, 'raw');"
            report_line = "$et->FoundTag($tagInfo, $value);"
            self.assertIn(hash_line, source)
            self.assertIn(report_line, source)
            source = source.replace(hash_line, "# deliberately moved by native-source control", 1)
            return source.replace(report_line, report_line + "\n            " + hash_line, 1)

        temporary, root = copied_canonraw_source(reorder)
        with temporary:
            reply, = replay([request("II", [{"tag": 0x2005, "offset": 123, "payload_hex": "0123456789abcdef"}],
                                    image_data_hash=True)], root=root, fallback="fallback")
        self.assertTrue(reply["ok"], reply)
        self.assertEqual([event["event"] for event in reply["trace"]],
                         ["FoundTag", "ImageDataHash"])
        self.assertNotEqual(reply["selection"]["process"]["source_sha256"],
                            replay([request("II", [{"tag": 0x2005, "offset": 123, "payload_hex": "0123456789abcdef"}],
                                           image_data_hash=True)])[0]["selection"]["process"]["source_sha256"])


if __name__ == "__main__":
    unittest.main()
