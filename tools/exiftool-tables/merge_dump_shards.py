#!/usr/bin/env python3
"""Run dump_tables.pl as N encoding shards and merge them byte-identically.

Each ``dump_tables.pl --shard I/N`` process performs the complete capture and
encodes only its share of the leaves (see ``OxiDex/ShardedJson.pm``).  The
merge interleaves records by sequence number and refuses unless:

* exactly shards ``0..N-1`` are present, all agreeing on N and record count;
* every skeleton (``S``) record -- which carries every global fact outside the
  partitioned leaves -- is byte-identical in every shard;
* every leaf (``L``) record is supplied by exactly one shard;
* every union (``U``) record -- a map each shard computed only part of -- is
  supplied by every shard, and equal keys carry byte-identical values.

Records are streamed: only the current record of each shard is in memory.

Usage::

    merge_dump_shards.py merge OUT SHARD...
    merge_dump_shards.py run --shards N --out OUT [--perl PERL] -- DUMP_ARGS...

``run`` launches the N perl processes in parallel (``EXIFTOOL_PERL`` or
``perl``, with PERL5LIB/PERLLIB/PERL5OPT scrubbed like the tests do), writes
shard files beside OUT, merges, and removes them.
"""

from __future__ import annotations

import argparse
import os
import resource
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import BinaryIO

SHARD = Path(__file__).resolve().with_name("dump_tables_shard.pl")


class ShardError(RuntimeError):
    pass


class _Reader:
    def __init__(self, path: Path):
        self.path = path
        self.fh: BinaryIO = open(path, "rb")
        header = self.fh.readline().split()
        if len(header) != 3 or header[0] != b"H":
            raise ShardError(f"{path}: missing shard header")
        self.index, self.count = (int(x) for x in header[1:])
        self.records: int | None = None
        self.last = -1
        self.pending: tuple[bytes, int, bytes] | None = None
        self._advance()

    def _advance(self) -> None:
        line = self.fh.readline()
        parts = line.split()
        if not parts:
            raise ShardError(f"{self.path}: truncated (no end record)")
        if parts[0] == b"E":
            if len(parts) != 2 or int(parts[1]) <= self.last:
                raise ShardError(f"{self.path}: malformed end record")
            self.records = int(parts[1])
            if self.fh.read(1):
                raise ShardError(f"{self.path}: bytes after end record")
            self.pending = None
            return
        if len(parts) != 3 or parts[0] not in (b"S", b"L", b"U"):
            raise ShardError(f"{self.path}: malformed record header {line[:80]!r}")
        seq, size = int(parts[1]), int(parts[2])
        body = self.fh.read(size)
        if len(body) != size:
            raise ShardError(f"{self.path}: truncated record {seq}")
        if seq <= self.last:
            raise ShardError(f"{self.path}: records out of order at {seq}")
        self.last = seq
        self.pending = (parts[0], seq, body)

    def take(self) -> bytes:
        assert self.pending is not None
        body = self.pending[2]
        self._advance()
        return body


INDENT = b"   "


def _union(seq: int, bodies: list[bytes]) -> bytes:
    level = None
    entries: dict[bytes, tuple[bytes, bytes]] = {}
    for body in bodies:
        head, _, rest = body.partition(b"\n")
        if level is None:
            level = int(head)
        elif int(head) != level:
            raise ShardError(f"union record {seq} disagrees on depth")
        pos = 0
        while pos < len(rest):
            end = rest.index(b"\n", pos)
            klen, tlen, vlen = (int(x) for x in rest[pos:end].split())
            pos = end + 1
            key = rest[pos:pos + klen]
            text = rest[pos + klen:pos + klen + tlen]
            value = rest[pos + klen + tlen:pos + klen + tlen + vlen]
            pos += klen + tlen + vlen
            if pos > len(rest):
                raise ShardError(f"union record {seq} is truncated")
            if entries.setdefault(key, (text, value)) != (text, value):
                raise ShardError(f"union record {seq} key {key!r} differs between shards")
    if not entries:
        return b"{}"
    inner = INDENT * (level + 1)
    # UTF-8 byte order is code point order, which is Perl's `cmp` order.
    items = [inner + entries[k][0] + b" : " + entries[k][1] for k in sorted(entries)]
    return b"{\n" + b",\n".join(items) + b"\n" + INDENT * level + b"}"


