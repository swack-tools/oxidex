"""Focused subprocess-contract tests for the SubDirectory native oracle."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import textwrap
import types
import unittest
from unittest import mock


MODULE_PATH = Path(__file__).with_name("verify_subdirs.py")


def load_module():
    """Load the helper without importing verify.py's process-global Perl."""
    verify_stub = types.ModuleType("verify")
    spec = importlib.util.spec_from_file_location("verify_subdirs_under_test", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    with mock.patch.dict(sys.modules, {"verify": verify_stub}):
        assert spec.loader is not None
        spec.loader.exec_module(module)
    return module


class CapabilityProbeTests(unittest.TestCase):
    def make_perl(self, root: Path, *, docx: str = "DOCX") -> tuple[Path, Path]:
        log = root / "calls.jsonl"
        perl = root / "perl5.38.2"
        perl.write_text(
            "#!" + sys.executable + "\n"
            + textwrap.dedent(
                f"""
                import json
                import os
                from pathlib import Path
                import sys

                with Path(os.environ["PROBE_LOG"]).open("a", encoding="utf-8") as handle:
                    handle.write(json.dumps({{
                        "argv": sys.argv[1:],
                        "env": {{name: os.environ.get(name) for name in (
                            "EXIFTOOL_HOME", "PERL5LIB", "PERLLIB", "PERL5OPT", "PATH"
                        )}},
                    }}) + "\\n")
                if "-ver" in sys.argv:
                    print("13.59")
                elif "-FileType" in sys.argv:
                    print('{{"FileType": "{docx}"}}')
                elif "-e" in sys.argv:
                    print("VERSION\\t13.59")
                    print("EVAL\\t19")
                else:
                    raise SystemExit(64)
                """
            ),
            encoding="utf-8",
        )
        perl.chmod(0o755)
        return perl, log

    def test_capability_probe_uses_one_explicit_oracle_and_ignores_ambient_config(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl, log = self.make_perl(root)
            library = root / "tree/lib"
            library.mkdir(parents=True)
            exiftool = root / "tree/exiftool"
            exiftool.write_bytes(perl.read_bytes())
            exiftool.chmod(0o755)
            carrier = root / "tree/t/images/OOXML.docx"
            carrier.parent.mkdir(parents=True)
            carrier.write_bytes(b"fixture")
            hostile = root / "hostile-bin"
            hostile.mkdir()
            (hostile / "exiftool").write_text("must not run\n", encoding="utf-8")

            environment = {
                "PROBE_LOG": str(log),
                "PATH": str(hostile),
                "EXIFTOOL_HOME": str(root / "hostile-home"),
                "PERL5LIB": str(root / "hostile-perl5lib"),
                "PERLLIB": str(root / "hostile-perllib"),
                "PERL5OPT": "-MHostile",
            }
            with mock.patch.dict(os.environ, environment, clear=False):
                module.capability_probe(exiftool, perl, library, "13.59", carrier)

            calls = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(len(calls), 3)
            oracle_prefix = [f"-I{library}", str(exiftool), "-config", ""]
            self.assertEqual(calls[0]["argv"][:4], oracle_prefix)
            self.assertEqual(calls[0]["argv"][4:], ["-ver"])
            self.assertEqual(calls[1]["argv"][:4], oracle_prefix)
            self.assertIn("-FileType", calls[1]["argv"])
            self.assertEqual(calls[2]["argv"][0], f"-I{library}")
            self.assertEqual(calls[2]["argv"][1], "-e")
            for call in calls:
                self.assertEqual(call["env"]["EXIFTOOL_HOME"], os.devnull)
                self.assertEqual(call["env"]["PERL5LIB"], "")
                self.assertEqual(call["env"]["PERLLIB"], "")
                self.assertEqual(call["env"]["PERL5OPT"], "")

    def test_capability_probe_refuses_missing_or_degraded_interpreter(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            library = root / "tree/lib"
            library.mkdir(parents=True)
            exiftool = root / "tree/exiftool"
            working_perl, working_log = self.make_perl(root)
            exiftool.write_bytes(working_perl.read_bytes())
            exiftool.chmod(0o755)
            carrier = root / "tree/t/images/OOXML.docx"
            carrier.parent.mkdir(parents=True)
            carrier.write_bytes(b"fixture")

            with mock.patch.dict(os.environ, {"PROBE_LOG": str(working_log)}, clear=False):
                try:
                    module.capability_probe(
                        exiftool, root / "missing-perl", library, "13.59", carrier
                    )
                except OSError as exc:
                    self.fail(f"missing interpreter escaped as an OS error: {exc}")
                except SystemExit as exc:
                    self.assertIn("capability probe failed", str(exc))
                else:
                    self.fail("missing interpreter was accepted")

            perl, log = self.make_perl(root, docx="ZIP")
            with mock.patch.dict(os.environ, {"PROBE_LOG": str(log)}, clear=False):
                with self.assertRaisesRegex(SystemExit, "capability probe failed"):
                    module.capability_probe(exiftool, perl, library, "13.59", carrier)


if __name__ == "__main__":
    unittest.main()
