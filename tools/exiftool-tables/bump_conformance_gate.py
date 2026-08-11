#!/usr/bin/env python3
"""Grade a bump's before/after conformance.py runs against Step 17's gates.

`bump-exiftool.sh` runs `conformance.py --json-out` twice against the SAME
pinned oracle and the SAME corpus -- once with an OxiDex binary built from
the pre-bump tables, once from the post-bump tables -- and this script reads
the two JSON reports plus the triage report's classified-delta count to
answer the two gates OVERHAUL_OXIDEX_PLAN.md's Step 17 names:

  1. Zero group-qualified VALUE regressions. conformance.py's matching is
     already group-qualified (see its own module docstring and
     AGENTS.md's "memory: compare-file is group-blind -- always the
     group-qualified mode"); a regression here is a (file, tag name) pair
     that disagreed with ExifTool in the AFTER run but did not disagree in
     the BEFORE run -- a tag that used to read correctly (or was silently
     missing/extra) and now reads a specific wrong value.
  2. MISSING growth <= the EXPR+COND+HAND count from the triage report. A
     bump is allowed to surface new gaps -- ExifTool adding tags nobody
     transcribed yet is not a defect -- but only up to the amount of
     genuinely new, classified work the triage report already accounts
     for. Growth beyond that means something regressed silently instead
     of being counted.

Both gates read "regression" relative to the SAME (before, after) pair, not
against some absolute target, so a shrinking-coverage bump (ExifTool
removing tags) is never flagged.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def total_missing(conformance_doc):
    return sum(c.get("missing", 0) for c in conformance_doc.get("per_format", {}).values())


def value_diff_pairs(conformance_doc):
    """{(file, tag_name): (expected, actual, severity)} across all per-file value diffs."""
    out = {}
    for path, rec in conformance_doc.get("per_file", {}).items():
        for name, expected, actual, sev in rec.get("value_diff", []):
            out[(path, name)] = (expected, actual, sev)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("before_json", help="conformance.py --json-out from the pre-bump binary")
    ap.add_argument("after_json", help="conformance.py --json-out from the post-bump binary")
    ap.add_argument("triage_json", help="triage_bump.py --json-out")
    ap.add_argument("--report-out", help="write a markdown gate summary here")
    args = ap.parse_args()

    before = load(args.before_json)
    after = load(args.after_json)
    triage = load(args.triage_json)

    before_missing = total_missing(before)
    after_missing = total_missing(after)
    missing_growth = after_missing - before_missing

    counts = triage.get("counts", {})
    classified_debt = counts.get("EXPR", 0) + counts.get("COND", 0) + counts.get("HAND", 0)

    before_vd = value_diff_pairs(before)
    after_vd = value_diff_pairs(after)
    regressions = sorted(k for k in after_vd if k not in before_vd)

    gate1_ok = len(regressions) == 0
    gate2_ok = missing_growth <= classified_debt

    lines = []
    lines.append("## Conformance gates\n")
    lines.append(f"- MISSING (before): {before_missing}")
    lines.append(f"- MISSING (after): {after_missing}")
    lines.append(f"- MISSING growth: {missing_growth}")
    lines.append(f"- Triage classified debt (EXPR+COND+HAND): {classified_debt}")
    lines.append(f"- **Gate: MISSING growth <= classified debt** -> "
                 f"{'PASS' if gate2_ok else 'FAIL'} ({missing_growth} <= {classified_debt})")
    lines.append(f"- Group-qualified VALUE regressions: {len(regressions)}")
    lines.append(f"- **Gate: zero group-qualified VALUE regressions** -> "
                 f"{'PASS' if gate1_ok else 'FAIL'}")
    if regressions:
        lines.append("\nRegressed (file, tag) pairs:\n")
        for path, name in regressions[:100]:
            e, a, sev = after_vd[(path, name)]
            lines.append(f"- `{path}` `{name}`: expected {e!r}, got {a!r} [{sev}]")
        if len(regressions) > 100:
            lines.append(f"- ... and {len(regressions) - 100} more")
    report = "\n".join(lines) + "\n"

    print(report)
    if args.report_out:
        Path(args.report_out).write_text(report, encoding="utf-8")
        print(f"wrote {args.report_out}")

    return 0 if (gate1_ok and gate2_ok) else 1


if __name__ == "__main__":
    raise SystemExit(main())
