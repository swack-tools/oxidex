import io
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import merge_dump_shards as mds  # noqa: E402

DUMP = Path(__file__).resolve().with_name("dump_tables.pl")
PERL = os.environ.get("EXIFTOOL_PERL", "perl")


def configured_library() -> Path | None:
    configured = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
    if not configured:
        return None
    source = Path(configured)
    return source / "lib" if (source / "lib").is_dir() else source


def executable(command: str) -> bool:
    return Path(command).is_file() if "/" in command else shutil.which(command) is not None


LIBRARY = configured_library()
NATIVE_READY = executable(PERL) and LIBRARY is not None and LIBRARY.is_dir()
ENV = {k: v for k, v in os.environ.items() if k not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}}


def frames(index: int, count: int, records: list[tuple[str, int, bytes]], total: int) -> bytes:
    out = f"H {index} {count}\n".encode()
    for kind, seq, body in records:
        out += f"{kind} {seq} {len(body)}\n".encode() + body
    return out + f"E {total}\n".encode()


class MergeProtocolTests(unittest.TestCase):
    def merge(self, *shards: bytes) -> bytes:
        with tempfile.TemporaryDirectory() as tmp:
            paths = []
            for i, data in enumerate(shards):
                path = Path(tmp) / f"s{i}"
                path.write_bytes(data)
                paths.append(path)
            out = io.BytesIO()
            mds.merge(out, paths)
            return out.getvalue()

    def test_interleaves_leaves_between_shared_skeleton(self):
        a = frames(0, 2, [("S", 0, b"{"), ("L", 1, b"a"), ("S", 2, b",")], 4)
        b = frames(1, 2, [("S", 0, b"{"), ("S", 2, b","), ("L", 3, b"b}")], 4)
        self.assertEqual(self.merge(b, a), b"{a,b}")

    def test_refuses_disagreeing_global_skeleton(self):
        a = frames(0, 2, [("S", 0, b"{x"), ("L", 1, b"a")], 2)
        b = frames(1, 2, [("S", 0, b"{y")], 2)
        with self.assertRaisesRegex(mds.ShardError, "differs"):
            self.merge(a, b)

    def test_refuses_missing_duplicate_or_absent_shards(self):
        a = frames(0, 2, [("S", 0, b"{"), ("L", 1, b"a")], 2)
        dup = frames(1, 2, [("S", 0, b"{"), ("L", 1, b"a")], 2)
        missing = frames(1, 2, [("S", 0, b"{")], 3)
        short = frames(1, 2, [("S", 0, b"{")], 1)
        with self.assertRaisesRegex(mds.ShardError, "2 shards"):
            self.merge(a, dup)
        with self.assertRaisesRegex(mds.ShardError, "disagree on record count"):
            self.merge(a, missing)
        with self.assertRaisesRegex(mds.ShardError, "disagree on record count"):
            self.merge(a, short)
        with self.assertRaisesRegex(mds.ShardError, "exactly shards"):
            self.merge(a)

    def test_unions_partial_maps(self):
        def part(level, entries):
            body = f"{level}\n".encode()
            for key, value in entries:
                text = f'"{key}"'.encode()
                body += f"{len(key)} {len(text)} {len(value)}\n".encode() + key.encode() + text + value
            return body
        a = frames(0, 2, [("S", 0, b"x : "), ("U", 1, part(1, [("b", b"2"), ("a", b"1")]))], 2)
        b = frames(1, 2, [("S", 0, b"x : "), ("U", 1, part(1, [("b", b"2")]))], 2)
        self.assertEqual(self.merge(a, b), b'x : {\n      "a" : 1,\n      "b" : 2\n   }')
        empty = frames(1, 2, [("S", 0, b"x : "), ("U", 1, part(1, []))], 2)
        none = frames(0, 2, [("S", 0, b"x : "), ("U", 1, part(1, []))], 2)
        self.assertEqual(self.merge(none, empty), b"x : {}")
        clash = frames(1, 2, [("S", 0, b"x : "), ("U", 1, part(1, [("b", b"3")]))], 2)
        with self.assertRaisesRegex(mds.ShardError, "differs"):
            self.merge(a, clash)

    def test_refuses_truncated_shard(self):
        a = frames(0, 1, [("S", 0, b"{}")], 1)
        with self.assertRaisesRegex(mds.ShardError, "truncated"):
            self.merge(a[:-4])


@unittest.skipUnless(NATIVE_READY, "configured pinned ExifTool is unavailable")
class ShardedEqualsMonolithicTests(unittest.TestCase):
    MODULES = ("Exif", "QuickTime", "XMP")

    def assert_identical(self, *options: str) -> None:
        args = [*options, str(LIBRARY), *self.MODULES]
        mono = subprocess.run([PERL, str(DUMP), *args], check=True, capture_output=True,
                              env=ENV, timeout=600).stdout
        self.assertGreater(len(mono), 100_000)
        with tempfile.TemporaryDirectory() as tmp:
            for shards in (1, 4):
                out = Path(tmp) / f"merged-{shards}.json"
                mds.run(shards, out, args, PERL)
                self.assertEqual(out.read_bytes(), mono, f"{shards} shards, {options}")

    def test_reader_only_hydrated_layouts(self):
        self.assert_identical(
            "--reader-only", "--hydrated-layouts",
            "--hydrated-layout-table", "Image::ExifTool::QuickTime::ItemList",
            "--hydrated-layout-table", "Image::ExifTool::JFIF::Main",
            "--hydrated-layout-table", "Image::ExifTool::Composite")

    def test_full_writer_document(self):
        self.assert_identical()


if __name__ == "__main__":
    unittest.main()
