"""Light synthetic controls for the candidate producer; never builds Perl."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import stat
import sys
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
    def test_source_member_limit_refuses_before_eager_enumeration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            source.mkdir()
            for name in ("c", "a", "b"):
                (source / name).write_bytes(b"x")
            with mock.patch.object(freeze, "MAX_MEMBERS", 2), mock.patch.object(
                Path, "rglob", side_effect=AssertionError("eager tree enumeration")
            ):
                with self.assertRaisesRegex(freeze.Refused, "bounded archive limits"):
                    freeze._members(source)

    def test_source_members_remain_sorted_without_following_symlinked_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            (source / "b").mkdir(parents=True)
            (source / "b/z").write_bytes(b"z")
            (source / "a").mkdir()
            (source / "a/link").symlink_to("../b")
            self.assertEqual([name for _, name, _ in freeze._members(source)],
                             ["a", "a/link", "b", "b/z"])

    def test_hostile_perl_environment_is_removed_before_oracle_load(self) -> None:
        hostile = {key: "hostile" for key in freeze.PERL_ENV_KEYS}
        with mock.patch.dict(os.environ, {**hostile, "PRODUCER_KEEP": "yes"}):
            with mock.patch.object(freeze, "_load_oracle", wraps=freeze._load_oracle) as load:
                oracle, removed = freeze._load_clean_oracle()
                self.assertEqual(load.call_count, 1)
                self.assertTrue(all(key not in os.environ for key in freeze.PERL_ENV_KEYS))
                self.assertEqual(removed, sorted(hostile))
            self.assertEqual(os.environ["PRODUCER_KEEP"], "yes")
            command = [sys.executable, "-c", "import json,os; print(json.dumps(dict(os.environ)))"]
            # bootstrap.run without env is the Configure/make/install path.
            built = json.loads(oracle.run(command))
            self.assertTrue(all(key not in built for key in hostile))
            with tempfile.TemporaryDirectory() as directory:
                staged = oracle.staged_perl_environment(Path(directory))
                self.assertTrue(all(key not in staged for key in hostile if key != "PERL5LIB"))
                self.assertNotIn("hostile", staged["PERL5LIB"])
            self.assertEqual(built["PRODUCER_KEEP"], "yes")

    def test_producer_probe_excludes_late_hostile_perl_environment(self) -> None:
        hostile = {key: "hostile" for key in freeze.PERL_ENV_KEYS}
        with mock.patch.dict(os.environ, {**hostile, "PRODUCER_KEEP": "yes"}):
            command = [sys.executable, "-c", "import json,os; print(json.dumps(dict(os.environ)))"]
            observed = json.loads(freeze._command(command))
            self.assertTrue(all(key not in observed for key in hostile))
            self.assertEqual(observed["PRODUCER_KEEP"], "yes")

    def test_uploader_export_only_opens_two_files_outside_private_ops(self) -> None:
        def other_uid_can_read(path: Path, mount: Path) -> bool:
            current = path.parent
            while current != mount.parent:
                if not current.stat().st_mode & stat.S_IXOTH:
                    return False
                current = current.parent
            return bool(path.stat().st_mode & stat.S_IROTH)

        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.mkdir(mode=0o755)
            ops = target / "ops"
            ops.mkdir(mode=0o700)
            private = ops / "evidence"
            private.mkdir(mode=0o700)
            archive = private / "perl.tar.gz"
            receipt = private / "candidate-receipt.json"
            archive.write_bytes(b"candidate bytes")
            receipt.write_bytes(b'{"status":"candidate"}')
            self.assertFalse(other_uid_can_read(archive, target))
            export = target / "perl-candidate-export"
            freeze.publish_uploader_export(archive, receipt, export)
            self.assertEqual(sorted(p.name for p in export.iterdir()),
                             ["candidate-receipt.json", "perl-5.38.2-prefix.tar.gz"])
            self.assertEqual((export / "perl-5.38.2-prefix.tar.gz").read_bytes(), archive.read_bytes())
            self.assertEqual((export / "candidate-receipt.json").read_bytes(), receipt.read_bytes())
            for path in export.iterdir():
                self.assertTrue(other_uid_can_read(path, target))
            self.assertEqual(stat.S_IMODE(ops.stat().st_mode), 0o700)
            with self.assertRaises(freeze.Refused):
                freeze.publish_uploader_export(archive, receipt, export)

    def test_freshness_refuses_existing_root_before_import_or_download(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(freeze, "OPS", root), mock.patch.object(
                freeze, "_load_oracle", side_effect=AssertionError("oracle imported")
            ), mock.patch.object(freeze.platform, "system", return_value="Linux"), mock.patch.object(
                freeze.platform, "machine", return_value="x86_64"
            ), mock.patch.object(freeze.Path, "cwd", return_value=Path("/target/checkout")):
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

    def test_chained_symlink_escape_refused_in_source_and_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source"
            (source / "d").mkdir(parents=True)
            (source / "x").mkdir()
            (base / "outside").write_bytes(b"external bytes")
            (source / "d/a").symlink_to("../x")
            (source / "d/link").symlink_to("a/../../outside")
            self.assertEqual((source / "d/link").resolve(), (base / "outside").resolve())
            with self.assertRaises(freeze.Refused):
                freeze.freeze_tree(source, base / "candidate.tar.gz")
            self.assertFalse((base / "candidate.tar.gz").exists())

            archive = base / "untrusted.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                for name in ("d", "x"):
                    item = tarfile.TarInfo(name)
                    item.type = tarfile.DIRTYPE
                    tar.addfile(item)
                for name, link in (("d/a", "../x"), ("d/link", "a/../../outside")):
                    item = tarfile.TarInfo(name)
                    item.type = tarfile.SYMTYPE
                    item.linkname = link
                    tar.addfile(item)
            with self.assertRaises(freeze.Refused):
                freeze.extract_frozen(archive, base / "replay")
            self.assertFalse((base / "replay").exists())

    def test_dangling_and_cycle_links_refused(self) -> None:
        for links in ((('a', 'missing'),), (('a', 'b'), ('b', 'a'))):
            with self.subTest(links=links), tempfile.TemporaryDirectory() as directory:
                archive = Path(directory) / "bad.tar.gz"
                with tarfile.open(archive, "w:gz") as tar:
                    for name, link in links:
                        item = tarfile.TarInfo(name)
                        item.type = tarfile.SYMTYPE
                        item.linkname = link
                        tar.addfile(item)
                with self.assertRaises(freeze.Refused):
                    freeze.inspect_archive(archive)

    def test_member_limit_refuses_during_iteration_without_getmembers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "many.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                for name in ("a", "b", "c"):
                    item = tarfile.TarInfo(name)
                    item.size = 1
                    tar.addfile(item, io.BytesIO(b"x"))
            with mock.patch.object(freeze, "MAX_MEMBERS", 2), mock.patch.object(
                tarfile.TarFile, "getmembers", side_effect=AssertionError("eager member load")
            ):
                with self.assertRaisesRegex(freeze.Refused, "member count"):
                    freeze.inspect_archive(archive)

    def test_oversized_pax_header_refused_before_tarfile_parser(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "pax.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                item = tarfile.TarInfo("pax")
                item.type = tarfile.XHDTYPE
                item.size = 32
                tar.addfile(item, io.BytesIO(b"x" * 32))
                regular = tarfile.TarInfo("ordinary")
                regular.size = 1
                tar.addfile(regular, io.BytesIO(b"y"))
            with mock.patch.object(freeze, "MAX_PAX_HEADER_BYTES", 16), mock.patch.object(
                tarfile, "open", side_effect=AssertionError("tarfile parsed metadata")
            ):
                with self.assertRaisesRegex(freeze.Refused, "PAX"):
                    freeze.inspect_archive(archive)


if __name__ == "__main__":
    unittest.main()
