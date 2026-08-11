#!/usr/bin/env python3
"""Capture (or recapture) the verbatim-text snapshots `check_perl_anchors.py`
diffs against, from `perl_anchors_manifest.json` (the list of facts, with NO
snapshot text) against a real ExifTool tree.

Run this once against the pinned tree to produce the committed
`perl_anchors.json` (which DOES carry the snapshot text) -- and again,
deliberately, when a citation's line numbers need to move because ExifTool's
own file shifted lines around it (confirm the fact is still the same fact
first; this script does not know the difference between "the formula moved"
and "the formula changed").

Usage:
    gen_perl_anchor_snapshots.py <exiftool-dist-root>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from check_perl_anchors import extract  # noqa: E402


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 1
    root = Path(sys.argv[1])
    here = Path(__file__).parent
    manifest = json.loads((here / "perl_anchors_manifest.json").read_text())

    out = []
    for anchor in manifest:
        snippet = extract(root, anchor["file"], anchor["start"], anchor["end"])
        out.append({**anchor, "snippet": snippet})

    dest = here / "perl_anchors.json"
    dest.write_text(json.dumps(out, indent=1, sort_keys=False) + "\n")
    print(f"wrote {len(out)} anchor snapshots to {dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
