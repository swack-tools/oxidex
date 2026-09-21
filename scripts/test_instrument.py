import os
from pathlib import Path
import tempfile
import unittest

import instrument


class DirtyTreeScopeTests(unittest.TestCase):
    def setUp(self):
        self.override = os.environ.pop(instrument.DIRTY_OVERRIDE_ENV, None)
        self.addCleanup(self.restore_override)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def restore_override(self):
        if self.override is not None:
            os.environ[instrument.DIRTY_OVERRIDE_ENV] = self.override
        else:
            os.environ.pop(instrument.DIRTY_OVERRIDE_ENV, None)

    def state(self, *paths):
        return instrument.GitState(self.root, "a" * 40, "fixture", True, list(paths))

    def test_exact_regeneration_outputs_are_allowed_without_generic_override(self):
        owned = ["generated/a.rs", "generated/a-ledger.json", "generated/a-history.json"]
        self.assertFalse(instrument.refuse_if_dirty(
            self.state(*owned), "validator", allowed_dirty_paths=owned))

    def test_unrelated_dirty_path_remains_fatal(self):
        owned = ["generated/a.rs", "generated/a-ledger.json", "generated/a-history.json"]
        with self.assertRaisesRegex(SystemExit, "handwritten.rs"):
            instrument.refuse_if_dirty(
                self.state(*owned, "src/handwritten.rs"), "validator",
                allowed_dirty_paths=owned)


if __name__ == "__main__":
    unittest.main()
