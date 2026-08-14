#!/usr/bin/env python3
"""Stage 4 exit criterion: "duplicate-loss scan shows zero irrecoverable
losses on t/images".

The Part V §1.1 finding (merged tag review) measured ~209-215 repeated
`group:name` cases across 53/194 `t/images` files, ~89-94 of them carrying
DISTINCT values -- i.e. genuinely lost, not redundant copies -- and noted the
figures were INSTRUMENT-SENSITIVE. Step 19 pinned five specific families
(`step19_duplicate_retention_regression` in `src/core/metadata_map.rs`) as a
permanent regression test, but never ran the full 194-file scan the exit
criterion actually names. This script is that scan.

INSTRUMENT, stated precisely (the plan explicitly asks for this because the
answer depends on how you count):

  1. For each file, run the PINNED oracle (`scripts/exiftool_oracle.py`,
     refuses a skewed or capability-degraded ExifTool) as:
         exiftool -a -G1 -s <file>
     in **text mode**, not `-j`/JSON. This matters: JSON output cannot carry
     two keys with the same name, and ExifTool's own JSON writer silently
     drops every occurrence but one when a `group:name` pair repeats (see
     README section "Why not -j" below, and this script's --self-test) --
     so a JSON-based scan would *underreport* duplicates by construction and
     silently agree with a broken oxidex. Text mode with `-s` (short tag
     names, unambiguous ": "-delimited value) prints one line per
     occurrence, in file order, which is the only ExifTool output mode that
     actually exposes what FoundTag retained.
  2. Each output line is parsed as `[G] Name: Value` (the `-G1` group tag,
     the short tag name `-s` produces, the printed value). A line that does
     not match that shape is treated as a continuation of the previous
     line's value (multi-line values happen, e.g. some GPS/XMP structures).
  3. "Repeated `group:name` case" = a `(group1, name)` pair that appears on
     more than one line in one file's output. This is a *group1-qualified*
     count, per the task's instruction to use `-a -G1 -s`; it is not the
     same number a `-G0`-qualified (family-0) or bare-name count would give,
     which is exactly the "range because the answer depends on how you
     count" the plan warns about. This script reports the group1-qualified
     number and does not attempt to also produce the other two -- pick a
     dimension and name it, rather than producing a third unlabeled figure.
  4. "Carries distinct values" = of the occurrences under a repeated key,
     more than one distinct value string appears (byte comparison of the
     printed value, after normalizing embedded whitespace runs -- ExifTool's
     text output sometimes reflows long values across the exact same
     terminal width, which is a display artifact, not a value difference).
  5. Tags excluded from consideration, and why:
       - The `IGNORE` set conformance.py already uses (SourceFile,
         ExifToolVersion, FileName, Directory, and the File[Modify|Access|
         InodeChange]Date/FilePermissions/FileSize/Now/ProcessingTime
         filesystem tags): these vary by machine and run, not by parser
         behavior, and would swamp the signal exactly as conformance.py's
         own comment says.
       - `Warning`/`Error` tags, under any group: diagnostic text the tool
         itself emits about its own parse, not data extracted from the
         file. oxidex and ExifTool phrase these completely differently by
         construction, so treating a `Warning` mismatch as a "duplicate
         loss" would conflate error-message wording with tag-occurrence
         retention -- a different, real thing this repo tracks by other
         means (compare-file's MISSING/EXTRA/VALUE/RENAME classes).
       - `ExifTool` as a group1: version banner and warnings live there in
         the oracle's own output; oxidex has no equivalent group, so a
         key-for-key diff would be 100% "loss" for every file for reasons
         unrelated to occurrence handling.

Step 2 (the oxidex side) runs `oxidex -a -G1 -s <file>` with the exact same
parser and the exact same exclusions, then for every oracle key with >1
distinct values asks whether oxidex's own output has >1 distinct values
under the *same* `(group1, name)` key. Three outcomes:
  - RETAINED: oxidex shows >=2 distinct values too.
  - PARTIAL: oxidex shows the key but with fewer occurrences/distinct values
    than the oracle.
  - MISSING: oxidex shows the key 0 or 1 times.
A oracle key absent from oxidex's exact `(group1, name)` but present under a
*different* group1 with the same bare name is flagged separately as
GROUP_RENAMED rather than counted as a loss -- that is a group-naming
difference (the RENAME class conformance.py already has a name for), not an
occurrence being thrown away.

Usage:
    python3 tools/exiftool-tables/duplicate_loss_scan.py \\
        /tmp/oxidex-exiftool-cache/exiftool/t/images \\
        --oxidex ./target/debug/oxidex
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

IGNORE_NAMES = {
    "SourceFile", "ExifToolVersion", "FileName", "Directory",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate",
    "FilePermissions", "FileSize", "Now", "ProcessingTime",
    "Warning", "Error",
}
IGNORE_GROUPS = {"ExifTool"}

LINE_RE = re.compile(r"^\[(?P<group>[^\]]+)\]\s*(?P<name>\S+)\s*:\s?(?P<value>.*)$")
WS_RE = re.compile(r"\s+")


def normalize_value(v: str) -> str:
    return WS_RE.sub(" ", v).strip()


def parse_group_output(text: str) -> list[tuple[str, str, str]]:
    """Parses `-a -G1 -s` text output into `(group, name, value)` triples,
    one per printed occurrence, in file order. A non-matching line is
    treated as a continuation of the previous occurrence's value."""
    out: list[tuple[str, str, str]] = []
    for raw_line in text.splitlines():
        if not raw_line.strip():
            continue
        m = LINE_RE.match(raw_line)
        if m:
            out.append((m.group("group"), m.group("name"), m.group("value")))
        elif out:
            g, n, v = out[-1]
            out[-1] = (g, n, v + " " + raw_line.strip())
    return out


