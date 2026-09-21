"""Exercise the benchmark instrument with hosted CI's pinned-source channel."""

import os
from pathlib import Path
import json
import re
import shlex
import subprocess
import sys
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[2]


class BenchmarkOracleTests(unittest.TestCase):
    def test_single_file_scenario_preserves_empty_config_and_paths_at_timer_boundary(self):
        script = (REPO / "benches/exiftool_comparison.sh").read_text()
        scenario = re.search(r"^benchmark_single_file\(\) \{.*?^\}", script, re.M | re.S).group(0)
        with tempfile.TemporaryDirectory(prefix="benchmark arguments ") as directory:
            root = Path(directory)
            fixture = root / "jpeg/simple/sample_with_exif.jpg"
            fixture.parent.mkdir(parents=True)
            fixture.touch()
            (root / "Canon.jpg").touch()
            capture = root / "capture"
            capture.write_text(
                f"#!{sys.executable}\nimport json, shlex, sys\n"
                "print(json.dumps([shlex.split(value) for value in sys.argv[-2:]]))\n"
            )
            capture.chmod(0o755)
            oracle = ["/portable perl", "-I/library path", "/source/exiftool", "-config", ""]
            setup = (
                "stamp_load() { :; }; stamp_after() { :; }; load1() { echo 0; };\n"
                f"FIXTURE_DIR={shlex.quote(str(root))}; EXIFTOOL_CORPUS=$FIXTURE_DIR; TEMP_DIR=$FIXTURE_DIR\n"
                f"HF=({shlex.quote(str(capture))}); EXIFTOOL=({shlex.join(oracle)})\n"
                f"EXIFTOOL_CMD={shlex.quote(shlex.join(oracle))}; OXIDEX_BIN='/oxidex binary'\n"
            )
            result = subprocess.run(["bash", "-c", setup + scenario + "\nbenchmark_single_file"],
                                    text=True, capture_output=True, check=True)
            commands = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("[[")]
            self.assertEqual(commands, [
                [[*oracle, str(fixture)], ["/oxidex binary", str(fixture)]],
                [[*oracle, "-j", "-a", "-G1", str(root / "Canon.jpg")],
                 ["/oxidex binary", "-j", "-a", "-G1", str(root / "Canon.jpg")]],
            ])

    def run_instrument(self, *, version="13.59", docx="DOCX", module_exit=0):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / "source"
            (source / "lib/Image").mkdir(parents=True)
            (source / "t/images").mkdir(parents=True)
            (source / "exiftool").write_text("fixture source\n")
            (source / "lib/Image/ExifTool.pm").write_text("$VERSION = '13.59';\n")
            (source / "t/images/OOXML.docx").write_text("fixture\n")
            perl = root / "perl"
            perl.write_text("#!/bin/sh\ncase \"$*\" in\n"
                            f" *-MArchive::Zip*) exit {module_exit};;\n"
                            f" *-ver*) echo {version};;\n"
                            f" *-FileType*) echo {docx};;\n"
                            " *'-e 1'*) exit 0;;\n *) exit 72;;\nesac\n")
            perl.chmod(0o755)
            binary = root / "oxidex"
            binary.write_text("#!/bin/sh\necho 'oxidex test fixture'\n")
            binary.chmod(0o755)
            return subprocess.run(
                [sys.executable, str(REPO / "benches/instrument_check.py"), str(binary)],
                cwd=REPO, env={**os.environ, "EXIFTOOL_SOURCE": str(source),
                               "OXIDEX_TABLES_PERL": str(perl), "OXIDEX_BENCHMARK_CI": "1",
                               "EXIFTOOL_CACHE_DIR": str(root / "runner-cache"),
                               "EXIFTOOL_PERL": "perl", "OXIDEX_ALLOW_DIRTY_TREE": "1"},
                text=True, capture_output=True, timeout=20,
            )

    def test_ci_instrument_uses_pinned_source_and_capable_runner_perl(self):
        result = self.run_instrument()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("EXIFTOOL_VERSION=13.59", result.stdout)
        self.assertIn("docx:    DOCX", result.stderr)
        self.assertIn("CI pinned source", result.stdout)

    def test_ci_instrument_refuses_skew_degraded_docx_and_missing_modules(self):
        for overrides, message in (({"version": "13.58"}, "version"),
                                   ({"docx": "ZIP"}, "DOCX"),
                                   ({"module_exit": 1}, "Perl")):
            with self.subTest(overrides=overrides):
                result = self.run_instrument(**overrides)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
