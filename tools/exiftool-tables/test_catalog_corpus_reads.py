"""Catalog credit from corpus read receipts: exact coordinates, bound inputs."""
from __future__ import annotations

import json
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


def credit(coordinates, manifest="m" * 64, receipt=RECEIPT, native=None):
    derived = {"credited_coordinates": coordinates, "metric_c": {"corpus_files": 1}}
    native = {tuple(item) for item in coordinates} if native is None else native
    with patch.object(corpus.corpus_read_receipt, "validate", return_value=derived) as validate, \
            patch.object(corpus, "native_coordinates", return_value=native), \
            patch.object(corpus.runtime_inputs, "runtime_input_manifest", return_value=manifest):
        result = corpus.observed_reads(receipt, CATALOG, Path("."))
        validate.assert_called_once_with(receipt, "13.59")
        return result


def source_receipt(rows_by_file):
    """A receipt carrying only authentic source-capture transcripts."""
    native = {"perl": {"path": "/perl"}, "source_capture": {"path": "/capture.pl"}, "library": "/lib"}
    root = "/corpus"
    sources = {}
    for name, rows in rows_by_file.items():
        path = f"{root}/{name}"
        stdout = json.dumps({path: rows}).encode()
        sources[name] = {"command": corpus.corpus_read_receipt.source_command(native, path), "returncode": 0,
                         "stdout_hex": stdout.hex(), "stdout_sha256": corpus.corpus_read_receipt.sha(stdout),
                         "stderr_hex": "", "stderr_sha256": corpus.corpus_read_receipt.sha(b"")}
    return {"native": native, "corpus": {"root": root, "files": {name: "0" * 64 for name in rows_by_file}},
            "sources": sources}


class CorpusCreditTests(unittest.TestCase):
    def test_only_the_exact_source_row_is_credited(self):
        credited, native, counts = credit([["Image::ExifTool::Exif::Main", "256", 0],
                                           ["Image::ExifTool::APP12::PictureInfo", "PicLen", 0]])
        self.assertEqual(credited, {("Image::ExifTool::Exif::Main", "256", 0)})
        self.assertEqual((counts["credited_catalog_entries"], counts["credited_coordinates_outside_catalog"]), (1, 1))
        self.assertEqual(native, credited)

    def test_native_reads_bound_the_reachability_ceiling_separately_from_credit(self):
        read = {("Image::ExifTool::Exif::Main", "256", 0), ("Image::ExifTool::Exif::Main", "48256", 0),
                ("Image::ExifTool::APP12::PictureInfo", "PicLen", 0)}
        credited, native, counts = credit([["Image::ExifTool::Exif::Main", "256", 0]], native=read)
        self.assertEqual(native, {("Image::ExifTool::Exif::Main", "256", 0), ("Image::ExifTool::Exif::Main", "48256", 0)})
        self.assertEqual((counts["native_catalog_entries"], counts["native_coordinates_outside_catalog"]), (2, 1))
        self.assertEqual(counts["credited_catalog_entries"], 1)

    def test_credit_outside_the_native_reads_is_refused(self):
        with self.assertRaisesRegex(ValueError, "never read"):
            credit([["Image::ExifTool::Exif::Main", "256", 0]], native=set())

    def test_native_coordinates_come_from_authenticated_source_rows(self):
        receipt = source_receipt({
            "a.jpg": [["IFD0", "ImageWidth", "Image::ExifTool::Exif::Main", "256", 0],
                      ["Composite", "ImageSize", None, None, None],
                      ["System", "FileSize", "Image::ExifTool::Extra", "FileSize", 0]],
            "b.jpg": [["IFD0", "ImageWidth", "Image::ExifTool::Exif::Main", "256", 0],
                      ["ExifIFD", "Other", "Image::ExifTool::Exif::Main", "48256", 0]]})
        self.assertEqual(corpus.native_coordinates(receipt),
                         {("Image::ExifTool::Exif::Main", "256", 0), ("Image::ExifTool::Exif::Main", "48256", 0)})
        receipt["sources"]["a.jpg"]["stdout_hex"] = b"{}".hex()
        with self.assertRaisesRegex(ValueError, "hash differs"):
            corpus.native_coordinates(receipt)

    def test_receipt_from_another_runtime_or_exiftool_source_is_refused(self):
        with self.assertRaisesRegex(ValueError, "runtime differs"):
            credit([], manifest="n" * 64)
        other = {**RECEIPT, "native": {"library_fingerprint": {"Image/ExifTool.pm": "b" * 64}}}
        with self.assertRaisesRegex(ValueError, "source differs"):
            credit([], receipt=other)

    def test_no_receipt_credits_nothing(self):
        self.assertEqual(corpus.observed_reads(None, CATALOG, Path(".")), (set(), set(), {}))


if __name__ == "__main__":
    unittest.main()
