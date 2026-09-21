"""Execute ``just verify-tables`` across instrumented native boundaries."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


REPO = Path(__file__).resolve().parents[2]
JUST = shutil.which("just")


class VerifyTablesRecipeTests(unittest.TestCase):
    def setUp(self):
        if JUST is None:
            self.skipTest("just is unavailable")
        self.temporary = tempfile.TemporaryDirectory(prefix="verify-tables-recipe-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.cache = self.root / "cache/13.59"
        self.tree = self.cache / "exiftool"
        (self.tree / "lib/Image").mkdir(parents=True)
        (self.tree / "lib/Image/ExifTool.pm").write_text("fixture\n", encoding="utf-8")
        (self.tree / "t/images").mkdir(parents=True)
        (self.tree / "t/images/OOXML.docx").write_bytes(b"fixture")
        (self.tree / "exiftool").write_text("fixture\n", encoding="utf-8")
        self.target = self.root / "target"
        self.target.mkdir()
        (self.target / "ambient-tmp").mkdir()
        self.calls = self.root / "calls.jsonl"
        self.perl = self.bin / "perl5.38.2"
        self._write_perl()
        self._write_uv()
        self._write_hostile("python3")
        self._write_hostile("perl")
        self._write_hostile("exiftool")

        # Keep the old recipe away from the network so RED is deterministic.
        for version in ("13.59", "13.58"):
            (self.root / f"legacy/exiftool-{version}/lib").mkdir(parents=True)

    def _write_perl(self):
        self.perl.write_text(
            "#!" + sys.executable + "\n"
            + textwrap.dedent(
                """
                import json
                import os
                from pathlib import Path
                import sys

                with Path(os.environ["VERIFY_TABLES_CALLS"]).open("a", encoding="utf-8") as handle:
                    handle.write(json.dumps({
                        "kind": "perl",
                        "argv": sys.argv[1:],
                        "env": {name: os.environ.get(name) for name in (
                            "EXIFTOOL_HOME", "EXIFTOOL_PERL", "EXIFTOOL_CACHE_DIR",
                            "OXIDEX_TABLES_PERL", "PERL5LIB", "PERLLIB", "PERL5OPT",
                            "TMPDIR", "PATH"
                        )},
                    }) + "\\n")
                args = sys.argv[1:]
                if os.environ.get("FAKE_PERL_DEGRADED") == "1" and any(
                    arg == "-MArchive::Zip" for arg in args
                ):
                    raise SystemExit(2)
                if "print $^V" in args:
                    print("v5.38.2", end="")
                elif any(arg == "-MArchive::Zip" for arg in args):
                    pass
                elif "-ver" in args:
                    print("13.59")
                elif "-FileType" in args:
                    print("DOCX")
                elif any(arg.endswith("dump_tables.pl") for arg in args):
                    print("{}")
                else:
                    raise SystemExit(64)
                """
            ),
            encoding="utf-8",
        )
        self.perl.chmod(0o755)

    def _write_uv(self):
        uv = self.bin / "uv"
        uv.write_text(
            "#!" + sys.executable + "\n"
            + textwrap.dedent(
                f"""
                import json
                import os
                from pathlib import Path
                import subprocess
                import sys

                args = sys.argv[1:]
                if args[:2] != ["run", "python"]:
                    raise SystemExit(98)
                command = args[2:]
                with Path(os.environ["VERIFY_TABLES_CALLS"]).open("a", encoding="utf-8") as handle:
                    handle.write(json.dumps({{
                        "kind": "python",
                        "argv": command,
                        "env": {{name: os.environ.get(name) for name in (
                            "EXIFTOOL_HOME", "EXIFTOOL_PERL", "EXIFTOOL_CACHE_DIR",
                            "OXIDEX_TABLES_PERL", "PERL5LIB", "PERLLIB", "PERL5OPT",
                            "TMPDIR", "CARGO_TARGET_DIR", "PATH"
                        )}},
                    }}) + "\\n")
                if command[0] == "tools/ci/release_oracle.py" or command[0] == "-c":
                    raise SystemExit(subprocess.run([{sys.executable!r}, *command]).returncode)
                raise SystemExit(0)
                """
            ),
            encoding="utf-8",
        )
        uv.chmod(0o755)

    def _write_hostile(self, name: str):
        path = self.bin / name
        path.write_text("#!/bin/sh\necho hostile " + name + " >&2\nexit 97\n", encoding="utf-8")
        path.chmod(0o755)

    def run_recipe(self, version: str = "", overrides: dict[str, str] | None = None):
        self.calls.unlink(missing_ok=True)
        environment = {
            **os.environ,
            "PATH": os.pathsep.join((str(self.bin), "/usr/bin", "/bin")),
            "VERIFY_TABLES_CALLS": str(self.calls),
            "OXIDEX_OPS_DIR": str(self.root / "ops"),
            "EXIFTOOL_PERL": str(self.perl),
            "EXIFTOOL_CACHE_DIR": str(self.cache),
            "CARGO_TARGET_DIR": str(self.target),
            "TMPDIR": str(self.target / "ambient-tmp"),
            "EXIFTOOL_HOME": str(self.root / "hostile-home"),
            "PERL5LIB": str(self.root / "hostile-perl5lib"),
            "PERLLIB": str(self.root / "hostile-perllib"),
            "PERL5OPT": "-MHostile",
            "OXIDEX_ET_CACHE": str(self.root / "legacy"),
            "PYTHONDONTWRITEBYTECODE": "1",
        }
        environment.update(overrides or {})
        command = [JUST, "--justfile", str(REPO / "justfile"), "--working-directory", str(REPO), "verify-tables"]
        if version:
            command.append(version)
        return subprocess.run(
            command,
            cwd=REPO,
            env=environment,
            text=True,
            capture_output=True,
            timeout=30,
        )

    def read_calls(self):
        if not self.calls.is_file():
            return []
        return [json.loads(line) for line in self.calls.read_text(encoding="utf-8").splitlines()]

    def test_one_verified_oracle_reaches_every_verifier_under_hostile_ambient_state(self):
        result = self.run_recipe()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.read_calls()
        python_calls = [call for call in calls if call["kind"] == "python"]
        perl_calls = [call for call in calls if call["kind"] == "perl"]
        by_script = {call["argv"][0]: call for call in python_calls if call["argv"]}

        release = by_script["tools/ci/release_oracle.py"]
        self.assertEqual(release["argv"][release["argv"].index("--repo") + 1], ".")
        verify = by_script["tools/exiftool-tables/verify.py"]
        self.assertEqual(verify["argv"][2], str(self.tree / "lib"))
        subdirs = by_script["tools/exiftool-tables/verify_subdirs.py"]
        self.assertEqual(subdirs["argv"][2], str(self.tree / "lib"))
        self.assertEqual(subdirs["argv"][subdirs["argv"].index("--perl") + 1], str(self.perl))
        self.assertEqual(subdirs["argv"][subdirs["argv"].index("--exiftool") + 1], str(self.tree / "exiftool"))

        serial = next(call for call in perl_calls if any(arg.endswith("dump_tables.pl") for arg in call["argv"]))
        self.assertEqual(serial["argv"][0], f"-I{self.tree / 'lib'}")
        self.assertEqual(serial["argv"][-1], str(self.tree / "lib"))
        probe_prefix = [f"-I{self.tree / 'lib'}", str(self.tree / "exiftool"), "-config", ""]
        self.assertTrue(any(call["argv"][:4] == probe_prefix and "-ver" in call["argv"] for call in perl_calls))
        self.assertTrue(any(call["argv"][:4] == probe_prefix and "-FileType" in call["argv"] for call in perl_calls))

        downstream = [verify, subdirs, serial]
        for call in downstream:
            self.assertEqual(call["env"]["EXIFTOOL_PERL"], str(self.perl))
            self.assertEqual(call["env"]["EXIFTOOL_CACHE_DIR"], str(self.cache))
            self.assertEqual(call["env"]["OXIDEX_TABLES_PERL"], str(self.perl))
            self.assertEqual(call["env"]["EXIFTOOL_HOME"], os.devnull)
            self.assertEqual(call["env"]["PERL5LIB"], "")
            self.assertEqual(call["env"]["PERLLIB"], "")
            self.assertEqual(call["env"]["PERL5OPT"], "")
            self.assertTrue(Path(call["env"]["TMPDIR"]).is_relative_to(self.target))

    def test_explicit_version_must_match_repository_pin(self):
        result = self.run_recipe("13.58")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match repository pin 13.59", result.stderr)
        self.assertEqual(self.read_calls(), [])

    def test_missing_or_degraded_perl_refuses_before_verifiers(self):
        for overrides, expected in (
            ({"EXIFTOOL_PERL": str(self.root / "missing-perl")}, "perl_path"),
            ({"FAKE_PERL_DEGRADED": "1"}, "Perl modules"),
        ):
            with self.subTest(overrides=overrides):
                result = self.run_recipe(overrides=overrides)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected, result.stderr)
                downstream = {
                    "tools/exiftool-tables/verify.py",
                    "tools/exiftool-tables/verify_serial_directory.py",
                    "tools/exiftool-tables/verify_subdirs.py",
                }
                self.assertFalse(
                    any(call["kind"] == "python" and call["argv"] and call["argv"][0] in downstream
                        for call in self.read_calls())
                )


if __name__ == "__main__":
    unittest.main()
