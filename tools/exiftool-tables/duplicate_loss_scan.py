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
     drops every occurrence but one when a `group1:name` pair repeats -- so
     a `-G1 -j` scan would *underreport* duplicates by construction and
     silently agree with a broken oxidex.

     Do not take that on the paragraph's word: `--self-test` re-measures it
     on the pinned tree's own `t/images/ExifTool.jpg`, where `-G1 -j -a`
     prints ONE `File:Comment` key and `-a -G1 -s` prints TWO `Comment`
     lines. It asserts a third case too, because the second one alone
     supports a subtly wrong conclusion: `-G1:4 -j -a` prints BOTH
     occurrences, as `File:Comment` and `File:Copy1:Comment`. The
     suppression is therefore specific to family-4-less JSON -- family 4 is
     ExifTool's copy-identity mechanism -- and not a property of JSON as
     such. Asserting all three keeps that distinction from rotting into
     "JSON can never carry duplicates", which is false.

     Text mode with `-s` (short tag names, unambiguous ": "-delimited
     value) is what this scan uses because it prints one line per
     occurrence, in file order, under the *same* `(group1, name)` key each
     time -- which is the key oxidex's output is compared against. `-G1:4`
     exposes the occurrences too, but renames every one after the first
     (`File:Copy1:...`), so a `-G1:4` scan would have to strip the copy
     prefix back off before it could compare anything, and would be
     measuring its own un-mangling as much as ExifTool's retention.
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

    # verify the text-mode premise above; needs no corpus and no oxidex
    python3 tools/exiftool-tables/duplicate_loss_scan.py --self-test
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


# ---------------------------------------------------------------------------
# --self-test: the module docstring's text-mode premise, as an assertion
#
# Everything above rests on one claim about a tool this script does not own:
# that ExifTool's `-j` writer drops all but one occurrence of a repeated
# `group1:name`. A reader who wants to check that claim should be able to RUN
# it, not re-read the paragraph asserting it -- and if a future ExifTool ever
# changed the behaviour, the instrument should fail loudly here rather than
# keep producing confident text-mode numbers under a stale justification.
#
# Measured only against the PINNED oracle, and only after that oracle has
# proved both that it is the transcribed release and that it is not running
# under a capability-degraded interpreter. A bare `exiftool` off PATH would
# make this self-test evidence about some other ExifTool.
# ---------------------------------------------------------------------------

SELFTEST_IMAGE = "t/images/ExifTool.jpg"
SELFTEST_DOCX = "t/images/OOXML.docx"
SELFTEST_TAG = "Comment"

# The expected three-way result. Case 3 is the load-bearing one: cases 1 and 2
# alone (one key vs two lines) are equally consistent with "JSON can never
# carry duplicate occurrences", which is false -- family 4 carries them fine,
# by renaming the copies. Pinning all three keeps the conclusion the docstring
# draws narrower than the conclusion the first two cases would license.
SELFTEST_CASES = (
    ("-G1 -j -a", ["-G1", "-j", "-a"], "json", ["File:Comment"]),
    ("-a -G1 -s", ["-a", "-G1", "-s"], "text", 2),
    ("-G1:4 -j -a", ["-G1:4", "-j", "-a"], "json", ["File:Comment", "File:Copy1:Comment"]),
)

# Keys are read off the RAW `-j` text rather than through json.loads(): a
# Python dict collapses repeated keys exactly as ExifTool's writer does, so
# parsing would measure the parser and report "1" for both JSON cases no
# matter what ExifTool emitted. That is the same class of instrument error
# AGENTS.md's "Name the instrument" section is about. Anchored at line start,
# so `":` sequences inside a printed value cannot match.
JSON_KEY_RE = re.compile(r'^\s*"((?:[^"\\]|\\.)*)"\s*:')


def json_keys_for_tag(text: str, tag: str) -> list[str]:
    """Every JSON key in `text` whose tag component is `tag`, in file order."""
    return [
        m.group(1)
        for m in (JSON_KEY_RE.match(line) for line in text.splitlines())
        if m and m.group(1).rsplit(":", 1)[-1] == tag
    ]


def text_lines_for_tag(text: str, tag: str) -> list[str]:
    """Every `-a -G1 -s` occurrence of `tag`, via this script's own parser.

    Deliberately reuses parse_group_output() so the self-test also pins the
    thing the scan actually depends on: that the parser keeps both
    occurrences rather than folding them the way JSON does.
    """
    return [f"{g}:{n} = {v}" for g, n, v in parse_group_output(text) if n == tag]


def _capture(argv: list[str]) -> str:
    return subprocess.run(  # nosec B603 -- list-argv, no shell
        argv, capture_output=True, text=True, errors="replace"
    ).stdout


