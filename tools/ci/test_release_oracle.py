"""Tests for the explicit, fail-closed release ExifTool oracle probe."""

from __future__ import annotations

import os
import pathlib
import tempfile
import unittest

from tools.ci import release_oracle


class ReleaseOracleTests(unittest.TestCase):
    def make_fixture(self, root: pathlib.Path, *, version: str = "13.59", docx: str = "DOCX"):
        repo = root / "repo"
        tree = root / "tree"
        repo.mkdir()
        (repo / ".exiftool-version").write_text("13.59\n", encoding="utf-8")
        (tree / "lib").mkdir(parents=True)
        (tree / "lib/strict.pm").write_text("fixture\n", encoding="utf-8")
        (tree / "t/images").mkdir(parents=True)
        (tree / "exiftool").write_text("fixture\n", encoding="utf-8")
        (tree / "t/images/OOXML.docx").write_bytes(b"fixture")
        perl = root / "perl5.38.2"
        perl.write_text(
            "#!/bin/sh\n"
            "case \"$*\" in\n"
            "  *'print $^V'*) printf 'v5.38.2' ;;\n"
            "  *'-Mstrict -Mwarnings -MArchive::Zip -MCompress::Zlib'*) exit 0 ;;\n"
            f"  *'-ver'*) printf '{version}\\n' ;;\n"
            f"  *'-FileType'*) printf '{docx}\\n' ;;\n"
            "  *) exit 64 ;;\n"
            "esac\n",
            encoding="utf-8",
        )
        perl.chmod(0o755)
        return repo, perl, tree

    def test_probe_requires_explicit_existing_perl_and_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            repo = root / "repo"
            repo.mkdir()
            (repo / ".exiftool-version").write_text("13.59\n", encoding="utf-8")
            with self.assertRaisesRegex(release_oracle.OracleProbeError, "perl_path"):
                release_oracle.probe_oracle(repo, root / "missing-perl", root / "missing-tree")

    def test_probe_records_full_capability_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo, perl, tree = self.make_fixture(pathlib.Path(tmp))
            receipt = release_oracle.probe_oracle(repo, perl, tree)
            self.assertEqual(receipt["status"], "verified")
            self.assertEqual(receipt["perl_version"], "v5.38.2")
            self.assertEqual(receipt["exiftool_version"], "13.59")
            self.assertEqual(receipt["docx_file_type"], "DOCX")
            self.assertGreater(receipt["library_file_count"], 0)
            self.assertRegex(receipt["library_fingerprint_sha256"], r"^[0-9a-f]{64}$")
            self.assertEqual(
                pathlib.Path(receipt["library_fingerprint_path"]), (tree / "lib").resolve()
            )
            self.assertEqual(pathlib.Path(receipt["perl_path"]), perl.resolve())
            self.assertEqual(pathlib.Path(receipt["tree_path"]), tree.resolve())

    def test_probe_rejects_version_skew_and_degraded_docx(self):
        for version, docx, message in (
            ("13.58", "DOCX", "ExifTool version"),
            ("13.59", "ZIP", "DOCX capability"),
        ):
            with self.subTest(version=version, docx=docx), tempfile.TemporaryDirectory() as tmp:
                repo, perl, tree = self.make_fixture(
                    pathlib.Path(tmp), version=version, docx=docx
                )
                with self.assertRaisesRegex(release_oracle.OracleProbeError, message):
                    release_oracle.probe_oracle(repo, perl, tree)

    def test_defaults_are_durable_and_versioned_not_path_lookup(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = pathlib.Path(tmp)
            (repo / ".exiftool-version").write_text("13.59\n", encoding="utf-8")
            old = os.environ.copy()
            try:
                os.environ.pop("EXIFTOOL_PERL", None)
                os.environ.pop("EXIFTOOL_CACHE_DIR", None)
                perl, tree = release_oracle.default_paths(repo)
            finally:
                os.environ.clear()
                os.environ.update(old)
            self.assertEqual(
                perl,
                pathlib.Path("/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2"),
            )
            self.assertEqual(
                tree,
                pathlib.Path("/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool"),
            )


if __name__ == "__main__":
    unittest.main()
