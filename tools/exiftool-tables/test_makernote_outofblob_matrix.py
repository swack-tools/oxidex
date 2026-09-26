"""Unit checks of makernote_outofblob_matrix.py's survey helpers.

No ExifTool needed: the oracle is a stub returning `-j` rows.
"""
import unittest

import makernote_outofblob_matrix as M


class _Rows:
    def __init__(self, rows):
        self.rows = rows

    def json(self, args, path):
        return self.rows


class OffsetTargetsTests(unittest.TestCase):
    def test_plural_offset_and_count_pairs_are_recognised(self):
        # StripOffsets/StripByteCounts, FreeOffsets/FreeByteCounts and
        # TileOffsets/TileByteCounts are IsOffset pairs with plural names;
        # the generated Rust inventory pairs them the same way.
        rows = {
            "MakerNotes:StripOffsets": 100, "MakerNotes:StripByteCounts": 20,
            "MakerNotes:FreeOffsets": 200, "MakerNotes:FreeByteCounts": 30,
            "MakerNotes:TileOffsets": 300, "MakerNotes:TileByteCounts": 40,
        }
        got = sorted(M.offset_targets(_Rows(rows), "x.jpg"))
        self.assertEqual(got, [
            ("FreeOffsets", "isoffset", 200, 30),
            ("StripOffsets", "isoffset", 100, 20),
            ("TileOffsets", "isoffset", 300, 40),
        ])

    def test_singular_pairs_still_recognised(self):
        rows = {
            "Casio:PreviewImageStart": 10, "Casio:PreviewImageLength": 5,
            "Olympus:ThumbnailOffset": 50, "Olympus:ThumbnailLength": 6,
            "Sony:HiddenDataOffset": 70, "Sony:HiddenDataLength": 0,
        }
        got = sorted(M.offset_targets(_Rows(rows), "x.jpg"))
        self.assertEqual(got, [
            ("PreviewImageStart", "isoffset", 10, 5),
            ("ThumbnailOffset", "isoffset", 50, 6),
        ])


if __name__ == "__main__":
    unittest.main()
