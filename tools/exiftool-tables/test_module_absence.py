"""module_absence.py and its InfiRay / NikonSettings call sites.

A release that predates a module (11.78 has neither InfiRay.pm nor
NikonSettings.pm) is recorded as "absent" only when that is proven from the
release's own lib/: the module file fails to stat with ENOENT and none of the
caller's names occurs in any file of the tree. The version label is never the
proof. A present module that is broken or changed still refuses in the
generator's strict path, and an absence is distinguishable from a crash.
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
from module_absence import (  # noqa: E402
    SCHEMA, TABLE_SCHEMA, NotAbsent, prove_module_absent, prove_table_absent, validate_absence_record)
from test_hydrated_catalog_universe import CANONICAL_LIB, CANONICAL_PERL, NATIVE_READY  # noqa: E402

INFIRAY_GEN = ROOT / "scripts/gen_infiray_tables.pl"
NIKON_GEN = HERE / "gen_nikon_settings_tables.py"
NIKON_VERIFY = HERE / "verify_nikon_settings.py"
DUMP = HERE / "dump_tables.pl"
COMMITTED_INFIRAY = ROOT / "src/parsers/jpeg/app_segments/infiray_tables.rs"
COMMITTED_NIKON = ROOT / "src/parsers/tiff/makernotes/nikon/settings_tables.rs"
PERL = str(CANONICAL_PERL) if CANONICAL_PERL else "perl"

# The historical 11.78 tree the upgrade rehearsal reads, when this host has it.
OLD_LIB = Path(os.environ.get(
    "OXIDEX_TEST_EXIFTOOL_1178_LIB",
    "/Users/allen/Documents/Codex/2026-09-10/oxidex-worktree-cleanup-audit/handoff-continuation/"
    "sony-plain-recovery-z2ml2k_w/shared-pilot/write-upgrade-integration-20260913/"
    "first-random-pair-20260913/source-attempt-01/sources/"
    "exiftool-11.78-ca8685788f5763c547349f239764bd19cf1952da/lib"))


def pinned_version() -> str:
    return (ROOT / ".exiftool-version").read_text().strip()


def make_lib(root: Path, label: str = "11.78", core_extra: str = "", modules: dict | None = None) -> Path:
    """A minimal release lib/: a loadable Image::ExifTool plus named modules."""
    lib = root / "lib"
    (lib / "Image/ExifTool").mkdir(parents=True)
    (lib / "Image/ExifTool.pm").write_text(
        f"package Image::ExifTool;\n$VERSION = '{label}';\n{core_extra}\n1;\n")
    for name, body in (modules or {"Nikon": "package Image::ExifTool::Nikon;\n1;\n"}).items():
        (lib / f"Image/ExifTool/{name}.pm").write_text(body)
    return lib


def committed_names(path: Path) -> set[str]:
    return set(re.findall(r'name: "([^"]+)"', path.read_text()))


class ProveModuleAbsent(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_absent_is_recorded_with_source_proof(self):
        lib = make_lib(self.tmp)
        record = prove_module_absent(lib, "InfiRay", ["IJPEG", "HasIJPEG"])
        self.assertEqual(record["kind"], SCHEMA)
        self.assertIs(record["module_file_present"], False)
        self.assertEqual(record["occurrences_in_release"], {"InfiRay": 0, "IJPEG": 0, "HasIJPEG": 0})
        self.assertEqual(record["release_files_scanned"], 2)
        self.assertEqual(record["release_pm_files_scanned"], 2)
        self.assertRegex(record["exiftool_pm_sha256"], r"^[0-9a-f]{64}$")
        self.assertRegex(record["release_inventory_sha256"], r"^[0-9a-f]{64}$")
        validate_absence_record(record, "InfiRay")

    def test_version_label_is_never_the_proof(self):
        # The pinned label on a tree without the module is still absent, and
        # an old label on a tree with the module is still present.
        absent = prove_module_absent(make_lib(self.tmp / "a", label=pinned_version()), "NikonSettings")
        self.assertEqual(absent["occurrences_in_release"], {"NikonSettings": 0})
        present = make_lib(self.tmp / "b", label="11.78",
                           modules={"NikonSettings": "package Image::ExifTool::NikonSettings;\n1;\n"})
        with self.assertRaisesRegex(NotAbsent, "present"):
            prove_module_absent(present, "NikonSettings")

    def test_reference_without_module_file_refuses(self):
        for text in ("# see InfiRay.pm", "GetTagTable('Image::ExifTool::InfiRay::Version');",
                     "$$self{HasIJPEG} = 1;"):
            with self.subTest(text=text):
                lib = make_lib(self.tmp / str(abs(hash(text))), core_extra=text)
                with self.assertRaisesRegex(NotAbsent, "absent but referenced"):
                    prove_module_absent(lib, "InfiRay", ["IJPEG", "HasIJPEG"])

    def test_reference_in_a_non_perl_file_refuses(self):
        lib = make_lib(self.tmp)
        (lib / "Image/ExifTool/README").write_text("NikonSettings moved here\n")
        with self.assertRaisesRegex(NotAbsent, "README:NikonSettings"):
            prove_module_absent(lib, "NikonSettings")

    def test_incomplete_tree_refuses(self):
        lib = self.tmp / "lib"
        (lib / "Image/ExifTool").mkdir(parents=True)
        with self.assertRaisesRegex(NotAbsent, "not a complete release"):
            prove_module_absent(lib, "InfiRay")

    def test_validate_refuses_an_incomplete_record(self):
        good = prove_module_absent(make_lib(self.tmp), "InfiRay", ["IJPEG"])
        for key, value in (("occurrences_in_release", {"InfiRay": 1, "IJPEG": 0}),
                           ("module", "Nikon"), ("module_file_present", True),
                           ("exiftool_pm_sha256", "0" * 63), ("release_files_scanned", 0)):
            with self.subTest(key=key), self.assertRaises(NotAbsent):
                validate_absence_record({**good, key: value}, "InfiRay")

    def test_rejects_non_identifier_tokens(self):
        with self.assertRaises(ValueError):
            prove_module_absent(make_lib(self.tmp), "Infi.Ray")

    @unittest.skipUnless(OLD_LIB.is_dir(), "ExifTool 11.78 source tree unavailable")
    def test_real_1178_tree_lacks_both_modules(self):
        for module, tokens in (("InfiRay", ["IJPEG", "HasIJPEG"]), ("NikonSettings", ["ProcessNikonSettings"])):
            with self.subTest(module=module):
                record = prove_module_absent(OLD_LIB, module, tokens)
                self.assertEqual(set(record["occurrences_in_release"].values()), {0})

    @unittest.skipUnless(CANONICAL_LIB and CANONICAL_LIB.is_dir(), "pinned ExifTool tree unavailable")
    def test_pinned_tree_has_both_modules(self):
        for module in ("InfiRay", "NikonSettings"):
            with self.subTest(module=module), self.assertRaisesRegex(NotAbsent, "present"):
                prove_module_absent(CANONICAL_LIB, module)


CANON_OLD = "package Image::ExifTool::Canon;\n%Image::ExifTool::Canon::FileInfo = ( 60 => 'LensType' );\n1;\n"


class ProveTableAbsent(unittest.TestCase):
    """The same proof one level down: a named table of a module that exists."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_absent_table_is_recorded_with_module_hash(self):
        lib = make_lib(self.tmp, modules={"Canon": CANON_OLD})
        record = prove_table_absent(lib, "Canon", "RFLensType")
        self.assertEqual(record["kind"], TABLE_SCHEMA)
        self.assertIs(record["module_file_present"], True)
        self.assertEqual(record["table"], "RFLensType")
        self.assertEqual(record["occurrences_in_release"], {"RFLensType": 0})
        self.assertRegex(record["module_sha256"], r"^[0-9a-f]{64}$")
        validate_absence_record(record, "Canon", "RFLensType")
        with self.assertRaises(NotAbsent):
            validate_absence_record(record, "Canon")        # not a module absence
        with self.assertRaises(NotAbsent):
            validate_absence_record({**record, "module_sha256": None}, "Canon", "RFLensType")

    def test_table_name_still_in_its_module_refuses_as_present(self):
        # Present, perhaps in a changed shape: the strict loader decides, never "absent".
        lib = make_lib(self.tmp, modules={"Canon": CANON_OLD.replace("60 => 'LensType'",
                                                                     "61 => { Name => 'RFLensType' }")})
        with self.assertRaisesRegex(NotAbsent, "present in Image/ExifTool/Canon.pm"):
            prove_table_absent(lib, "Canon", "RFLensType")

    def test_table_named_elsewhere_refuses_as_moved(self):
        lib = make_lib(self.tmp, modules={"Canon": CANON_OLD,
                                          "Exif": "package Image::ExifTool::Exif;\n# use Canon RFLensType\n1;\n"})
        with self.assertRaisesRegex(NotAbsent, "moved"):
            prove_table_absent(lib, "Canon", "RFLensType")

    def test_missing_module_is_not_a_table_absence(self):
        with self.assertRaisesRegex(NotAbsent, "prove the module absent instead"):
            prove_table_absent(make_lib(self.tmp), "Canon", "RFLensType")

    def test_cli_selects_table_granularity(self):
        lib = make_lib(self.tmp, modules={"Canon": CANON_OLD})
        run = subprocess.run([sys.executable, str(HERE / "module_absence.py"), "--lib", str(lib),
                              "--module", "Canon", "--table", "RFLensType"], capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(json.loads(run.stdout)["kind"], TABLE_SCHEMA)

    @unittest.skipUnless(OLD_LIB.is_dir(), "ExifTool 11.78 source tree unavailable")
    def test_real_1178_canon_has_no_rf_lens_table(self):
        record = prove_table_absent(OLD_LIB, "Canon", "RFLensType")
        self.assertEqual(record["occurrences_in_release"], {"RFLensType": 0})

    @unittest.skipUnless(CANONICAL_LIB and CANONICAL_LIB.is_dir(), "pinned ExifTool tree unavailable")
    def test_pinned_canon_has_the_rf_lens_table(self):
        with self.assertRaisesRegex(NotAbsent, "present in Image/ExifTool/Canon.pm"):
            prove_table_absent(CANONICAL_LIB, "Canon", "RFLensType")


def fake_repo(root: Path, pin: str) -> Path:
    """The generator, its pin resolver and the helper under a chosen pin."""
    (root / "scripts/lib").mkdir(parents=True)
    (root / "tools/exiftool-tables").mkdir(parents=True)
    shutil.copy2(INFIRAY_GEN, root / "scripts")
    shutil.copy2(ROOT / "scripts/lib/ExiftoolPin.pm", root / "scripts/lib")
    shutil.copy2(HERE / "module_absence.py", root / "tools/exiftool-tables")
    (root / ".exiftool-version").write_text(pin + "\n")
    return root / "scripts/gen_infiray_tables.pl"


def run_infiray(gen: Path, lib: Path) -> subprocess.CompletedProcess:
    env = {k: v for k, v in os.environ.items() if not k.startswith("PERL")}
    env["OXIDEX_EXIFTOOL_LIB"] = str(lib)
    return subprocess.run([PERL, str(gen)], capture_output=True, text=True, env=env, timeout=120)


@unittest.skipUnless(NATIVE_READY, "canonical Perl unavailable")
class InfiRayGenerator(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_absent_module_emits_empty_tables_with_proof(self):
        lib = make_lib(self.tmp / "src", label="11.78")
        run = run_infiray(fake_repo(self.tmp / "repo", "11.78"), lib)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("InfiRay module absent from this release", run.stdout)
        self.assertIn("pub(crate) const GENERATED_FIELD_COUNT: usize = 0;", run.stdout)
        self.assertEqual(run.stdout.count("usize = usize::MAX;"), 7)
        self.assertEqual(run.stdout.count("&[Field] = &[];"), 7)
        self.assertNotIn("Field { offset", run.stdout)
        record = prove_module_absent(lib, "InfiRay", ["IJPEG", "HasIJPEG"])
        self.assertIn(record["exiftool_pm_sha256"], run.stdout)
        self.assertIn(record["release_inventory_sha256"], run.stdout)
        for name in committed_names(COMMITTED_INFIRAY):
            self.assertNotIn(f'"{name}"', run.stdout)

    def test_broken_module_crash_is_refused_not_absent(self):
        lib = make_lib(self.tmp / "src", modules={"InfiRay": "package Image::ExifTool::InfiRay;\ndie 'boom';\n"})
        run = run_infiray(fake_repo(self.tmp / "repo", "11.78"), lib)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("boom", run.stderr)
        self.assertNotIn("absent", run.stdout + run.stderr)

    def test_referenced_but_missing_module_is_refused(self):
        lib = make_lib(self.tmp / "src", core_extra="# $$self{HasIJPEG} set by APP2")
        run = run_infiray(fake_repo(self.tmp / "repo", "11.78"), lib)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("absence is not proven", run.stderr)
        self.assertNotIn("absent from this release", run.stdout)

    @unittest.skipUnless(CANONICAL_LIB and CANONICAL_LIB.is_dir(), "pinned ExifTool tree unavailable")
    def test_present_but_changed_module_still_refuses(self):
        lib = self.tmp / "lib"
        shutil.copytree(CANONICAL_LIB, lib)
        pm = lib / "Image/ExifTool/InfiRay.pm"
        text = pm.read_text(encoding="latin-1")
        changed = text.replace("%Image::ExifTool::InfiRay::Version = (",
                               "%Image::ExifTool::InfiRay::Version = (\n    FORMAT => 'int16u',", 1)
        self.assertNotEqual(changed, text)
        pm.write_text(changed, encoding="latin-1")
        run = run_infiray(fake_repo(self.tmp / "repo", pinned_version()), lib)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("REFUSING", run.stderr)
        self.assertIn("FORMAT", run.stderr)

    @unittest.skipUnless(CANONICAL_LIB and CANONICAL_LIB.is_dir(), "pinned ExifTool tree unavailable")
    def test_pinned_output_is_the_committed_file(self):
        run = run_infiray(INFIRAY_GEN, CANONICAL_LIB)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout.encode(), COMMITTED_INFIRAY.read_bytes())

    @unittest.skipUnless(OLD_LIB.is_dir(), "ExifTool 11.78 source tree unavailable")
    def test_real_1178_tree_is_absent(self):
        run = run_infiray(fake_repo(self.tmp / "repo", "11.78"), OLD_LIB)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("InfiRay module absent from this release", run.stdout)
        self.assertNotIn("Field { offset", run.stdout)


def run_nikon(dump: dict, out: Path, lib: Path | None) -> subprocess.CompletedProcess:
    path = out.with_suffix(".json")
    path.write_text(json.dumps(dump))
    argv = [sys.executable, str(NIKON_GEN), str(path), "-o", str(out)]
    if lib is not None:
        argv += ["--exiftool-lib", str(lib)]
    return subprocess.run(argv, capture_output=True, text=True, timeout=120)


class NikonSettingsGenerator(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.dump = {"exiftool_version": pinned_version(), "modules": {"Nikon": {"tables": {}}}}

    def test_absent_module_emits_empty_table_with_proof(self):
        self.emit_absent()

    def emit_absent(self):
        lib = make_lib(self.tmp / "src")
        out = self.tmp / "settings_tables.rs"
        run = run_nikon(self.dump, out, lib)
        self.assertEqual(run.returncode, 0, run.stderr)
        text = out.read_text()
        self.assertIn("NikonSettings module absent from this release", text)
        self.assertIn("pub(super) const SETTINGS_TAGS: &[E] = &[];", text)
        self.assertNotIn("E {", text)
        self.assertIn('"module_absent": true', run.stderr)
        for name in committed_names(COMMITTED_NIKON):
            self.assertNotIn(f'"{name}"', text)
        return lib, out

    def test_missing_from_dump_without_lib_is_refused(self):
        out = self.tmp / "settings_tables.rs"
        run = run_nikon(self.dump, out, None)
        self.assertEqual(run.returncode, 1)
        self.assertIn("pass --exiftool-lib", run.stderr)
        self.assertFalse(out.exists())

    def test_failed_load_is_refused_not_absent(self):
        # dump_tables.pl SKIPs a module that dies on require, so the dump
        # alone looks like an absence; the module file on disk says otherwise.
        lib = make_lib(self.tmp / "src", modules={
            "Nikon": "package Image::ExifTool::Nikon;\n1;\n",
            "NikonSettings": "package Image::ExifTool::NikonSettings;\ndie 'boom';\n"})
        out = self.tmp / "settings_tables.rs"
        run = run_nikon(self.dump, out, lib)
        self.assertEqual(run.returncode, 1)
        self.assertIn("not proven absent: present", run.stderr)
        self.assertFalse(out.exists())

    def test_dump_from_another_tree_is_refused(self):
        lib = make_lib(self.tmp / "src")
        dump = {**self.dump, "modules": {"Canon": {"tables": {}}}}
        run = run_nikon(dump, self.tmp / "settings_tables.rs", lib)
        self.assertEqual(run.returncode, 1)
        self.assertIn("dump modules not in", run.stderr)

    def test_present_but_changed_contract_still_refuses(self):
        dump = {**self.dump, "modules": {"NikonSettings": {"tables": {"Main": {
            "meta": {"GROUPS": {"0": "MakerNotes", "2": "Camera"}, "FORMAT": "int16u",
                     "PROCESS_PROC": {"__name": "Image::ExifTool::NikonSettings::ProcessNikonSettings"}},
            "tags": {}}}}}}
        run = run_nikon(dump, self.tmp / "settings_tables.rs", make_lib(self.tmp / "src"))
        self.assertEqual(run.returncode, 1)
        self.assertIn("contract changed", run.stderr)

    def test_verifier_accepts_only_the_bound_empty_table(self):
        lib, out = self.emit_absent()
        verify = [sys.executable, str(NIKON_VERIFY), "--exiftool-dir", str(lib.parent),
                  "--perl", PERL, "--input", str(out)]
        run = subprocess.run(verify, capture_output=True, text=True, timeout=120)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn('"module_absent": true', run.stdout)
        text = out.read_text()
        for tampered in (text.replace("&[];", '&[E { id: 0x0001, name: "X" }];'),
                         re.sub(r"inventory sha256 [0-9a-f]{64}", "inventory sha256 " + "0" * 64, text)):
            out.write_text(tampered)
            run = subprocess.run(verify, capture_output=True, text=True, timeout=120)
            self.assertEqual(run.returncode, 1, run.stdout)
        # A reference appearing in the release after generation breaks the proof.
        out.write_text(text)
        (lib / "Image/ExifTool/Nikon.pm").write_text("package Image::ExifTool::Nikon;\n# NikonSettings\n1;\n")
        run = subprocess.run(verify, capture_output=True, text=True, timeout=120)
        self.assertEqual(run.returncode, 1)
        self.assertIn("not proven absent", run.stderr)

    @unittest.skipUnless(NATIVE_READY, "canonical Perl and pinned ExifTool tree unavailable")
    def test_pinned_output_is_the_committed_file(self):
        env = {k: v for k, v in os.environ.items() if not k.startswith("PERL")}
        dump = self.tmp / "dump.json"
        with dump.open("wb") as fh:
            subprocess.run([PERL, str(DUMP), "--reader-only", str(CANONICAL_LIB), "NikonSettings"],
                           stdout=fh, stderr=subprocess.DEVNULL, env=env, check=True, timeout=300)
        out = self.tmp / "settings_tables.rs"
        run = subprocess.run([sys.executable, str(NIKON_GEN), str(dump), "-o", str(out),
                              "--exiftool-lib", str(CANONICAL_LIB)], capture_output=True, text=True, timeout=120)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(out.read_bytes(), COMMITTED_NIKON.read_bytes())


if __name__ == "__main__":
    unittest.main()