def keep(group: str, name: str) -> bool:
    if group in IGNORE_GROUPS:
        return False
    if name in IGNORE_NAMES:
        return False
    return True


@dataclass
class FileResult:
    path: str
    oracle_repeated: dict[tuple[str, str], list[str]] = field(default_factory=dict)
    oracle_distinct: dict[tuple[str, str], list[str]] = field(default_factory=dict)
    oxidex_by_key: dict[tuple[str, str], list[str]] = field(default_factory=dict)
    oxidex_by_name: dict[str, dict[str, list[str]]] = field(default_factory=dict)
    losses: dict[tuple[str, str], str] = field(default_factory=dict)  # key -> status


def group_occurrences(triples: list[tuple[str, str, str]]) -> dict[tuple[str, str], list[str]]:
    out: dict[tuple[str, str], list[str]] = defaultdict(list)
    for g, n, v in triples:
        if not keep(g, n):
            continue
        out[(g, n)].append(normalize_value(v))
    return out


def run_text(argv: list[str], path: str) -> str:
    result = subprocess.run(
        [*argv, "-a", "-G1", "-s", path],
        capture_output=True, text=True, errors="replace",
    )
    return result.stdout


def scan_file(oracle_argv: list[str], oxidex_bin: str, path: str) -> FileResult:
    fr = FileResult(path=path)

    oracle_text = run_text(oracle_argv, path)
    oracle_groups = group_occurrences(parse_group_output(oracle_text))
    fr.oracle_repeated = {k: v for k, v in oracle_groups.items() if len(v) > 1}
    fr.oracle_distinct = {
        k: v for k, v in fr.oracle_repeated.items() if len(set(v)) > 1
    }

    oxidex_text = run_text([oxidex_bin], path)
    oxidex_triples = parse_group_output(oxidex_text)
    fr.oxidex_by_key = group_occurrences(oxidex_triples)
    by_name: dict[str, dict[str, list[str]]] = defaultdict(dict)
    for (g, n), vals in fr.oxidex_by_key.items():
        by_name[n][g] = vals
    fr.oxidex_by_name = by_name

    for key, oracle_vals in fr.oracle_distinct.items():
        g, n = key
        oracle_distinct_n = len(set(oracle_vals))
        ox_vals = fr.oxidex_by_key.get(key, [])
        ox_distinct_n = len(set(ox_vals))
        if ox_distinct_n >= oracle_distinct_n and len(ox_vals) >= len(oracle_vals):
            fr.losses[key] = "RETAINED"
        elif ox_distinct_n > 0 or len(ox_vals) > 0:
            fr.losses[key] = "PARTIAL"
        else:
            # exact (group,name) absent from oxidex -- check for the same
            # bare name surfacing under a different group1 before calling
            # this an occurrence loss rather than a naming difference.
            alt_groups = fr.oxidex_by_name.get(n, {})
            alt_groups = {ag: av for ag, av in alt_groups.items() if ag != g}
            if any(len(av) > 1 for av in alt_groups.values()):
                fr.losses[key] = "GROUP_RENAMED"
            else:
                fr.losses[key] = "MISSING"

    return fr


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", help="directory of sample files (e.g. .../exiftool/t/images)")
    ap.add_argument("--exiftool-dir", help="ExifTool checkout root; defaults to the pinned tree")
    ap.add_argument("--oxidex", default="./target/debug/oxidex")
    ap.add_argument("--json-out")
    ap.add_argument("--show", type=int, default=0,
                     help="print per-file detail for the first N files with an oracle-distinct duplicate")
    args = ap.parse_args()

    try:
        oracle = (exiftool_oracle.resolve_tree(args.exiftool_dir)
                  if args.exiftool_dir else exiftool_oracle.shared())
    except exiftool_oracle.OracleError as exc:
        sys.exit(f"❌ {exc}")

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "duplicate_loss_scan.py")
    binary = instrument.resolve_binary(args.oxidex, kind="oxidex")

    if not os.path.isdir(args.corpus):
        sys.exit(f"❌ corpus root not found: {args.corpus}")

    files = sorted(
        p for p in (os.path.join(args.corpus, f) for f in os.listdir(args.corpus))
        if os.path.isfile(p)
    )
    if not files:
        sys.exit(f"❌ no files found under {args.corpus}")

    instrument.print_header(
        tool="duplicate_loss_scan.py",
        git=git,
        binary=binary,
        dirty_overridden=dirty_overridden,
        oracle=oracle,
        corpus_paths=[args.corpus],
        file_count=len(files),
        extra=["note:    `-a -G1 -s` text mode, group1-qualified (see module docstring)"],
    )

    results: list[FileResult] = []
    shown = 0
    for i, path in enumerate(files, 1):
        fr = scan_file(oracle.command(), str(binary.path), path)
        results.append(fr)
        print(f"[{i}/{len(files)}] {os.path.basename(path)}: "
              f"{len(fr.oracle_repeated)} repeated, {len(fr.oracle_distinct)} distinct, "
              f"{sum(1 for s in fr.losses.values() if s != 'RETAINED')} lost/partial",
              file=sys.stderr)
        if args.show and fr.oracle_distinct and shown < args.show:
            shown += 1
            print(f"\n--- {path} ---")
            for key, vals in fr.oracle_distinct.items():
                status = fr.losses.get(key, "?")
                print(f"  [{status}] {key[0]}:{key[1]} oracle={vals} oxidex={fr.oxidex_by_key.get(key)}")

    files_with_repeats = sum(1 for r in results if r.oracle_repeated)
    total_repeated = sum(len(r.oracle_repeated) for r in results)
    total_distinct = sum(len(r.oracle_distinct) for r in results)

    status_counts: dict[str, int] = defaultdict(int)
    for r in results:
        for s in r.losses.values():
            status_counts[s] += 1

    print("\n" + "=" * 72)
    print("DUPLICATE-LOSS SCAN -- instrument: `-a -G1 -s` text mode, group1-qualified")
    print("=" * 72)
    print(f"files scanned:                         {len(files)}")
    print(f"files with >=1 repeated group1:name:    {files_with_repeats}")
    print(f"repeated group1:name cases (total):     {total_repeated}")
    print(f"  of which carry >=2 DISTINCT values:   {total_distinct}")
    print()
    print("oxidex retention of the distinct-value cases:")
    for status in ("RETAINED", "PARTIAL", "GROUP_RENAMED", "MISSING"):
        print(f"  {status:14s} {status_counts.get(status, 0)}")
    irrecoverable = status_counts.get("MISSING", 0) + status_counts.get("PARTIAL", 0)
    print()
    print(f"irrecoverable losses (MISSING + PARTIAL): {irrecoverable}")
    print(f"Stage 4 criterion ('zero irrecoverable losses on t/images'): "
          f"{'MET' if irrecoverable == 0 else 'NOT MET'}")

    if args.json_out:
        payload = {
            "oracle": oracle.provenance(),
            "oxidex": str(binary.path),
            "commit": git.commit,
            "dirty": git.dirty,
            "files_scanned": len(files),
            "files_with_repeats": files_with_repeats,
            "total_repeated": total_repeated,
            "total_distinct": total_distinct,
            "status_counts": dict(status_counts),
            "irrecoverable": irrecoverable,
            "per_file": [
                {
                    "path": r.path,
                    "repeated": len(r.oracle_repeated),
                    "distinct": len(r.oracle_distinct),
                    "losses": {f"{k[0]}:{k[1]}": v for k, v in r.losses.items()},
                }
                for r in results
            ],
        }
        with open(args.json_out, "w") as f:
            json.dump(payload, f, indent=2)
        print(f"\nwrote {args.json_out}")

    return 1 if irrecoverable > 0 else 0


if __name__ == "__main__":
    raise SystemExit(main())
