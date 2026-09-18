"""dump_af_points.pl + codegen_af_points.py: absence is proven, shape change refuses.

Older ExifTool releases legitimately lack some Nikon `afPoints*` grids
(11.78's Nikon.pm declares only afPoints51/39/135/153). The dump records such
a grid as `kind: "absent"` only when its name occurs nowhere in the release's
own source; any other mismatch -- a changed declaration, a stray mention, a
grid moved to another module -- still refuses, because a silently mis-parsed
point grid is worse than no grid.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
from test_hydrated_catalog_universe import CANONICAL_LIB, CANONICAL_PERL, NATIVE_READY

DUMP = HERE / "dump_af_points.pl"
CODEGEN = HERE / "codegen_af_points.py"
COMMITTED_JSON = HERE / "af_points.json"
COMMITTED_RS = ROOT / "src/parsers/tiff/makernotes/nikon/af_points.rs"

# A Nikon.pm carrying only the four grids ExifTool 11.78 declares, in the
# exact declaration shapes the dump expects.
OLD_RELEASE_NIKON = """package Image::ExifTool::Nikon;
my %afPoints51 = (
    1 => 'C6', 2 => 'B6',
);
my %afPoints39 = (
    1 => 'C6',
);
my %afPoints135 = (
    1 => 'A1',
);
my %afPoints153 = (
    1 => 'E9',
);
sub PrintAFPoints { my ($val, $afPoints) = @_; }
1;
"""

ABSENT_IN_OLD = {
    "afPoints105": "hash",
    "afPoints81": "hash",
    "afPoints11": "array",
    "afPoints231": "array",
    "afPoints299": "array",
    "afPoints405": "array",
}


def pinned_version() -> str:
    return (ROOT / ".exiftool-version").read_text().strip()


def lib_version(lib: Path) -> str | None:
    match = re.search(r"""^\s*\$VERSION\s*=\s*['"]([^'"]+)['"]""",
                      (lib / "Image/ExifTool.pm").read_text(errors="replace"), re.M)
    return match.group(1) if match else None


class Release:
    """A minimal release lib/ tree in a temp dir."""

    def __init__(self, nikon: str, others: dict[str, str] | None = None):
        self.tmp = tempfile.TemporaryDirectory()
        self.lib = Path(self.tmp.name) / "lib"
        (self.lib / "Image/ExifTool").mkdir(parents=True)
        (self.lib / "Image/ExifTool.pm").write_text("package Image::ExifTool;\n$VERSION = '0.00';\n1;\n")
        self.nikon = self.lib / "Image/ExifTool/Nikon.pm"
        self.nikon.write_text(nikon)
        for name, text in (others or {}).items():
            (self.lib / "Image/ExifTool" / name).write_text(text)

    def dump(self) -> subprocess.CompletedProcess:
        out = Path(self.tmp.name) / "af_points.json"
        return subprocess.run([CANONICAL_PERL, str(DUMP), str(self.nikon), str(out)],
                              capture_output=True, text=True)

    def json(self) -> dict:
        return json.loads((Path(self.tmp.name) / "af_points.json").read_text())

    def codegen(self) -> tuple[subprocess.CompletedProcess, Path]:
        rs = Path(self.tmp.name) / "af_points.rs"
        proc = subprocess.run([sys.executable, str(CODEGEN), str(Path(self.tmp.name) / "af_points.json"),
                               str(rs)], capture_output=True, text=True)
        return proc, rs

    def close(self):
        self.tmp.cleanup()


@unittest.skipUnless(shutil.which(CANONICAL_PERL) or Path(CANONICAL_PERL).is_file(), "no perl")
class AbsentGridIsRecordedAsAbsent(unittest.TestCase):
    def setUp(self):
        self.release = Release(OLD_RELEASE_NIKON, {"Canon.pm": "package Image::ExifTool::Canon;\n1;\n"})
        self.addCleanup(self.release.close)

    def test_dump_records_each_missing_grid_with_source_proof(self):
        proc = self.release.dump()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        doc = self.release.json()
        for name, kind in ABSENT_IN_OLD.items():
            entry = doc[name]
            self.assertEqual(entry["kind"], "absent", name)
            self.assertEqual(entry["expected_kind"], kind, name)
            self.assertNotIn("points", entry, name)
            proof = entry["proof"]
            self.assertEqual(proof["source"], "Image/ExifTool/Nikon.pm")
            self.assertEqual(proof["occurrences_in_source"], 0)
            self.assertEqual(proof["occurrences_in_release"], 0)
            # ExifTool.pm, Nikon.pm and Canon.pm.
            self.assertEqual(proof["release_modules_scanned"], 3)
            self.assertRegex(proof["source_sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(doc["afPoints51"], {"kind": "hash", "points": {"1": "C6", "2": "B6"}})
        self.assertIn("afPoints105=absent", proc.stdout)

    def test_codegen_emits_no_points_for_an_absent_grid(self):
        self.assertEqual(self.release.dump().returncode, 0)
        proc, rs = self.release.codegen()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        text = rs.read_text()
        self.assertIn('pub const AF_POINTS_51: &[(u8, &str)] = &[(1, "C6"), (2, "B6")];', text)
        self.assertIn("pub const AF_POINTS_105: &[(u8, &str)] = &[];", text)
        self.assertIn("pub const AF_POINTS_81: &[(u8, &str)] = &[];", text)
        for n in (11, 231, 299, 405):
            self.assertIn(f"pub const AF_POINTS_{n}: &[&str] = &[];", text)
        self.assertIn("// afPoints105: absent from this release.", text)

    def test_codegen_refuses_an_absence_record_without_proof(self):
        self.assertEqual(self.release.dump().returncode, 0)
        doc = self.release.json()
        doc["afPoints105"]["proof"]["occurrences_in_source"] = 2
        (Path(self.release.tmp.name) / "af_points.json").write_text(json.dumps(doc))
        proc, _ = self.release.codegen()
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("does not carry a source-level proof", proc.stderr)

    def test_codegen_refuses_an_absence_of_the_wrong_kind(self):
        self.assertEqual(self.release.dump().returncode, 0)
        doc = self.release.json()
        doc["afPoints105"]["expected_kind"] = "array"
        (Path(self.release.tmp.name) / "af_points.json").write_text(json.dumps(doc))
        proc, _ = self.release.codegen()
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("afPoints105: absent as 'array', expected 'hash'", proc.stderr)


@unittest.skipUnless(shutil.which(CANONICAL_PERL) or Path(CANONICAL_PERL).is_file(), "no perl")
class ChangedShapeStillRefuses(unittest.TestCase):
    def refuse(self, nikon: str, others: dict[str, str] | None, message: str):
        release = Release(nikon, others)
        self.addCleanup(release.close)
        proc = release.dump()
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn(message, proc.stderr)
        self.assertFalse((Path(release.tmp.name) / "af_points.json").exists())

    def test_present_grid_in_a_changed_declaration_shape(self):
        nikon = OLD_RELEASE_NIKON.replace("1;\n", "our %afPoints105 = ( 1 => 'D8' );\n1;\n")
        self.refuse(nikon, None, "shape changed: could not find 'my %afPoints105 = ( ... );'")

    def test_grid_declared_with_the_wrong_sigil(self):
        nikon = OLD_RELEASE_NIKON.replace("1;\n", "my @afPoints81 = ( 'A1', 'A2' );\n1;\n")
        self.refuse(nikon, None, "shape changed: could not find 'my %afPoints81 = ( ... );'")

    def test_array_grid_no_longer_a_qw_list(self):
        nikon = OLD_RELEASE_NIKON.replace("1;\n", "my @afPoints231 = ( 'A1', 'A2' );\n1;\n")
        self.refuse(nikon, None, "shape changed: could not find 'my @afPoints231 = (qw(...));'")

    def test_a_mere_reference_is_not_absence(self):
        nikon = OLD_RELEASE_NIKON.replace("1;\n", "# see \\%afPoints405 for the Z9 grid\n1;\n")
        self.refuse(nikon, None, "shape changed: could not find 'my @afPoints405 = (qw(...));'")

    def test_afpoints11_mentioned_but_not_declared(self):
        nikon = OLD_RELEASE_NIKON.replace("1;\n", "PrintConv => \\%afPoints11,\n1;\n")
        self.refuse(nikon, None, "shape changed: could not find 'my %afPoints11 = ( ... );'")

    def test_grid_moved_to_another_module(self):
        self.refuse(OLD_RELEASE_NIKON, {"NikonCustom.pm": "my %afPoints105 = ( 1 => 'D8' );\n"},
                    "moved: 'afPoints105' no longer occurs in")

    def test_absence_needs_a_whole_release_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            nikon = Path(tmp) / "Nikon.pm"
            nikon.write_text(OLD_RELEASE_NIKON)
            proc = subprocess.run([CANONICAL_PERL, str(DUMP), str(nikon), str(Path(tmp) / "out.json")],
                                  capture_output=True, text=True)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("cannot prove absence", proc.stderr)


@unittest.skipUnless(NATIVE_READY, "set OXIDEX_PINNED_EXIFTOOL to the pinned ExifTool tree")
class PinnedReleaseUnchanged(unittest.TestCase):
    def test_pinned_release_regenerates_the_committed_artifacts_byte_for_byte(self):
        self.assertEqual(lib_version(CANONICAL_LIB), pinned_version(),
                         f"{CANONICAL_LIB} is not the pinned ExifTool release")
        with tempfile.TemporaryDirectory() as tmp:
            out_json = Path(tmp) / "af_points.json"
            out_rs = Path(tmp) / "af_points.rs"
            subprocess.run([CANONICAL_PERL, str(DUMP), str(CANONICAL_LIB / "Image/ExifTool/Nikon.pm"),
                            str(out_json)], check=True, capture_output=True)
            shutil.copyfile(COMMITTED_RS, out_rs)
            subprocess.run([sys.executable, str(CODEGEN), str(out_json), str(out_rs)],
                           check=True, capture_output=True)
            self.assertEqual(out_json.read_bytes(), COMMITTED_JSON.read_bytes())
            self.assertEqual(out_rs.read_bytes(), COMMITTED_RS.read_bytes())
        self.assertNotIn('"absent"', COMMITTED_JSON.read_text())


if __name__ == "__main__":
    unittest.main()
