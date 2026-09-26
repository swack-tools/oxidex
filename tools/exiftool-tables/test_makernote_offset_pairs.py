"""The generated maker-note offset/length pairs match the pinned ExifTool.

Skipped without a loadable pinned tree (CI's tools shards export
OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL, so there it always runs).
"""
import os
import subprocess  # nosec B404 -- list-argv only
import tempfile
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


    def test_a_module_that_fails_to_load_aborts_the_inventory(self):
        # A pinned tree whose Perl cannot load one module (a missing
        # dependency) must not yield a smaller inventory that passes: the
        # walk aborts naming the module and Perl's error.
        tree = M.default_tree()
        if tree is None or not (Path(tree) / "lib/Image/ExifTool.pm").is_file():
            self.skipTest("no pinned ExifTool tree")
        with tempfile.TemporaryDirectory() as tmp:
            # the pinned lib/ by symlink, plus one module whose load fails
            def mirror(src: Path, dst: Path, into: tuple[str, ...]) -> None:
                dst.mkdir(parents=True)
                for item in src.iterdir():
                    if into and item.name == into[0]:
                        mirror(item, dst / item.name, into[1:])
                    else:
                        os.symlink(item, dst / item.name)

            mirror(Path(tree) / "lib", Path(tmp) / "lib", ("Image", "ExifTool"))
            lib = Path(tmp) / "lib/Image"
            (lib / "ExifTool/ZZNeedsMissingDep.pm").write_text(
                "package Image::ExifTool::ZZNeedsMissingDep;\nuse Oxidex::No::Such::Dependency;\n1;\n"
            )
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                M.inventory(M.default_perl(), Path(tmp))
            self.assertIn("Image::ExifTool::ZZNeedsMissingDep", caught.exception.stderr)
            self.assertIn("Oxidex/No/Such/Dependency.pm", caught.exception.stderr)


if __name__ == "__main__":
    unittest.main()
