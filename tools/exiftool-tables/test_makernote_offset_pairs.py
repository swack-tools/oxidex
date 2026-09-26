"""The generated maker-note offset/length pairs match the pinned ExifTool.

Skipped without a loadable pinned tree (CI's tools shards export
OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL, so there it always runs).
"""
import unittest
from pathlib import Path

import makernote_offset_pairs as M


class GeneratedPairsTests(unittest.TestCase):
    def test_generated_file_matches_the_pinned_tree(self):
        tree = M.default_tree()
        if tree is None or not (Path(tree) / "lib/Image/ExifTool.pm").is_file():
            self.skipTest("no pinned ExifTool tree")
        rows = M.inventory(M.default_perl(), Path(tree))
        self.assertEqual(M.OUTPUT.read_text(), M.render(rows, M.release(Path(tree))))

    def test_every_pair_is_in_the_rust_list(self):
        # Independent of the renderer: each offsetpair row of the inventory
        # appears as a tuple in the committed Rust source.
        tree = M.default_tree()
        if tree is None or not (Path(tree) / "lib/Image/ExifTool.pm").is_file():
            self.skipTest("no pinned ExifTool tree")
        text = M.OUTPUT.read_text()
        for kind, offset, length, _ in M.inventory(M.default_perl(), Path(tree)):
            if kind == "offsetpair":
                self.assertIn(f'("{offset}", "{length}")', text)
            else:
                self.assertIn(f'"{offset}",', text)
        # the pairs the maintainer's cases rest on
        for pair in ('("PreviewImageStart", "PreviewImageLength")', '("ThumbnailOffset", "ThumbnailLength")',
                     '("HiddenDataOffset", "HiddenDataLength")'):
            self.assertIn(pair, text)
        self.assertIn('"OriginalDecisionDataOffset",', text)


if __name__ == "__main__":
    unittest.main()
