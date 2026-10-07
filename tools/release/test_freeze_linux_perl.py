"""Light synthetic controls for the candidate producer; never builds Perl."""

from __future__ import annotations

import importlib.util
import io
import os
import stat
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


MODULE = Path(__file__).with_name("freeze_linux_perl.py")
spec = importlib.util.spec_from_file_location("freeze_linux_perl", MODULE)
assert spec and spec.loader
freeze = importlib.util.module_from_spec(spec)
spec.loader.exec_module(freeze)


class FreezePerlTests(unittest.TestCase):
    def test_freshness_refuses_existing_root_before_import_or_download(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(freeze, "OPS", root), mock.patch.object(
                freeze, "_load_oracle", side_effect=AssertionError("oracle imported")
            ), mock.patch.object(freeze.platform, "system", return_value="Linux"), mock.patch.object(
                freeze.platform, "machine", return_value="x86_64"
            ), mock.patch.object(freeze.Path, "cwd", return_value=Path("/src")):
                with self.assertRaisesRegex(freeze.Refused, "fresh owned"):
                    freeze.worker("a" * 40, "b" * 40)

    def test_complete_roundtrip_preserves_tree_contract_and_refuses_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source"
            (source / "bin").mkdir(parents=True)
            executable = source / "bin/perl5.38.2"
            executable.write_bytes(b"synthetic perl\n")
            executable.chmod(0o755)
            (source / "lib").mkdir()
            module = source / "lib/Zip.pm"
            module.write_bytes(b"synthetic zip\n")
            module.chmod(0o644)
            (source / "bin/perl").symlink_to("perl5.38.2")
            archive = base / "perl.tar.gz"
            freeze.freeze_tree(source, archive)
            self.assertEqual(len(freeze.inspect_archive(archive)), 5)
            from hashlib import sha256

            before = sha256(archive.read_bytes()).hexdigest()
            extracted = base / "extracted"
            freeze.extract_frozen(archive, extracted)
            self.assertEqual(freeze._load_oracle().sha256_tree(source),
                             freeze._load_oracle().sha256_tree(extracted))
            self.assertEqual((extracted / "bin/perl").readlink(), Path("perl5.38.2"))
            self.assertEqual(stat.S_IMODE((extracted / "bin/perl5.38.2").stat().st_mode), 0o755)
            with self.assertRaisesRegex(freeze.Refused, "already exists"):
                freeze.freeze_tree(source, archive)
            module.write_bytes(b"mutated zip\n")
            self.assertNotEqual(freeze._load_oracle().sha256_tree(source),
                                freeze._load_oracle().sha256_tree(extracted))
            self.assertEqual(sha256(archive.read_bytes()).hexdigest(), before)

    def test_refuses_unsafe_entries_before_extraction(self) -> None:
        cases = [
            [("../escape", tarfile.REGTYPE, "")],
            [("bin/perl", tarfile.SYMTYPE, "../../escape")],
            [("bin/perl", tarfile.REGTYPE, ""), ("bin/perl", tarfile.REGTYPE, "")],
            [("device", tarfile.CHRTYPE, "")],
            [("bin", tarfile.SYMTYPE, "lib"), ("bin/perl", tarfile.REGTYPE, "")],
        ]
        for entries in cases:
            with self.subTest(entries=entries), tempfile.TemporaryDirectory() as directory:
                archive = Path(directory) / "bad.tar.gz"
                with tarfile.open(archive, "w:gz") as tar:
                    for name, kind, link in entries:
                        item = tarfile.TarInfo(name)
                        item.type = kind
                        item.linkname = link
                        if kind == tarfile.REGTYPE:
                            item.size = 1
                            tar.addfile(item, io.BytesIO(b"x"))
                        else:
                            tar.addfile(item)
                destination = Path(directory) / "out"
                with self.assertRaises(freeze.Refused):
                    freeze.extract_frozen(archive, destination)
                self.assertFalse(destination.exists())

    def test_refuses_special_source_before_creating_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source"
            source.mkdir()
            os.mkfifo(source / "pipe")
            archive = base / "candidate.tar.gz"
            with self.assertRaisesRegex(freeze.Refused, "unsupported"):
                freeze.freeze_tree(source, archive)
            self.assertFalse(archive.exists())


if __name__ == "__main__":
    unittest.main()