def merge(out: BinaryIO, shard_paths: list[Path]) -> None:
    readers = [_Reader(p) for p in shard_paths]
    try:
        if not readers:
            raise ShardError("no shards")
        count = readers[0].count
        if any(r.count != count for r in readers):
            raise ShardError("shards disagree on shard count")
        if sorted(r.index for r in readers) != list(range(count)):
            raise ShardError(f"need exactly shards 0..{count - 1}, got "
                             f"{sorted(r.index for r in readers)}")
        seq = 0
        while any(r.pending is not None for r in readers):
            present = [r for r in readers if r.pending is not None and r.pending[1] == seq]
            if not present:
                raise ShardError(f"record {seq} supplied by no shard")
            kinds = {r.pending[0] for r in present}
            if kinds == {b"U"}:
                if len(present) != count:
                    raise ShardError(f"union record {seq} missing from a shard")
                out.write(_union(seq, [r.take() for r in present]))
            elif kinds == {b"S"}:
                if len(present) != count:
                    raise ShardError(f"skeleton record {seq} missing from a shard")
                bodies = [r.take() for r in present]
                if any(b != bodies[0] for b in bodies[1:]):
                    raise ShardError(f"skeleton record {seq} differs between shards")
                out.write(bodies[0])
            elif kinds == {b"L"}:
                if len(present) != 1:
                    raise ShardError(f"leaf record {seq} supplied by {len(present)} shards")
                out.write(present[0].take())
            else:
                raise ShardError(f"record {seq} has mixed kinds")
            seq += 1
        if any(r.records != seq for r in readers):
            raise ShardError(f"shards disagree on record count (merged {seq})")
    finally:
        for r in readers:
            r.fh.close()


def run(shards: int, out: Path, dump_args: list[str], perl: str) -> None:
    env = {k: v for k, v in os.environ.items()
           if k not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}}
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=out.name + ".shards.", dir=out.parent) as tmp:
        paths = [Path(tmp) / f"shard-{i}" for i in range(shards)]
        procs = []
        for i, path in enumerate(paths):
            with open(path, "wb") as fh:
                procs.append(subprocess.Popen(
                    [perl, str(SHARD), "--shard", f"{i}/{shards}", *dump_args],
                    stdout=fh, env=env))
        failed = []
        for i, proc in enumerate(procs):
            # wait4 reports each shard's own peak RSS, which CI logs: the
            # shards run concurrently, so their sum bounds the job's memory.
            _, status, usage = os.wait4(proc.pid, 0)
            proc.returncode = os.waitstatus_to_exitcode(status)
            print(f"merge_dump_shards: shard {i}/{shards} exit={proc.returncode} "
                  f"user={usage.ru_utime:.0f}s maxrss={_rss_mb(usage.ru_maxrss)}MB",
                  file=sys.stderr)
            if proc.returncode != 0:
                failed.append(i)
        if failed:
            raise ShardError(f"dump shards failed: {failed}")
        # Every shard file repeats the skeleton, so this is the job's peak
        # scratch disk (next to the merged output).
        shard_bytes = sum(path.stat().st_size for path in paths)
        print(f"merge_dump_shards: shard files {shard_bytes // (1 << 20)}MB", file=sys.stderr)
        partial = out.with_name(out.name + ".partial")
        with open(partial, "wb") as fh:
            merge(fh, paths)
        partial.replace(out)
    self_usage = resource.getrusage(resource.RUSAGE_SELF)
    print(f"merge_dump_shards: merge maxrss={_rss_mb(self_usage.ru_maxrss)}MB", file=sys.stderr)


def _rss_mb(maxrss: int) -> int:
    # ru_maxrss is bytes on macOS and KiB on Linux.
    return maxrss // (1 << 20) if sys.platform == "darwin" else maxrss // 1024


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    m = sub.add_parser("merge")
    m.add_argument("out", type=Path)
    m.add_argument("shards", type=Path, nargs="+")
    r = sub.add_parser("run")
    r.add_argument("--shards", type=int, required=True)
    r.add_argument("--out", type=Path, required=True)
    r.add_argument("--perl", default=os.environ.get("EXIFTOOL_PERL", "perl"))
    r.add_argument("dump_args", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    try:
        if args.command == "merge":
            with open(args.out, "wb") as fh:
                merge(fh, args.shards)
        else:
            dump_args = args.dump_args[1:] if args.dump_args[:1] == ["--"] else args.dump_args
            if args.shards < 1 or not dump_args:
                parser.error("run needs --shards >= 1 and dump_tables.pl arguments")
            run(args.shards, args.out, dump_args, args.perl)
    except ShardError as exc:
        print(f"merge_dump_shards: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
