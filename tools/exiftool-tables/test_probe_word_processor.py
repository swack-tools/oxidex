"""Native-only JSONL replay checks for selected word PROCESS_PROC callbacks.

The Perl helper loads the native table and invokes its actual PROCESS_PROC. No
compiler, descriptor, or copied recognizer participates in these assertions.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
import unittest


PINNED = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
PERL = os.environ.get("EXIFTOOL_PERL")
REPO_ROOT = Path(__file__).resolve().parents[2]
PROBE = REPO_ROOT / "tools" / "exiftool-tables" / "probe_word_processor.pl"


def request(name, order, data_hex, *, members=None, module="CanonCustom", table="FunctionsD30"):
    return {
        "protocol": "oxidex.word_processor.v1",
        "module": module,
        "table": table,
        "case": {
            "name": name,
            "byte_order": order,
            "data_hex": data_hex,
            "dir_start": 0,
            "dir_len": len(bytes.fromhex(data_hex)),
            "members": {} if members is None else members,
        },
    }


def replay(root, requests, *, fallback=None):
    """Run JSONL with a relative library argument and caller timeout."""
    command = [PERL, str(PROBE), "--lib", "lib"]
    if fallback is not None:
        command.extend(["--fallback-lib", "fallback_lib"])
    payload = "".join(json.dumps(item, separators=(",", ":")) + "\n" for item in requests)
    result = subprocess.run(
        command,
        cwd=root,
        input=payload,
        text=True,
        capture_output=True,
        timeout=10,
        check=True,
    )
    if result.stderr:
        raise AssertionError(f"native helper wrote stderr: {result.stderr}")
    lines = [json.loads(line) for line in result.stdout.splitlines()]
    if len(lines) != len(requests):
        raise AssertionError(f"expected {len(requests)} JSONL responses, got {len(lines)}")
    return lines


@unittest.skipUnless(PINNED and PERL, "set OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL for native replay")
class NativeWordProcessorReplay(unittest.TestCase):
    def test_normal_both_orders_authenticates_selected_table_and_full_handle_tag_arguments(self):
        # Same two native u16 words encoded in II and MM order.
        replies = replay(PINNED, [
            request("ii-normal", "II", "060002010403"),
            request("mm-normal", "MM", "000601020304", module="Image::ExifTool::CanonCustom"),
        ])
        for reply in replies:
            with self.subTest(case=reply["request"]["case"]):
                self.assertTrue(reply["ok"], reply)
                self.assertEqual(reply["returned"]["numeric"], 1)
                self.assertEqual(reply["byte_order"]["active"], reply["byte_order"]["requested"])
                self.assertEqual(reply["byte_order"]["restored"], reply["byte_order"]["before"])
                self.assertEqual(reply["unexpected_side_effects"], [])
                self.assertEqual([tag["raw_id"]["numeric"] for tag in reply["handle_tags"]], [1, 3])
                self.assertEqual([tag["value"]["numeric"] for tag in reply["handle_tags"]], [2, 4])
                self.assertEqual([tag["index"]["numeric"] for tag in reply["handle_tags"]], [0, 1])
                for tag in reply["handle_tags"]:
                    self.assertEqual(tag["format"]["string"], "int8u")
                    self.assertEqual(tag["count"]["numeric"], 1)
                    self.assertEqual(tag["size"]["numeric"], 1)
                    self.assertEqual(tag["raw_args"][0]["table_reference"], "selected_table")
                process = reply["selection"]["process"]
                binding = reply["selection"]["reader_binding"]
                self.assertTrue(process["resolved"])
                self.assertEqual(process["name"], "Image::ExifTool::CanonCustom::ProcessCanonCustom")
                self.assertEqual(process["source_file"], "Image/ExifTool/CanonCustom.pm")
                self.assertRegex(process["source_sha256"], r"^[0-9a-f]{64}$")
                self.assertEqual(binding["requested"], "Image::ExifTool::CanonCustom::Get16u")
                self.assertEqual(binding["name"], "Image::ExifTool::Get16u")

    def test_header_first_missing_model_exception_odd_short_empty_and_bad_header(self):
        replies = replay(PINNED, [
            # Exact size accepts before reading the absent Model predicate.
            request("exact-missing-model", "II", "04000201"),
            # Only D60's native exception admits header 3 in five bytes.
            request("d60-length-exception", "II", "0300020134", members={"Model": "EOS D60"}),
            request("missing-model-rejects-exception", "II", "0300020134"),
            request("odd-short-mm", "MM", "00050102ff"),
            request("empty", "II", ""),
            request("bad-header", "MM", "000401020304"),
        ])
        exact, d60, missing, odd, empty, bad = replies
        self.assertTrue(exact["ok"])
        self.assertEqual([tag["raw_id"]["numeric"] for tag in exact["handle_tags"]], [1])
        self.assertTrue(d60["ok"])
        self.assertEqual([tag["raw_id"]["numeric"] for tag in d60["handle_tags"]], [1, 0])
        self.assertEqual([tag["value"]["numeric"] for tag in d60["handle_tags"]], [2, 0])
        self.assertTrue(missing["ok"])
        self.assertEqual(missing["returned"]["numeric"], 0)
        self.assertEqual(missing["handle_tags"], [])
        self.assertTrue(missing["warnings"])
        self.assertTrue(odd["ok"])
        self.assertEqual([tag["raw_id"]["numeric"] for tag in odd["handle_tags"]], [1, 0])
        self.assertEqual([tag["value"]["numeric"] for tag in odd["handle_tags"]], [2, 0])
        self.assertTrue(empty["ok"])
        self.assertEqual(empty["returned"]["numeric"], 1)
        self.assertEqual(empty["handle_tags"], [])
        self.assertTrue(bad["ok"])
        self.assertEqual(bad["returned"]["numeric"], 0)
        self.assertEqual(bad["handle_tags"], [])
        self.assertTrue(bad["warnings"])

    def copied_root(self, before, after, *, whole_file=False):
        tmp = TemporaryDirectory()
        root = Path(tmp.name)
        source = Path(PINNED) / "lib" / "Image" / "ExifTool" / "CanonCustom.pm"
        target = root / "lib" / "Image" / "ExifTool"
        target.mkdir(parents=True)
        text = source.read_text()
        if whole_file:
            self.assertIn(before, text)
            changed = text.replace(before, after, 1)
        else:
            marker = "sub ProcessCanonCustom($$$)\n{"
            prefix, process = text.split(marker, 1)
            self.assertIn(before, process)
            changed = prefix + marker + process.replace(before, after, 1)
        target.joinpath("CanonCustom.pm").write_text(changed)
        root.joinpath("fallback_lib").symlink_to(Path(PINNED) / "lib", target_is_directory=True)
        self.addCleanup(tmp.cleanup)
        return root

    def test_copied_mask_and_package_local_reader_rebinding_change_the_native_trace(self):
        wide = self.copied_root("$val & 0xff", "$val & 0x1ff")
        wide_reply = replay(wide, [request("wide-mask", "II", "0400ff01")], fallback=True)[0]
        self.assertTrue(wide_reply["ok"], wide_reply)
        self.assertEqual(wide_reply["handle_tags"][0]["value"]["numeric"], 511)
        self.assertEqual(wide_reply["selection"]["process"]["source_root"], "lib")
        self.assertEqual(wide_reply["selection"]["reader_binding"]["source_root"], "fallback_lib")

        rebound = self.copied_root(
            "use Image::ExifTool::Exif;",
            "use Image::ExifTool::Exif;\n*Image::ExifTool::CanonCustom::Get16u = \\&Image::ExifTool::Get32u;",
            whole_file=True,
        )
        rebound_reply = replay(rebound, [request("rebound-reader", "II", "060002010403")], fallback=True)[0]
        self.assertTrue(rebound_reply["ok"], rebound_reply)
        self.assertEqual(rebound_reply["selection"]["reader_binding"]["name"], "Image::ExifTool::Get32u")
        self.assertEqual(rebound_reply["returned"]["numeric"], 0)
        self.assertEqual(rebound_reply["handle_tags"], [])
        self.assertTrue(rebound_reply["warnings"])

    def test_package_qualified_callback_and_invalid_request_are_explicit_failures(self):
        direct = self.copied_root(
            "$et->HandleTag($tagTablePtr, $tag, $val,",
            "Image::ExifTool::HandleTag($et, $tagTablePtr, $tag, $val,",
        )
        direct_reply = replay(direct, [request("direct-callback", "II", "04000201")], fallback=True)[0]
        self.assertFalse(direct_reply["ok"])
        self.assertIn("package-qualified HandleTag", direct_reply["error"])
        self.assertEqual(direct_reply["handle_tags"], [])

        invalid = request("outside", "II", "04000201")
        invalid["case"]["dir_len"] = 5
        reply = replay(PINNED, [invalid])[0]
        self.assertFalse(reply["ok"])
        self.assertEqual(reply["error"]["kind"], "case")


if __name__ == "__main__":
    unittest.main()
