#!/usr/bin/env python3
"""Decide what, if anything, an archived fleet patch backlog is still worth.

A backlog accumulates faster than it drains, and the honest question is not
"can these be applied" but "do they close anything that is still open". Those
are different questions and they have different answers, so this asks both,
in the order that costs least:

  1. Does the patch claim REAL ExifTool tags? A patch whose `Tag:` trailers
     name PrintConv display strings was right to be refused whatever else it
     did. Ground truth is `exiftool -f -listx`, never the Perl source alone --
     ExifTool's binary-data tables spell a tag name as the VALUE of a numeric
     key (`10 => 'BlackMaskTopBorder'`), which is character-for-character a
     PrintConv row, so Perl cannot tell them apart and -listx already has.

  2. Are the tags it claims STILL missing? Measure against a freshly built
     comparison of current main. A gap the fleet closed by other means is not
     a reason to revive a patch.

  3. Does it still apply? Last, because it is the most expensive and the
     least informative. Note that `git apply --check --3way` reports
     conflicts and still exits 0 -- it validates that the patch parses, not
     that it merges. Use a real `git apply --3way` on a scratch worktree, or
     cherry-pick from the archive bundle to get a true three-way merge with
     history.

The output that survives a backlog being deleted is the list of gaps it
identified, not the patches themselves.

Usage:
    python3 scripts/triage_patch_backlog.py \
        --archive ~/.oxidex/patch-archive/post-drain \
        --comparison /tmp/comparison.json
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True
import sys
import xml.etree.ElementTree as ET  # nosec B405 -- parses local exiftool output
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from exiftool_oracle import shared as shared_exiftool_oracle  # noqa: E402

SUBJECT_RE = re.compile(r"^Subject: (?:\[PATCH[^\]]*\] )?(\w+)\(([^)]+)\)", re.M)
TAG_RE = re.compile(r"^Tag:\s*(.+)$", re.M)

# comparison.json reports per format; patch subjects use a lowercase scope.
SCOPE_TO_FORMAT = {
    "cr2": "CR2", "dng": "DNG", "nef": "NEF", "rw2": "RW2", "x3f": "X3F",
    "jpeg": "JPEG", "tiff": "TIFF", "pdf": "PDF", "psd": "PSD", "xmp": "XMP",
    "ttf": "TTF", "rar": "RAR", "pe": "PE", "elf": "ELF", "heic": "HEIC",
    "mp4": "MP4", "gif": "GIF", "png": "PNG", "mkv": "MKV", "avi": "AVI",
}


def exiftool_ground_truth(listx_path=None):
    """(tag names, PrintConv display values) as ExifTool itself reports them.

    Both are needed, and conflating them convicts real work. A name absent
    from -listx is NOT thereby fabricated: ExifTool names some tags at
    runtime, so ProcessAPP12's `ucfirst $tag` fallback yields REV, S0, STB1,
    TagQ and TagR, which appear in no table yet are exactly what a correct
    APP12 port emits (see the port landed in #164). Only a name that is a
    tag nowhere AND is a display string ExifTool prints for some numeric key
    is a harvested display value.

    Live dumps come from the PINNED oracle, never a bare `exiftool`: a name
    this triage would convict as "a tag nowhere" may simply be a tag the
    PATH exiftool's release does not have yet.
    """
    if listx_path:
        blob = open(listx_path, "rb").read()
    else:
        blob = subprocess.run(  # nosec B603
            shared_exiftool_oracle().command(["-f", "-listx"]),
            capture_output=True, check=True,
        ).stdout
    names, values = set(), set()
    for table in ET.fromstring(blob).iter("table"):  # nosec B314
        for tag in table.findall("tag"):
            names.add(tag.get("name", ""))
            for val in tag.findall("./values/key/val"):
                if val.get("lang") == "en" and val.text:
                    values.add(val.text.strip())
    return names, values


def open_gaps(comparison_path):
    """Per format, the tag names still missing or still wrong on main."""
    data = json.load(open(comparison_path))
    out = {}
    for fmt, c in data.get("by_format", {}).items():
        missing = {e["name"] for e in c.get("missing_in_oxidex") or []}
        # A wrong value is as open as an absent tag, and patches target both.
        wrong = {
            e["tag_key"].split(":", 1)[-1] for e in c.get("value_differences") or []
        }
        out[fmt] = missing | wrong
    return out, data.get("overall_coverage")


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--archive", required=True, help="dir holding patches/")
    ap.add_argument("--comparison", required=True,
                    help="comparison.json built from CURRENT main")
    ap.add_argument("--listx", help="cached exiftool -f -listx (else runs exiftool)")
    ap.add_argument("--json-out", help="write the verified gap list here")
    args = ap.parse_args()

    real, display_values = exiftool_ground_truth(args.listx)
    gaps, coverage = open_gaps(args.comparison)
    print(f"ground truth: {len(real)} ExifTool tag names")
    print(f"baseline:     {sum(len(v) for v in gaps.values())} open gaps, "
          f"{coverage:.2f}% coverage\n")

    patch_dir = os.path.join(args.archive, "patches")
    claims = defaultdict(set)
    fabricating = []
    n = 0
    for name in sorted(os.listdir(patch_dir)):
        if not name.endswith(".patch"):
            continue
        n += 1
        text = open(os.path.join(patch_dir, name), errors="replace").read()
        m = SUBJECT_RE.search(text)
        if not m:
            continue
        scope = m.group(2)
        tags = {t.strip().split(":", 1)[-1] for t in TAG_RE.findall(text)}
        # Damning only if it is a tag NOWHERE and a display string SOMEWHERE.
        bogus = sorted(
            t for t in tags if t and t not in real and t in display_values
        )
        if bogus:
            fabricating.append((name, bogus[:4]))
        claims[scope] |= {t for t in tags if t in real}

    print(f"{n} patches examined")
    print(f"  {len(fabricating)} claim a PrintConv display string as a tag "
          f"-- refused correctly")
    for name, bogus in fabricating[:6]:
        print(f"      {name}: {bogus}")

    verified = {}
    for scope, tags in claims.items():
        fmt = SCOPE_TO_FORMAT.get(scope, scope.upper())
        still = sorted(tags & gaps.get(fmt, set()))
        if still:
            verified[fmt] = still

    total = sum(len(v) for v in verified.values())
    print(f"\n{total} gaps the backlog identified are STILL OPEN on main "
          f"(all confirmed real ExifTool tags):")
    for fmt in sorted(verified, key=lambda f: -len(verified[f])):
        shown = ", ".join(verified[fmt][:10])
        more = " ..." if len(verified[fmt]) > 10 else ""
        print(f"  {fmt:6s} ({len(verified[fmt]):3d}): {shown}{more}")

    if args.json_out:
        json.dump(verified, open(args.json_out, "w"), indent=1, sort_keys=True)
        print(f"\nwrote {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
