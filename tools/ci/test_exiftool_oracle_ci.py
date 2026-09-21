"""CI portability tests for the pinned ExifTool oracle resolver."""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[2] / "scripts"
sys.path.insert(0, str(SCRIPTS))
import exiftool_oracle as oracle  # noqa: E402


@contextmanager
def isolated_roots():
    """Give path-fence tests explicit durable and external sibling roots."""
    parent = Path(__file__).resolve().parents[2].parent / \
        "oxidex-beta1-targets/test-root-isolation/test-roots"
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=parent, prefix="case-") as case:
        root = Path(case)
        durable = root / "durable"
        external = root / "external"
        durable.mkdir()
        external.mkdir()
        yield durable, external


class TablePerlResolutionTests(unittest.TestCase):
    def test_action_exports_table_channel_and_verifier_consumes_it(self):
        root = Path(__file__).resolve().parents[2]
        action = (root / ".github/actions/pinned-exiftool/action.yml").read_text(
            encoding="utf-8"
        )
        verifier = (root / "tools/exiftool-tables/verify.py").read_text(
            encoding="utf-8"
        )
        self.assertIn('OXIDEX_TABLES_PERL=$(command -v perl)', action)
        self.assertIn("exiftool_oracle.choose_table_perl()", verifier)

    def test_table_channel_accepts_absolute_capability_checked_interpreter(self):
        with tempfile.TemporaryDirectory() as directory:
            perl = Path(directory) / "perl"
            perl.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            perl.chmod(0o755)
            with mock.patch.dict(
                os.environ,
                {"OXIDEX_TABLES_PERL": str(perl), "EXIFTOOL_PERL": "perl"},
                clear=False,
            ), mock.patch.object(oracle, "_runs", return_value=True), mock.patch.object(
                oracle, "missing_modules", return_value=[]
            ):
                self.assertEqual(oracle.choose_table_perl(), str(perl.resolve()))

    def test_table_channel_refuses_interpreter_missing_required_modules(self):
        with tempfile.TemporaryDirectory() as directory:
            perl = Path(directory) / "perl"
            perl.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            perl.chmod(0o755)
            with mock.patch.dict(
                os.environ,
                {"OXIDEX_TABLES_PERL": str(perl)},
                clear=False,
            ), mock.patch.object(oracle, "_runs", return_value=True), mock.patch.object(
                oracle, "missing_modules", return_value=["Archive::Zip"]
            ):
                self.assertIsNone(oracle.choose_table_perl())

    def test_table_channel_refuses_relative_interpreter(self):
        with mock.patch.dict(
            os.environ, {"OXIDEX_TABLES_PERL": "perl"}, clear=False
        ):
            self.assertIsNone(oracle.choose_table_perl())

    def test_invalid_table_channel_does_not_fall_back_to_durable_perl(self):
        with mock.patch.dict(
            os.environ,
            {"OXIDEX_TABLES_PERL": "/definitely/missing/table-perl"},
            clear=False,
        ), mock.patch.object(
            oracle, "choose_perl", side_effect=AssertionError("must not fall back")
        ):
            self.assertIsNone(oracle.choose_table_perl())

    def test_table_channel_refuses_non_executable_and_broken_interpreters(self):
        with tempfile.TemporaryDirectory() as directory:
            perl = Path(directory) / "perl"
            perl.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
            with mock.patch.dict(
                os.environ, {"OXIDEX_TABLES_PERL": str(perl)}, clear=False
            ):
                self.assertIsNone(oracle.choose_table_perl())
            perl.chmod(0o755)
            with mock.patch.dict(
                os.environ, {"OXIDEX_TABLES_PERL": str(perl)}, clear=False
            ), mock.patch.object(oracle, "_runs", return_value=False):
                self.assertIsNone(oracle.choose_table_perl())

    def test_absent_table_channel_delegates_to_strict_release_selection(self):
        environment = dict(os.environ)
        environment.pop("OXIDEX_TABLES_PERL", None)
        with mock.patch.dict(os.environ, environment, clear=True), mock.patch.object(
            oracle, "choose_perl", return_value="/durable/perl"
        ) as strict:
            self.assertEqual(oracle.choose_table_perl(), "/durable/perl")
            strict.assert_called_once_with()

    def test_non_ci_override_remains_fenced_to_durable_root(self):
        with isolated_roots() as (durable, external):
            perl = external / "perl"
            perl.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            perl.chmod(0o755)
            with mock.patch.dict(
                os.environ,
                {
                    "GITHUB_ACTIONS": "true",
                    "EXIFTOOL_PERL": str(perl),
                    "OXIDEX_TABLES_PERL": str(perl),
                },
                clear=False,
            ), mock.patch.object(oracle, "DURABLE_ROOT", durable), self.assertRaisesRegex(
                oracle.OracleError, "durable root"
            ):
                oracle.choose_perl()


if __name__ == "__main__":
    unittest.main()
