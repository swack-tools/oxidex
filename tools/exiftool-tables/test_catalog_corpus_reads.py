"""Catalog credit from corpus read receipts: exact coordinates, bound inputs."""
from __future__ import annotations

import unittest
from pathlib import Path
from unittest.mock import patch

import catalog_corpus_reads as corpus


def entry(table, raw_key, name):
    return {"table": table, "raw_key": raw_key, "variant_index": 0, "name": name,
            "groups": {"0": "X", "1": "X", "2": "Other"}}


CATALOG = {
    "exiftool_version": "13.59",
    "producer": {"sources": {"Image/ExifTool.pm": {"sha256": "a" * 64}}},
    "entries": [entry("Image::ExifTool::Exif::Main", "256", "ImageWidth"),
                entry("Image::ExifTool::Exif::Main", "48256", "ImageWidth")],
}
RECEIPT = {"producer": {"runtime_input_manifest_sha256": "m" * 64},
           "native": {"library_fingerprint": {"Image/ExifTool.pm": "a" * 64}}}


def credit(coordinates, manifest="m" * 64, receipt=RECEIPT):
    derived = {"credited_coordinates": coordinates, "metric_c": {"corpus_files": 1}}
    with patch.object(corpus.corpus_read_receipt, "validate", return_value=derived) as validate, \
            patch.object(corpus.runtime_inputs, "runtime_input_manifest", return_value=manifest):
        result = corpus.observed_reads(receipt, CATALOG, Path("."))
        validate.assert_called_once_with(receipt, "13.59")
        return result


class CorpusCreditTests(unittest.TestCase):
    def test_only_the_exact_source_row_is_credited(self):
        credited, counts = credit([["Image::ExifTool::Exif::Main", "256", 0],
                                   ["Image::ExifTool::APP12::PictureInfo", "PicLen", 0]])
        self.assertEqual(credited, {("Image::ExifTool::Exif::Main", "256", 0)})
        self.assertEqual((counts["credited_catalog_entries"], counts["credited_coordinates_outside_catalog"]), (1, 1))

    def test_receipt_from_another_runtime_or_exiftool_source_is_refused(self):
        with self.assertRaisesRegex(ValueError, "runtime differs"):
            credit([], manifest="n" * 64)
        other = {**RECEIPT, "native": {"library_fingerprint": {"Image/ExifTool.pm": "b" * 64}}}
        with self.assertRaisesRegex(ValueError, "source differs"):
            credit([], receipt=other)

    def test_no_receipt_credits_nothing(self):
        self.assertEqual(corpus.observed_reads(None, CATALOG, Path(".")), (set(), {}))


if __name__ == "__main__":
    unittest.main()
