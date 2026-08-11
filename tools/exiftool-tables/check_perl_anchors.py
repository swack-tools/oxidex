#!/usr/bin/env python3
"""Verbatim-snapshot staleness check for hand-embedded ExifTool FORMULAS and
CONDITIONS (as opposed to data tables -- see `gen_staleness_facts.py` for
those).

Tag-machinery overhaul Step 16, R5 stage 1 / R9(a): a formula like Pentax's
CryptShutterCount XOR (`$val ^ $date ^ (0xffffffff - $time)`, Pentax.pm:6869)
or Sony's ExtraInfo3 NEX-model regex is Perl CODE, not a PrintConv enum map --
it has no `dump_tables.pl` representation to data-diff against (that is
exactly the gap Step 15's expression compiler exists to close at the
architecture level; this script is the narrow, mechanical stopgap for the
specific facts Stage 1 already cites by exact line number).

What this DOES check: `tools/exiftool-tables/perl_anchors.json` snapshots the
verbatim source text at each cited (file, line-range) anchor, captured once
against the pinned ExifTool tree by `gen_perl_anchor_snapshots.py`. This
script re-extracts those same line ranges from a (possibly newer) ExifTool
tree and diffs, byte for byte after trailing-whitespace normalization, against
the committed snapshot. A DRIFT means the cited lines changed since the
snapshot was taken -- the constant moved, the formula changed, or the lines
shifted -- and a human needs to re-read the hand-ported Rust against the new
Perl before the fix can be trusted again.

What this does NOT check: that the Rust formula still computes the same
values as the (possibly-drifted) Perl. That is what each fact's pinned decode
test (Stage 1's "Each fix carries a pinned decode test + omission assertion")
already covers for the CURRENT Perl; this script's job is narrower --
noticing when "current" has moved.

Usage:
    check_perl_anchors.py <exiftool-dist-root> [--manifest perl_anchors.json]

<exiftool-dist-root> is the directory containing `exiftool` and `lib/` (e.g.
/tmp/oxidex-exiftool-cache/exiftool, or a freshly fetched release tarball
during a bump).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def extract(root: Path, file_rel: str, start: int, end: int) -> str:
    """1-indexed, inclusive line range, trailing-whitespace-normalized."""
    path = root / "exiftool" if file_rel == "exiftool" else root / "lib" / file_rel
    if not path.is_file():
        raise FileNotFoundError(str(path))
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    if start < 1 or end > len(lines) or start > end:
        raise ValueError(f"{file_rel}:{start}-{end} out of range ({len(lines)} lines)")
    return "\n".join(line.rstrip() for line in lines[start - 1 : end])


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("exiftool_root", help="dir containing exiftool script + lib/")
    ap.add_argument(
        "--manifest",
        default=str(Path(__file__).with_name("perl_anchors.json")),
        help="committed snapshot file (default: perl_anchors.json next to this script)",
    )
    args = ap.parse_args()

    root = Path(args.exiftool_root)
    manifest = json.loads(Path(args.manifest).read_text())

    drift = []
    errors = []
    for anchor in manifest:
        try:
            current = extract(root, anchor["file"], anchor["start"], anchor["end"])
        except (FileNotFoundError, ValueError) as e:
            errors.append(f"{anchor['id']}: {e}")
            continue
        if current != anchor["snippet"]:
            drift.append(anchor["id"])

    print(f"{len(manifest)} anchors checked against {root}")
    if errors:
        print(f"{len(errors)} anchor(s) could not be read:")
        for e in errors:
            print(f"  ERROR: {e}")
    if drift:
        print(f"{len(drift)} anchor(s) DRIFTED (source text changed since snapshot):")
        for d in drift:
            anchor = next(a for a in manifest if a["id"] == d)
            print(f"  DRIFT: {d} ({anchor['file']}:{anchor['start']}-{anchor['end']}) -- {anchor['note']}")
        print(
            "\nA drifted anchor means the cited ExifTool source changed. Re-read the "
            "current Perl against the Rust hand-port this anchor exists to guard, then "
            "regenerate the snapshot: gen_perl_anchor_snapshots.py <exiftool-dist-root>"
        )
    if not drift and not errors:
        print("all anchors match the pinned snapshot -- no drift detected")

    return 1 if (drift or errors) else 0


if __name__ == "__main__":
    sys.exit(main())
