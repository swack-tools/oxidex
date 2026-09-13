"""Native-only JSONL replay checks for selected serial PROCESS_PROC callbacks.

The helper executes the native table-owned processor and observes only its
object callbacks. It does not import a compiler or fabricate ReadValue events.
"""

import json
import os
from pathlib import Path
import struct
import subprocess
import unittest


PINNED = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
PERL = os.environ.get("EXIFTOOL_PERL")
REPO_ROOT = Path(__file__).resolve().parents[2]
PROBE = REPO_ROOT / "tools" / "exiftool-tables" / "probe_serial_processor.pl"
CANONICAL_PERL = "v5.38.2"


def words(byte_order, values):
    """Encode native u16 carrier words without interpreting signed payloads."""
    return b"".join((value & 0xFFFF).to_bytes(2, byte_order) for value in values)


def afinfo_words(point_count, *, serial_11=3, serial_12=4, unknown_words=None):
    """Construct a bounded AFInfo record in source serial-index order.

    The fixture owns only carrier bytes. Native GetTagInfo selects the table
    alternative and native ProcessSerialData evaluates all dynamic counts.
    """
    values = [point_count, 1, 1000, 800, 200, 150, 40, 30]
    values.extend([-7] * point_count)  # index 8: signed x positions
    values.extend([9] * point_count)   # index 9: signed y positions
    values.extend([1] * ((point_count + 15) // 16))  # index 10: focus words
    if unknown_words is None:
        values.extend([serial_11, serial_12])
    else:
        if len(unknown_words) != 8:
            raise ValueError("AFInfo unknown record is exactly eight u16 words")
        values.extend(unknown_words)
        values.append(serial_12)
    return values


def request(name, order, payload, *, members=None, unknown=False, verbose=True):
    return {
        "protocol": "oxidex.serial_processor.v1",
        "module": "Canon",
        "table": "AFInfo",
        "case": {
            "name": name,
            "byte_order": order,
            "data_hex": payload.hex(),
            "dir_start": 0,
            "dir_len": len(payload),
            "members": {} if members is None else members,
            "unknown": int(unknown),
            "verbose": int(verbose),
        },
    }


def replay(root, requests):
    command = [PERL, str(PROBE), "--lib", "lib"]
    payload = "".join(json.dumps(item, separators=(",", ":")) + "\n" for item in requests)
    result = subprocess.run(
        command,
        cwd=root,
        input=payload,
        text=True,
        capture_output=True,
        timeout=15,
        check=True,
    )
    if result.stderr:
        raise AssertionError(f"native helper wrote stderr: {result.stderr}")
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    if len(replies) != len(requests):
        raise AssertionError(f"expected {len(requests)} JSONL responses, got {len(replies)}")
    return replies


def names(records):
    return [record["tag_info"]["name"]["string"] for record in records]


def numeric(record, key):
    return record[key]["numeric"]


@unittest.skipUnless(PINNED and PERL, "set OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL for canonical native replay")
class NativeSerialProcessorReplay(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        version = subprocess.check_output([PERL, "-e", "print $^V"], text=True).strip()
        if version != CANONICAL_PERL:
            raise AssertionError(
                f"serial replay requires canonical Perl {CANONICAL_PERL}, got {version}"
            )
        if not (Path(PINNED) / "lib" / "Image" / "ExifTool" / "Canon.pm").is_file():
            raise AssertionError("selected pinned source does not contain Canon.pm")

    def assert_common(self, reply):
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["returned"]["numeric"], 1)
        self.assertEqual(reply["byte_order"]["active"], reply["byte_order"]["requested"])
        self.assertEqual(reply["byte_order"]["restored"], reply["byte_order"]["before"])
        self.assertIsNone(reply["byte_order"]["restore_error"])
        process = reply["selection"]["process"]
        self.assertTrue(process["resolved"])
        self.assertEqual(process["name"], "Image::ExifTool::Canon::ProcessSerialData")
        self.assertEqual(process["source_file"], "Image/ExifTool/Canon.pm")
        self.assertRegex(process["source_sha256"], r"^[0-9a-f]{64}$")
        self.assertRegex(process["source_body_sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(reply["observability"]["read_value"],
                         "unobserved: ProcessSerialData calls package ReadValue directly")
        self.assertEqual(reply["observability"]["dynamic_count_eval"],
                         "unobserved: Perl eval occurs inside ProcessSerialData before verbose callback")
        self.assertEqual(reply["option_state"]["unknown_before"], reply["option_state"]["unknown_after"])
        self.assertFalse(reply["option_state"]["no_unknown_after"]["defined"])
        self.assertTrue(all(item["table_reference"] == "selected_table" for item in reply["get_tag_info"]))

    def test_both_orders_capture_selected_native_processor_and_dynamic_coordinates(self):
        source = afinfo_words(1)
        replies = replay(PINNED, [
            request("ii-normal", "II", words("little", source), members={}),
            request("mm-normal", "MM", words("big", source), members={}),
        ])
        for reply in replies:
            with self.subTest(case=reply["request"]["case"]):
                self.assert_common(reply)
                self.assertEqual(names(reply["found_tags"])[8:11], [
                    "AFAreaXPositions", "AFAreaYPositions", "AFPointsInFocus",
                ])
                verbose = reply["verbose_info"]
                self.assertEqual([numeric(item, "index") for item in verbose], list(range(13)))
                self.assertEqual(numeric(verbose[8]["named_arguments"], "Count"), 1)
                self.assertEqual(numeric(verbose[8]["named_arguments"], "Size"), 2)
                self.assertEqual(numeric(verbose[8]["named_arguments"], "Value"), -7)
                self.assertEqual(numeric(verbose[9]["named_arguments"], "Value"), 9)
                self.assertEqual(reply["verbose_dirs"][0][0]["string"], "SerialData")
                self.assertEqual(reply["verbose_dirs"][0][2]["numeric"], len(source) * 2)

    def test_missing_non_eos_and_eos_model_choose_or_stop_at_serial_11(self):
        payload = words("little", afinfo_words(1))
        missing, powershot, eos = replay(PINNED, [
            request("missing-model", "II", payload),
            request("non-eos", "II", payload, members={"Model": "PowerShot G7"}),
            request("eos", "II", payload, members={"Model": "EOS 40D"}),
        ])
        for reply in (missing, powershot, eos):
            self.assert_common(reply)
        self.assertEqual(names(missing["found_tags"])[-2:], ["PrimaryAFPoint", "PrimaryAFPoint"])
        self.assertEqual(names(powershot["found_tags"])[-2:], ["PrimaryAFPoint", "PrimaryAFPoint"])
        self.assertEqual([numeric(item, "index") for item in eos["get_tag_info"]], list(range(12)))
        self.assertFalse(eos["get_tag_info"][-1]["result"]["defined"])
        self.assertEqual([numeric(item, "index") for item in eos["verbose_info"]], list(range(11)))
        self.assertNotIn("PrimaryAFPoint", names(eos["found_tags"]))

    def test_zero_truncated_and_large_dynamic_count_stop_without_fabricated_reads(self):
        zero = words("little", afinfo_words(0))
        # Exactly eight fixed scalar fields: index 8 is selected but its
        # count-2 dynamic coordinate array does not fit and is not observed.
        truncated = words("little", afinfo_words(2)[:8])
        large = words("little", afinfo_words(53)[:8])
        zero_reply, truncated_reply, large_reply = replay(PINNED, [
            request("zero-count", "II", zero),
            request("truncated-dynamic", "II", truncated),
            request("large-dynamic", "II", large),
        ])
        self.assert_common(zero_reply)
        self.assertEqual([numeric(item, "index") for item in zero_reply["verbose_info"]], list(range(13)))
        self.assertEqual(numeric(zero_reply["verbose_info"][8]["named_arguments"], "Count"), 0)
        self.assertEqual(numeric(zero_reply["verbose_info"][10]["named_arguments"], "Size"), 0)
        self.assertNotIn("AFAreaXPositions", names(zero_reply["found_tags"]))
        for reply in (truncated_reply, large_reply):
            self.assert_common(reply)
            self.assertEqual([numeric(item, "index") for item in reply["get_tag_info"]], list(range(9)))
            self.assertEqual([numeric(item, "index") for item in reply["verbose_info"]], list(range(8)))
            self.assertEqual(len(reply["found_tags"]), 8)
            self.assertEqual(reply["warnings"], [])

    def test_unknown_alternative_consumes_eight_words_and_restores_original_option(self):
        payload = words("little", afinfo_words(
            1, unknown_words=[101, 102, 103, 104, 105, 106, 107, 108], serial_12=9
        ))
        hidden, visible = replay(PINNED, [
            request("unknown-hidden", "II", payload, members={"Model": "PowerShot G7", "AFInfoCount": "36"}),
            request("unknown-visible", "II", payload, members={"Model": "PowerShot G7", "AFInfoCount": "36"}, unknown=True),
        ])
        for reply in (hidden, visible):
            self.assert_common(reply)
            at_11 = reply["verbose_info"][11]["named_arguments"]
            self.assertEqual(numeric(at_11, "Count"), 8)
            self.assertEqual(numeric(at_11, "Size"), 16)
            self.assertEqual(numeric(reply["verbose_info"][-1], "index"), 12)
            unknown_events = [event for event in reply["option_events"]
                              if event.get("method") == "Options" and event.get("argument") == "Unknown"
                              and event.get("mode") == "set"]
            self.assertEqual(len(unknown_events), 2)
            self.assertEqual(unknown_events[0]["after"]["numeric"], 1)
            self.assertEqual(unknown_events[-1]["after"], reply["option_state"]["unknown_before"])
        self.assertNotIn("Canon_AFInfo_0x000b", names(hidden["found_tags"]))
        self.assertIn("Canon_AFInfo_0x000b", names(visible["found_tags"]))

    def test_empty_input_reports_no_read_callbacks_and_keeps_option_state(self):
        reply, = replay(PINNED, [request("empty", "MM", b"")])
        self.assert_common(reply)
        self.assertEqual([numeric(item, "index") for item in reply["get_tag_info"]], [0])
        self.assertEqual(reply["verbose_info"], [])
        self.assertEqual(reply["found_tags"], [])
        self.assertEqual(reply["warnings"], [])


if __name__ == "__main__":
    unittest.main()
