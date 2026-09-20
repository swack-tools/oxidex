"""Contract tests for the authenticated generated-route attribution receipt."""

import importlib.util
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[3]
MODULE = ROOT / "tools/exiftool-tables/genshare/attribute.py"
SPEC = importlib.util.spec_from_file_location("genshare_attribute", MODULE)
attribute = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(attribute)


def digest(char):
    return char * 64


def authenticated_receipt():
    process = {
        "returncode": 0,
        "binary_sha256": digest("b"),
        "stdout_sha256": digest("c"),
        "stderr_sha256": digest("d"),
        "output_sha256": digest("e"),
    }
    return {
        "schema": "genshare-receipt/v2",
        "source": {"commit": "a" * 40, "tree": "f" * 40, "dirty": False},
        "oracle": {
            "version": "13.59",
            "perl_sha256": digest("1"),
            "exiftool_sha256": digest("2"),
            "docx_filetype": "DOCX",
        },
        "corpus": {"manifest_sha256": digest("3"), "files": 3},
        "token_set": ["engine"],
        "control": process,
        "probes": {"engine": process},
        "inertness": {"equal": True, "files": 3, "differences": 0},
        "per_occurrence_deltas": {"engine": {"matched_lost": 1, "residual": 0}},
    }


class ReceiptAuthenticationTests(unittest.TestCase):
    def test_historical_summary_without_paired_provenance_is_refused(self):
        with self.assertRaisesRegex(attribute.ReceiptError, "schema"):
            attribute.validate_receipt({"engine": {"matched_lost": 50509}})

    def test_requires_raw_process_hashes_for_each_paired_side(self):
        receipt = authenticated_receipt()
        del receipt["probes"]["engine"]["stderr_sha256"]
        with self.assertRaisesRegex(attribute.ReceiptError, "stderr_sha256"):
            attribute.validate_receipt(receipt)

    def test_accepts_a_complete_reconciled_paired_receipt(self):
        attribute.validate_receipt(authenticated_receipt())


if __name__ == "__main__":
    unittest.main()
