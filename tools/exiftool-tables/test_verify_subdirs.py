"""Focused subprocess-contract tests for the SubDirectory native oracle."""

from __future__ import annotations

import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import textwrap
import types
import unittest
from contextlib import redirect_stdout
from unittest import mock


MODULE_PATH = Path(__file__).with_name("verify_subdirs.py")
VERIFY_PATH = Path(__file__).with_name("verify.py")


def load_module():
    """Load the helper without importing verify.py's process-global Perl."""
    verify_stub = types.ModuleType("verify")
    spec = importlib.util.spec_from_file_location("verify_subdirs_under_test", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    with mock.patch.dict(sys.modules, {"verify": verify_stub}):
        assert spec.loader is not None
        spec.loader.exec_module(module)
    return module


def load_real_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
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
                            "EXIFTOOL_HOME", "OXIDEX_TABLES_PERL", "PERL5LIB",
                            "PERLLIB", "PERL5OPT", "PATH"
                        )}},
                    }}) + "\\n")
                args = sys.argv[1:]
                if "-ver" in args:
                    print("13.59")
                elif "-FileType" in args:
                    print('{{"FileType": "{docx}"}}')
                elif any(arg == "-MArchive::Zip" for arg in args):
                    pass
                elif "-e" in args:
                    source = args[args.index("-e") + 1]
                    if source == "1":
                        pass
                    elif "eval($expr)" in source:
                        print("VERSION\\t13.59")
                        print("EVAL\\t19")
                    elif "$Image::ExifTool::VERSION" in source:
                        print("13.59", end="")
                    else:
                        raise SystemExit(65)
                elif any(arg.endswith("oracle.pl") for arg in args):
                    print("Module\\tTable\\t1\\tSUBDIR\\t-\\t0\\t\\t0\\t0\\t0")
                elif any(arg.endswith(".pl") for arg in args):
                    print("J0\\t19")
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
                output = io.StringIO()
                with redirect_stdout(output):
                    module.capability_probe(exiftool, perl, library, "13.59", carrier)

            diagnostic = output.getvalue()
            self.assertIn("exiftool-module-version='13.59'", diagnostic)
            self.assertNotIn("perl-version=", diagnostic)

            calls = [
                json.loads(line)
                for line in log.read_text(encoding="utf-8").splitlines()
            ]
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
                self.assertEqual(call["env"]["OXIDEX_TABLES_PERL"], str(perl))
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

    def test_real_verify_native_seams_use_selected_perl_and_library(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl, log = self.make_perl(root)
            library = root / "tree/lib"
            library.mkdir(parents=True)
            oracle = root / "oracle.pl"
            oracle.write_text("fixture\n", encoding="utf-8")
            environment = {
                "PROBE_LOG": str(log),
                "OXIDEX_TABLES_PERL": str(perl),
                "EXIFTOOL_HOME": os.devnull,
                "PERL5LIB": "",
                "PERLLIB": "",
                "PERL5OPT": "",
            }
            tool_dir = str(MODULE_PATH.parent)
            sys.path.insert(0, tool_dir)
            try:
                with mock.patch.dict(os.environ, environment, clear=False):
                    verify_module = load_real_module(
                        VERIFY_PATH, "verify_native_seams_under_test"
                    )
                    self.assertEqual(verify_module.oracle_version(library), "13.59")
                    verify_module.run_oracle(library, oracle)
            finally:
                sys.path.remove(tool_dir)

            calls = [
                json.loads(line)
                for line in log.read_text(encoding="utf-8").splitlines()
            ]
            version = next(
                call
                for call in calls
                if call["argv"][:1] == [f"-I{library}"]
                and "-e" in call["argv"]
                and "require Image::ExifTool" in call["argv"][-1]
            )
            census = next(
                call for call in calls if call["argv"] == [str(oracle), str(library)]
            )
            for call in (version, census):
                self.assertEqual(call["env"]["EXIFTOOL_HOME"], os.devnull)
                self.assertEqual(call["env"]["OXIDEX_TABLES_PERL"], str(perl))
                self.assertEqual(call["env"]["PERL5LIB"], "")
                self.assertEqual(call["env"]["PERLLIB"], "")
                self.assertEqual(call["env"]["PERL5OPT"], "")

    def test_main_explicit_perl_controls_version_census_and_evaluation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            healthy, log = self.make_perl(root)
            hostile_marker = root / "hostile-ran"
            hostile = root / "hostile-perl"
            hostile.write_text(
                "#!/bin/sh\nprintf ran > \"$HOSTILE_MARKER\"\nexit 91\n",
                encoding="utf-8",
            )
            hostile.chmod(0o755)
            library = root / "tree/lib"
            library.mkdir(parents=True)
            generated = root / "generated.rs"
            generated.write_text("fixture\n", encoding="utf-8")
            exiftool = root / "tree/exiftool"
            exiftool.write_text("fixture\n", encoding="utf-8")
            carrier = root / "tree/t/images/OOXML.docx"
            carrier.parent.mkdir(parents=True)
            carrier.write_bytes(b"fixture")
            environment = {
                "PROBE_LOG": str(log),
                "OXIDEX_TABLES_PERL": str(hostile),
                "HOSTILE_MARKER": str(hostile_marker),
                "EXIFTOOL_HOME": str(root / "hostile-home"),
                "PERL5LIB": str(root / "hostile-perl5lib"),
                "PERLLIB": str(root / "hostile-perllib"),
                "PERL5OPT": "-MHostile",
            }
            tool_dir = str(MODULE_PATH.parent)
            previous_verify = sys.modules.pop("verify", None)
            sys.path.insert(0, tool_dir)
            try:
                with mock.patch.dict(os.environ, environment, clear=False):
                    try:
                        module = load_real_module(
                            MODULE_PATH, "verify_subdirs_main_under_test"
                        )
                    except SystemExit as exc:
                        self.fail(f"ambient resolver preempted explicit --perl: {exc}")
                    argv = [
                        "verify_subdirs.py",
                        str(generated),
                        str(library),
                        "--exiftool",
                        str(exiftool),
                        "--perl",
                        str(healthy),
                        "--probe-file",
                        str(carrier),
                    ]
                    with (
                        mock.patch.object(sys, "argv", argv),
                        mock.patch.object(module.instrument, "git_state", return_value={}),
                        mock.patch.object(
                            module.instrument, "refuse_if_dirty", return_value=False
                        ),
                        mock.patch.object(module.instrument, "print_header"),
                        mock.patch.object(module, "census", side_effect=lambda *args: []),
                        mock.patch.object(module, "run_rust", return_value={}),
                    ):
                        module.main()
            finally:
                sys.path.remove(tool_dir)
                sys.modules.pop("verify", None)
                if previous_verify is not None:
                    sys.modules["verify"] = previous_verify

            self.assertFalse(hostile_marker.exists(), "ambient Perl was executed")
            calls = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
            for call in calls:
                self.assertEqual(call["env"]["EXIFTOOL_HOME"], os.devnull)
                self.assertEqual(call["env"]["OXIDEX_TABLES_PERL"], str(healthy))
                self.assertEqual(call["env"]["PERL5LIB"], "")
                self.assertEqual(call["env"]["PERLLIB"], "")
                self.assertEqual(call["env"]["PERL5OPT"], "")
            native = [
                call
                for call in calls
                if "-ver" in call["argv"]
                or "-FileType" in call["argv"]
                or any(arg.endswith("oracle.pl") for arg in call["argv"])
                or any(
                    arg.endswith(".pl") and not arg.endswith("oracle.pl")
                    for arg in call["argv"]
                )
                or (
                    "-e" in call["argv"]
                    and call["argv"][-1] != "1"
                    and not any(arg == "-MArchive::Zip" for arg in call["argv"])
                )
            ]
            self.assertGreaterEqual(len(native), 5)
            self.assertTrue(any("-ver" in call["argv"] for call in native))
            self.assertTrue(any("-FileType" in call["argv"] for call in native))
            self.assertTrue(
                any(
                    any(arg.endswith("oracle.pl") for arg in call["argv"])
                    for call in native
                )
            )
            self.assertTrue(
                any(
                    any(
                        arg.endswith(".pl") and not arg.endswith("oracle.pl")
                        for arg in call["argv"]
                    )
                    for call in native
                )
            )


if __name__ == "__main__":
    unittest.main()
