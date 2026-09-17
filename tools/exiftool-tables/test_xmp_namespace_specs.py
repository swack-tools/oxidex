"""XMP namespace table: committed artifacts replay from the pinned capture."""
from __future__ import annotations

import copy
import json
import os
import subprocess
import unittest
from pathlib import Path

import xmp_namespace_specs as specs

HERE = Path(__file__).resolve().parent
FRESH_INPUTS = ("EXIFTOOL_PERL", "OXIDEX_PINNED_EXIFTOOL")


class XmpNamespaceSpecTests(unittest.TestCase):
    def setUp(self):
        self.capture = json.loads(specs.FIXTURE.read_text())

    def test_committed_rust_replays_from_the_fixture(self):
        from join_catalog_hydrated import quicktime_rust_matches
        self.assertTrue(quicktime_rust_matches(specs.render(self.capture), specs.RUST.read_text()))

    def test_capture_is_internally_consistent(self):
        self.assertEqual(self.capture["exiftool_version"], (specs.ROOT / ".exiftool-version").read_text().strip())
        lookup = specs.read_lookup(self.capture)
        self.assertEqual(lookup["http://purl.org/dc/elements/1.1/"], "dc")
        self.assertEqual(lookup["http://ns.exiftool.ca/1.0/"], "et")
        # Structure-only namespaces are write-path registrations, never read lookups.
        self.assertNotIn("http://ns.google.com/photos/dd/1.0/pose/", lookup)
        self.assertEqual(set(self.capture["tables"]), {"Device", "photomechanic"})
        for mutate in (lambda c: c["uri2ns_seed"].update({"http://x/": "x"}),
                       lambda c: c["static"].update(dup=c["static"]["dc"]),
                       lambda c: c["tables"].update(dc={"uri": "http://y/", "table": "T"}),
                       lambda c: c["tables"]["Device"].update(uri="http://purl.org/dc/elements/1.1/"),
                       lambda c: c["static"].update(bad='http://q"uote/'),
                       lambda c: c.update(extra=1)):
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
        run = subprocess.run([os.environ["EXIFTOOL_PERL"], str(HERE / "capture_xmp_namespaces.pl"), str(library)],
                             capture_output=True, text=True, env=env)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(json.loads(run.stdout), self.capture)


if __name__ == "__main__":
    unittest.main()
