#!/usr/bin/env python3
"""Bare-name `-TAG` breadth: does oxidex pick ExifTool's winner?

For every file in a corpus (default: the pinned tree's `t/images`), ask the
PINNED oracle for `-a -G1 -s` and collect every tag NAME that the oracle
reports under more than one family-1 group in that file. Those are exactly
the names whose bare `-TAG` answer depends on `FoundTag`'s arbitration
(`lib/Image/ExifTool.pm:9518-9564`: Priority, Avoid, PRIORITY_DIR,
LOW_PRIORITY_DIR and the order tags are found in) followed by
`SetFoundTags`' bare-request rule (`ExifTool.pm:5389-5395`: a bare name
returns the primary key, i.e. `FoundTag`'s winner). For each such
(file, name) compare the oracle's `-s3 -NAME FILE` with oxidex's, byte for
byte after stripping the trailing newline.

Since #957 round 8 `-TagsFromFile`/`copy_metadata` copies each tag by name
using the value `-TAG` reports, so every mismatch here is also a value that
would be written differently from 13.59.

Mismatches are classified so the work can be costed:

  WRONG_WINNER  oxidex answered with a value the oracle prints for this
                name under a different group -- an arbitration/order defect.
  NOT_EMITTED   oxidex printed nothing for the name at all.
  WINNER_ABSENT oxidex answered with another group's value because the
                oracle's winning group is absent from oxidex's `-a -G1`
                output for this name (a missing reader / missing field,
                not an arbitration defect).
  VALUE         oxidex answered with a value the oracle prints for no group
                -- a conversion defect independent of arbitration.

The oracle's `-s3` answers come from ONE ExifTool process per file (commands
separated by `-execute`, each preceded by an `-echo` marker), so each answer
is exactly what a stand-alone `exiftool -s3 -NAME FILE` prints.

Usage:
  python3 tools/exiftool-tables/bare_name_breadth.py --oxidex <bin> \
      [--corpus DIR] [--json-out PATH] [--files a.jpg,b.jpg]
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

TOOL = "bare_name_breadth.py"
LINE_RE = re.compile(r"^\[(?P<group>[^\]]+)\]\s*(?P<name>\S+)\s*:\s?(?P<value>.*)$")
# Names whose value is per-run or per-path, never a property of the file.
IGNORE_NAMES = {
    "SourceFile", "ExifToolVersion", "FileName", "Directory",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate",
    "FilePermissions", "Now", "ProcessingTime", "Warning", "Error",
}
MARK = "@@BARE-NAME-BREADTH@@"


def parse_grouped(text: str) -> list[tuple[str, str, str]]:
    out: list[tuple[str, str, str]] = []
    for line in text.splitlines():
        m = LINE_RE.match(line)
        if m:
            out.append((m.group("group"), m.group("name"), m.group("value")))
        elif out and line.strip():
            g, n, v = out[-1]
            out[-1] = (g, n, v + "\n" + line)
    return out


def run(argv: list[str], timeout: int = 120) -> str:
    return subprocess.run(  # nosec B603 -- list argv
        argv, capture_output=True, text=True, errors="replace", timeout=timeout
    ).stdout


def multi_group_names(rows: list[tuple[str, str, str]]) -> dict[str, list[tuple[str, str]]]:
    groups: dict[str, list[tuple[str, str]]] = {}
    for g, n, v in rows:
        if n in IGNORE_NAMES:
            continue
        groups.setdefault(n, []).append((g, v))
    return {n: gv for n, gv in groups.items() if len({g for g, _ in gv}) > 1}


def oracle_answers(oracle, path: Path, names: list[str]) -> dict[str, str]:
    args: list[str] = []
    for n in names:
        args += ["-echo", f"{MARK}{n}", "-s3", f"-{n}", str(path), "-execute"]
    argv = oracle.command(["-@", "-"])
    proc = subprocess.run(  # nosec B603
        argv, input="\n".join(args) + "\n", capture_output=True, text=True,
        errors="replace", timeout=600,
    )
    answers: dict[str, list[str]] = {}
    current = None
    for line in proc.stdout.split("\n"):
        if line.startswith(MARK):
            current = line[len(MARK):]
            answers[current] = []
        elif current is not None:
            answers[current].append(line)
    return {n: "\n".join(v).rstrip("\n") for n, v in answers.items()}


def oxidex_answer(oxidex: Path, path: Path, name: str) -> str:
    return run([str(oxidex), "-s3", f"-{name}", str(path)]).rstrip("\n")


def classify(name: str, ora: str, ox: str, oracle_rows, ox_rows) -> str:
    if ora == ox:
        return "MATCH"
    if ox == "":
        return "NOT_EMITTED"
    ora_values = {v for _, v in oracle_rows}
    if ox not in ora_values:
        return "VALUE"
    winner_groups = {g for g, v in oracle_rows if v == ora}
    ox_groups = {g for g, n, _ in ox_rows if n == name}
    if winner_groups and not (winner_groups & ox_groups):
        return "WINNER_ABSENT"
    return "WRONG_WINNER"


def measure_file(oracle, oxidex: Path, path: Path) -> list[dict]:
    rows = parse_grouped(run(oracle.command(["-a", "-G1", "-s", str(path)])))
    multi = multi_group_names(rows)
    if not multi:
        return []
    names = sorted(multi)
    ora = oracle_answers(oracle, path, names)
    ox_rows = parse_grouped(run([str(oxidex), "-a", "-G1", "-s", str(path)]))
    out = []
    for n in names:
        o = ora.get(n)
        if o is None:
            raise SystemExit(f"oracle batch lost the answer for {path.name} -{n}")
        x = oxidex_answer(oxidex, path, n)
        out.append({
            "file": path.name,
            "tag": n,
            "oracle": o,
            "oxidex": x,
            "oracle_groups": [g for g, _ in multi[n]],
            "oxidex_groups": [g for g, m, _ in ox_rows if m == n],
            "verdict": classify(n, o, x, multi[n], ox_rows),
        })
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--oxidex", required=True)
    ap.add_argument("--corpus", default=None, help="default: pinned tree t/images")
    ap.add_argument("--json-out", default=None)
    ap.add_argument("--files", default=None, help="comma-separated basenames")
    ap.add_argument("--jobs", type=int, default=4)
    args = ap.parse_args()

    git = instrument.git_state()
    overridden = instrument.refuse_if_dirty(git, TOOL)
    binary = instrument.resolve_binary(args.oxidex)
    oracle = exiftool_oracle.resolve_or_exit()
    if not oracle.verified:
        print("❌ oracle is not verified against the pin; refusing to grade", file=sys.stderr)
        return 2
    corpus = Path(args.corpus) if args.corpus else exiftool_oracle.cache_dir() / "exiftool" / "t" / "images"
    oracle.check_container_support(exiftool_oracle.cache_dir() / "exiftool" / "t" / "images" / "OOXML.docx")
    files = sorted(p for p in corpus.iterdir() if p.is_file())
    if args.files:
        wanted = set(args.files.split(","))
        files = [p for p in files if p.name in wanted]
    instrument.print_header(
        tool=TOOL, git=git, binary=binary, dirty_overridden=overridden, oracle=oracle,
        corpus_paths=[corpus], file_count=len(files),
        extra=["probes: -ver matches the pin; OOXML.docx -> DOCX"],
    )
    if not args.files and len(files) < 150:
        print(f"❌ corpus floor: {len(files)} files < 150", file=sys.stderr)
        return 2

    records: list[dict] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        for recs in pool.map(lambda p: measure_file(oracle, binary.path, p), files):
            records.extend(recs)
    records.sort(key=lambda r: (r["file"], r["tag"]))

    counts: dict[str, int] = {}
    for r in records:
        counts[r["verdict"]] = counts.get(r["verdict"], 0) + 1
    files_with = len({r["file"] for r in records})
    if not args.files and len(records) < 200:
        print(f"❌ pair floor: {len(records)} (file, tag) pairs < 200 -- degraded run?", file=sys.stderr)
        return 2
    for r in records:
        if r["verdict"] != "MATCH":
            print(f"{r['verdict']:<13} {r['file']:<28} -{r['tag']:<28} oracle={r['oracle']!r} oxidex={r['oxidex']!r}")
    print()
    print(f"files with a multi-group name: {files_with}")
    print(f"(file, tag) pairs: {len(records)}")
    for k in ("MATCH", "WRONG_WINNER", "WINNER_ABSENT", "NOT_EMITTED", "VALUE"):
        print(f"  {k:<13} {counts.get(k, 0)}")
    print(f"TOTAL match {counts.get('MATCH', 0)} / {len(records)}; mismatch {len(records) - counts.get('MATCH', 0)}")
    if args.json_out:
        Path(args.json_out).write_text(json.dumps({
            "instrument": TOOL,
            "commit": git.commit,
            "dirty": git.dirty,
            "oxidex": str(binary.path),
            "oracle": oracle.provenance(),
            "corpus": str(corpus),
            "files": len(files),
            "counts": counts,
            "records": records,
        }, indent=1, ensure_ascii=False) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
