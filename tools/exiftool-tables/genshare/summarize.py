#!/usr/bin/env python3
"""Write the committed generated-share result JSON from attribute.py outputs.

This script measures nothing. It reads one `attribute.py --json-out` file per
measured commit (each must hold the union token and the `engine` token) and
writes the record that `tools/docs/render_status.py` reads:

    docs/public/measurements/generated-share-<exiftool>.json

Usage:
  summarize.py --exiftool 13.59 --corpus /tmp/oxidex-exiftool-cache/combined-samples \\
      --files 4238 --date 2026-09-18 \\
      --point <full-sha>=<attr.json> [--point ...]   # oldest first; the LAST is current
      [--label <full-sha>=<text>] --out docs/public/measurements/generated-share-13.59.json

Every number is copied or derived arithmetically from the attribute.py output;
the derivations are the ones README.md names:

  share              = matched_lost / control_matched   (union token)
  direct             = DIRECT + DIRECT_NAME rows
  composite_cascade  = CASCADE rows whose group is Composite or Composite(inferred)
  other_cascade      = the rest of CASCADE
  engine_alone       = matched_lost / control_matched   (`engine` token)

direct + composite_cascade + other_cascade + rename_delta = matched_lost, and
the script refuses a point where that, or attribute.py's reconciliation
residual, does not hold.
"""
from __future__ import annotations

import argparse
import json
import sys

UNION = "engine,legacy-l1,legacy-l2,producers"
ENGINE = "engine"
METHOD_VERSION = "genshare-probe/1"


def pct(part: int, whole: int) -> str:
    return f"{100 * part / whole:.2f}%"


def point(commit: str, path: str) -> dict:
    with open(path, encoding="utf-8") as fh:
        doc = json.load(fh)
    for tok in (UNION, ENGINE):
        if tok not in doc:
            sys.exit(f"{path}: no result for token {tok!r}")
        if doc[tok]["reconciliation_residual"] != 0:
            sys.exit(f"{path}: {tok} reconciliation residual "
                     f"{doc[tok]['reconciliation_residual']} != 0")
    u, e = doc[UNION], doc[ENGINE]
    if u["control_matched"] != e["control_matched"]:
        sys.exit(f"{path}: union and engine were scored against different controls")
    ctl = u["control_matched"]
    b = u["buckets"]
    direct = b.get("DIRECT", 0) + b.get("DIRECT_NAME", 0)
    cascade_groups = u["by_group"].get("CASCADE", {})
    composite = sum(v for g, v in cascade_groups.items() if g.startswith("Composite"))
    other = b.get("CASCADE", 0) - composite
    if direct + composite + other + u["rename_delta"] != u["matched_lost"]:
        sys.exit(f"{path}: components do not sum to matched_lost")
    return {
        "commit": commit,
        "control_matched": ctl,
        "all_generated": {"rows": u["matched_lost"], "share": pct(u["matched_lost"], ctl)},
        "direct": {"rows": direct, "share": pct(direct, ctl)},
        "composite_cascade": {"rows": composite, "share": pct(composite, ctl)},
        "other_cascade": {"rows": other, "share": pct(other, ctl)},
        "rename_delta": u["rename_delta"],
        "engine_alone": {"rows": e["matched_lost"], "share": pct(e["matched_lost"], ctl)},
        "direct_by_group": u["by_group"].get("DIRECT", {}),
    }


def step(a: dict, b: dict) -> dict:
    """What changed between two consecutive points, in rows and in points of share."""
    out = {"from": a["commit"], "to": b["commit"]}
    for key in ("all_generated", "direct", "composite_cascade", "other_cascade", "engine_alone"):
        out[key] = {
            "rows": b[key]["rows"] - a[key]["rows"],
            # `+ 0.0` turns round()'s -0.0 into 0.0
            "pts": round(100 * b[key]["rows"] / b["control_matched"]
                         - 100 * a[key]["rows"] / a["control_matched"], 2) + 0.0,
        }
    out["control_matched"] = b["control_matched"] - a["control_matched"]
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--exiftool", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--files", type=int, required=True)
    ap.add_argument("--date", required=True)
    ap.add_argument("--point", action="append", required=True, metavar="SHA=ATTR_JSON")
    ap.add_argument("--label", action="append", default=[], metavar="SHA=TEXT")
    ap.add_argument("--provenance", action="append", default=[], metavar="SHA=JSON",
                    help="a JSON object recorded as the point's `provenance` (probe commit, "
                         "binary hashes, inertness, census TOTAL lines)")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    labels = dict(spec.split("=", 1) for spec in args.label)
    provenance = {}
    for spec in args.provenance:
        sha, _, path = spec.partition("=")
        with open(path, encoding="utf-8") as fh:
            provenance[sha] = json.load(fh)
    points = []
    for spec in args.point:
        sha, _, path = spec.partition("=")
        if len(sha) != 40:
            sys.exit(f"--point wants a full 40-character sha, got {sha!r}")
        p = point(sha, path)
        if sha in labels:
            p["label"] = labels[sha]
        if sha in provenance:
            p["provenance"] = provenance[sha]
        points.append(p)
    current = points[-1]
    first = points[0]
    doc = {
        "schema": "oxidex-generated-share/1",
        "method_version": METHOD_VERSION,
        "exiftool": args.exiftool,
        "date": args.date,
        "commit": current["commit"],
        "corpus": {"path": args.corpus, "files": args.files},
        "instrument": {
            "census": "tools/exiftool-tables/conformance.py --recursive --json-out, once per "
                      "OXIDEX_PROBE_SILENCE token (tools/exiftool-tables/genshare/census.sh)",
            "probe": "tools/exiftool-tables/genshare/probe.patch (never landed)",
            "attribution": "tools/exiftool-tables/genshare/attribute.py",
            "class_index": ["tools/exiftool-tables/genshare/class-names-eadb5884.json",
                            "tools/exiftool-tables/genshare/class-names-addendum-838.json"],
            "union_token": UNION,
            "engine_token": ENGINE,
            "bound": "floor",
        },
        "current": current,
        "delta_vs_first": {
            "from": first["commit"],
            "all_generated_pts": round(
                100 * current["all_generated"]["rows"] / current["control_matched"]
                - 100 * first["all_generated"]["rows"] / first["control_matched"], 2) + 0.0,
            "engine_alone_pts": round(
                100 * current["engine_alone"]["rows"] / current["control_matched"]
                - 100 * first["engine_alone"]["rows"] / first["control_matched"], 2) + 0.0,
        },
        "steps": [step(a, b) for a, b in zip(points, points[1:])],
        "points": points,
    }
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, indent=2, sort_keys=True)
        fh.write("\n")
    print(f"summarize: wrote {args.out}: {current['all_generated']['share']} at "
          f"{current['commit'][:8]} (engine alone {current['engine_alone']['share']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