def self_test(oracle) -> int:
    """Assert the pinned oracle's duplicate-key behaviour. 0 = pass."""
    tree = exiftool_oracle.cache_dir() / "exiftool"
    image = tree / SELFTEST_IMAGE
    docx = tree / SELFTEST_DOCX
    failures: list[str] = []

    print("=== instrument: duplicate_loss_scan.py --self-test ===")
    print(f"oracle:  {oracle.display()}")
    print(f"         {oracle.provenance()}")
    print(f"sample:  {image}")
    print(f"tag:     {SELFTEST_TAG}")
    print()

    for label, path in (("sample image", image), ("container probe", docx)):
        if not path.is_file():
            failures.append(f"{label} not found: {path}")
    if failures:
        for f in failures:
            print(f"FAIL  {f}")
        print("\nSELF-TEST FAILED -- the pinned ExifTool checkout is incomplete.")
        return 1

    print("-- oracle probes (both must pass before any measurement) --")

    ver = _capture(oracle.command(["-ver"])).strip()
    pinned = oracle.pinned_version
    ok_ver = pinned is not None and ver == pinned
    print(f"  [{'ok' if ok_ver else 'FAIL'}] -ver                 "
          f"expected {pinned!r} (from .exiftool-version), got {ver!r}")
    if pinned is None:
        failures.append(
            "no pinned source tree found, so there is nothing to compare `-ver` against; "
            "this run cannot show it graded against the transcribed release"
        )
    elif not ok_ver:
        failures.append(
            f"oracle reports ExifTool {ver!r} but the transcriptions are pinned to {pinned!r}; "
            "a skewed release disagrees about sub-table selection and manufactures both "
            "phantom regressions and phantom fixes (see AGENTS.md)"
        )

    # Same assertion as Oracle.check_container_support(), run inline so the
    # observed FileType can be printed as evidence rather than only raised.
    ftype = _capture(oracle.command(["-s3", "-FileType", str(docx)])).strip()
    ok_docx = ftype == "DOCX"
    print(f"  [{'ok' if ok_docx else 'FAIL'}] -s3 -FileType OOXML.docx  "
          f"expected 'DOCX', got {ftype!r}")
    if not ok_docx:
        failures.append(
            f"oracle reports FileType {ftype!r} for {docx} -- expected 'DOCX'. The "
            "interpreter is probably missing Archive::Zip, which silently degrades every "
            "ZIP-container format while `-ver` still prints the right release"
        )

    if failures:
        print()
        for f in failures:
            print(f"FAIL  {f}")
        print("\nSELF-TEST FAILED -- oracle unfit; no measurement attempted.")
        return 1

    print()
    print("-- three-way duplicate-key result --")
    for label, extra, mode, expected in SELFTEST_CASES:
        out = _capture(oracle.command([*extra, str(image)]))
        if mode == "json":
            observed: object = json_keys_for_tag(out, SELFTEST_TAG)
            got_n, want_n = len(observed), len(expected)  # type: ignore[arg-type]
            ok = observed == expected
            detail = f"keys {observed!r}"
            want = f"{want_n} key(s) {expected!r}"
        else:
            occurrences = text_lines_for_tag(out, SELFTEST_TAG)
            observed = occurrences
            got_n, want_n = len(occurrences), expected  # type: ignore[assignment]
            ok = got_n == want_n
            detail = f"{got_n} line(s): " + " | ".join(occurrences)
            want = f"{want_n} line(s)"
        print(f"  [{'ok' if ok else 'FAIL'}] {label:<12} -> {got_n}  {detail}")
        if not ok:
            failures.append(
                f"`{label}` on {image.name}: expected {want}, got {got_n} -- {observed!r}"
            )

    print()
    if failures:
        for f in failures:
            print(f"FAIL  {f}")
        print(
            "\nSELF-TEST FAILED -- the premise this scan's text mode rests on no longer "
            "holds for the pinned oracle. Re-read the module docstring's instrument\n"
            "section before trusting any number this script prints: if ExifTool's `-j`\n"
            "writer no longer suppresses repeated group1:name keys, text mode may no\n"
            "longer be the mode that exposes retained occurrences."
        )
        return 1

    print(
        "SELF-TEST PASSED. `-G1 -j` suppresses the repeated key (1 of 2 occurrences\n"
        "survive), `-a -G1 -s` exposes both, and `-G1:4 -j` exposes both by renaming\n"
        "the copy -- so the suppression is family-4-less JSON's, not JSON's, and this\n"
        "scan's choice of text mode is sound."
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", nargs="?",
                    help="directory of sample files (e.g. .../exiftool/t/images)")
    ap.add_argument("--exiftool-dir", help="ExifTool checkout root; defaults to the pinned tree")
    ap.add_argument("--oxidex", default="./target/debug/oxidex")
    ap.add_argument("--json-out")
    ap.add_argument("--show", type=int, default=0,
                     help="print per-file detail for the first N files with an oracle-distinct duplicate")
    ap.add_argument("--self-test", action="store_true",
                     help="verify the pinned oracle's duplicate-key behaviour (the premise "
                          "this scan's text mode rests on) and exit; needs no corpus or oxidex")
    args = ap.parse_args()

    try:
        oracle = (exiftool_oracle.resolve_tree(args.exiftool_dir)
                  if args.exiftool_dir else exiftool_oracle.shared())
    except exiftool_oracle.OracleError as exc:
        sys.exit(f"❌ {exc}")

    # Before the corpus/binary/dirty-tree checks: the self-test measures
    # ExifTool alone, so no oxidex build and no committed tree state can
    # change its answer, and requiring either would just make the premise
    # harder to check than the prose it replaces.
    if args.self_test:
        return self_test(oracle)

    if not args.corpus:
        ap.error("the following arguments are required: corpus (or pass --self-test)")

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
