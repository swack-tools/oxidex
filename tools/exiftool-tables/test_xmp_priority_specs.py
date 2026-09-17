"""XMP tag priorities: committed artifacts replay from the pinned capture."""
from __future__ import annotations

import copy
import json
import os
import subprocess
import unittest
from pathlib import Path

import xmp_priority_specs as specs

HERE = Path(__file__).resolve().parent
FRESH_INPUTS = ("EXIFTOOL_PERL", "OXIDEX_PINNED_EXIFTOOL")


class XmpPrioritySpecTests(unittest.TestCase):
    def setUp(self):
        self.capture = json.loads(specs.FIXTURE.read_text())

    def test_committed_rust_replays_from_the_fixture(self):
        from join_catalog_hydrated import quicktime_rust_matches
        self.assertTrue(quicktime_rust_matches(specs.render(self.capture), specs.RUST.read_text()))

    def test_capture_is_internally_consistent(self):
        self.assertEqual(self.capture["exiftool_version"], (specs.ROOT / ".exiftool-version").read_text().strip())
        namespaces = self.capture["namespaces"]
        # FoundXMP looks tables up by the prefix after %stdXlatNS.
        self.assertIn("iptcCore", namespaces)
        self.assertNotIn("Iptc4xmpCore", namespaces)
        self.assertEqual(namespaces["photomech"]["table"], "Image::ExifTool::PhotoMechanic::XMP")
        self.assertEqual(namespaces["tiff"]["table_priority"], 0)
        self.assertTrue(all(priority == 0 for priority in namespaces["exif"]["tags"].values()))
        self.assertEqual(namespaces["pdf"]["tags"]["Keywords"], -1)
        self.assertEqual(namespaces["xmp"]["tags"]["CreateDate"], 0)
        # Flattened structure tags are entries too.
        self.assertIn("FlashFired", namespaces["exif"]["tags"])
        for mutate in (lambda c: c.update(extra=1),
                       lambda c: c["namespaces"]["dc"].update(extra=1),
                       lambda c: c["namespaces"]["dc"]["tags"].update(Title="1"),
                       lambda c: c["namespaces"]["dc"]["tags"].update(Title=True),
                       lambda c: c["namespaces"]["dc"]["tags"].update({'Q"uote': 1}),
                       lambda c: c["namespaces"]["dc"].update(table_priority="0")):
            broken = copy.deepcopy(self.capture); mutate(broken)
            with self.assertRaises(ValueError):
                specs.validate(broken)

    def test_fixture_is_the_fresh_pinned_capture(self):
        missing = [name for name in FRESH_INPUTS if not os.environ.get(name)]
        if missing:
            if os.environ.get("GITHUB_ACTIONS"):
                self.fail(f"fresh pinned-source inputs missing in CI: {missing}")
            self.skipTest(f"set {', '.join(FRESH_INPUTS)} for the pinned source")
        source = Path(os.environ["OXIDEX_PINNED_EXIFTOOL"])
        library = source / "lib" if (source / "lib").is_dir() else source
        env = {key: value for key, value in os.environ.items() if not key.startswith("PERL5")}
        run = subprocess.run([os.environ["EXIFTOOL_PERL"], str(HERE / "capture_xmp_priorities.pl"), str(library)],
                             capture_output=True, text=True, env=env)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(json.loads(run.stdout), self.capture)


if __name__ == "__main__":
    unittest.main()
