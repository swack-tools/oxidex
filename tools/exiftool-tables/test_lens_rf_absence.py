"""dump_lens_alternatives.pl / verify_lens_alternatives.py: a missing Canon RF table.

ExifTool 11.78's Canon FileInfo has no key 61 (RFLensType). The producer then
emits an empty CANON_RF table, but only when module_absence.py --table proves
from the release's own lib/ that no file names RFLensType. The version label
is never the proof. Key 61 present in a changed shape still refuses, and so do
a moved entry and a leftover reference. 13.59 output stays byte-identical.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
from test_hydrated_catalog_universe import CANONICAL_LIB, CANONICAL_PERL, NATIVE_READY  # noqa: E402
from test_module_absence import OLD_LIB  # noqa: E402

DUMP = HERE / "dump_lens_alternatives.pl"
VERIFY = HERE / "verify_lens_alternatives.py"
COMMITTED = ROOT / "src/composite/lens_alternatives.rs"
RF_ENTRY = "    0x3d => { #IB\n        Name => 'RFLensType',"


def env():
    return {k: v for k, v in os.environ.items() if not k.startswith("PERL")}


def fake_root(root: Path, pin: str) -> Path:
    """The producer, verifier and helper under a chosen pin."""
    tools = root / "tools/exiftool-tables"
    tools.mkdir(parents=True)
    for name in ("dump_lens_alternatives.pl", "verify_lens_alternatives.py", "module_absence.py"):
        shutil.copy2(HERE / name, tools)
    shutil.copy2(ROOT / "rustfmt.toml", root)
    (root / ".exiftool-version").write_text(pin + "\n")
    return tools


def run(tools: Path, tree: Path, out: Path):
    dump = subprocess.run([str(CANONICAL_PERL), str(tools / DUMP.name), "--exiftool-dir", str(tree),
                           "--out", str(out)], capture_output=True, text=True, env=env(), timeout=300)
    if dump.returncode:
        return dump, None
    verify = subprocess.run([sys.executable, str(tools / VERIFY.name), str(out), "--exiftool-dir", str(tree),
                             "--perl", str(CANONICAL_PERL)], capture_output=True, text=True, env=env(), timeout=300)
    return dump, verify


@unittest.skipUnless(NATIVE_READY, "canonical Perl and pinned ExifTool tree unavailable")
class CanonRfAbsence(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.pin = (ROOT / ".exiftool-version").read_text().strip()

    def mutated_tree(self, canon=None, everywhere=None) -> Path:
        tree = self.tmp / "exiftool"
        shutil.copytree(CANONICAL_LIB.parent / "lib", tree / "lib")
        if everywhere:
            for path in (tree / "lib").rglob("*"):
                if path.is_file() and path.name != "Canon.pm":
                    data = path.read_bytes()
                    if b"RFLensType" in data:
                        path.write_bytes(data.replace(b"RFLensType", everywhere))
        if canon:
            pm = tree / "lib/Image/ExifTool/Canon.pm"
            text = pm.read_text(encoding="latin-1")
            self.assertEqual(text.count(RF_ENTRY), 1)
            pm.write_text(canon(text), encoding="latin-1")
        return tree

    def test_proven_absence_emits_empty_rf_table_and_verifies(self):
        # No key 61, and RFLensType named nowhere in the release.
        tree = self.mutated_tree(
            canon=lambda t: t.replace(RF_ENTRY, "    0x3e3d => { #IB\n        Name => 'RFLensKindX',")
            .replace("RFLensType", "RFLensKindX"),
            everywhere=b"RFLensKindX")
        out = self.tmp / "lens.rs"
        dump, verify = run(fake_root(self.tmp / "root", self.pin), tree, out)
        self.assertEqual(dump.returncode, 0, dump.stderr)
        self.assertIn("Canon RF: table absent from this release", dump.stderr)
        text = out.read_text()
        self.assertIn("pub static CANON_RF_LENS_ALTERNATIVES: [(i64, &str, &[&str]); 0] = [];", text)
        self.assertEqual(verify.returncode, 0, verify.stdout)
        self.assertEqual(json.loads(verify.stdout)["canon_rf_absence"]["kind"], "exiftool_table_absent_v1")
        # The verifier refuses an empty RF table not bound to this release.
        out.write_text(text.replace("release inventory sha256 ", "release inventory sha256 0"))
        verify = subprocess.run([sys.executable, str(self.tmp / "root/tools/exiftool-tables" / VERIFY.name),
                                 str(out), "--exiftool-dir", str(tree), "--perl", str(CANONICAL_PERL)],
                                capture_output=True, text=True, env=env(), timeout=300)
        self.assertEqual(verify.returncode, 1)
        self.assertIn("not bound to this release", verify.stdout)

    def test_moved_entry_refuses(self):
        # Key 61 gone, but the RFLensType entry still exists under another key.
        tree = self.mutated_tree(canon=lambda t: t.replace("    0x3d => { #IB\n", "    0x3e3d => { #IB\n", 1))
        dump, _ = run(fake_root(self.tmp / "root", self.pin), tree, self.tmp / "lens.rs")
        self.assertNotEqual(dump.returncode, 0)
        self.assertIn("absence is not proven", dump.stderr)
        self.assertFalse((self.tmp / "lens.rs").exists())

    def test_reference_left_elsewhere_refuses(self):
        # Canon.pm no longer has the table, but Exif.pm still names it.
        tree = self.mutated_tree(
            canon=lambda t: t.replace(RF_ENTRY, "    0x3e3d => { #IB\n        Name => 'RFLensKindX',")
            .replace("RFLensType", "RFLensKindX"))
        dump, _ = run(fake_root(self.tmp / "root", self.pin), tree, self.tmp / "lens.rs")
        self.assertNotEqual(dump.returncode, 0)
        self.assertIn("absence is not proven", dump.stderr)

    def test_present_but_changed_shape_still_refuses(self):
        tree = self.mutated_tree(canon=lambda t: t.replace(
            RF_ENTRY + "\n        Format => 'int16u',\n        PrintConv => {",
            RF_ENTRY + "\n        Format => 'int16u',\n        PrintConv => 'sprintf(\"%d\", $val)',\n        Unused => {"))
        dump, _ = run(fake_root(self.tmp / "root", self.pin), tree, self.tmp / "lens.rs")
        self.assertNotEqual(dump.returncode, 0)
        self.assertIn("canon_rf: expected a hash", dump.stderr)

    def test_pinned_output_is_the_committed_file(self):
        out = self.tmp / "lens.rs"
        dump = subprocess.run([str(CANONICAL_PERL), str(DUMP), "--exiftool-dir", str(CANONICAL_LIB.parent),
                               "--out", str(out)], capture_output=True, text=True, env=env(), timeout=300)
        self.assertEqual(dump.returncode, 0, dump.stderr)
        self.assertEqual(out.read_bytes(), COMMITTED.read_bytes())

    @unittest.skipUnless(OLD_LIB.is_dir(), "ExifTool 11.78 source tree unavailable")
    def test_real_1178_tree(self):
        out = self.tmp / "lens.rs"
        dump, verify = run(fake_root(self.tmp / "root", "11.78"), OLD_LIB.parent, out)
        self.assertEqual(dump.returncode, 0, dump.stderr)
        self.assertIn("[(i64, &str, &[&str]); 0] = [];", out.read_text())
        self.assertEqual(verify.returncode, 0, verify.stdout)


if __name__ == "__main__":
    unittest.main()
