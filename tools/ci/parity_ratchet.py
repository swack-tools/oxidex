#!/usr/bin/env python3
"""Refuse a commit that moves ExifTool parity backwards.

CI already pins the published measurements exactly: the "catalog join" job
recomputes the join and refuses any drift from
`docs/public/measurements/catalog-hydrated-join-<version>.json`. So these
numbers cannot change behind anyone's back.

What that check cannot say is which *way* a change went. It enforces
"identical", not "not worse". The moment a human regenerates the published
measurement -- which is exactly what the drift check tells them to do -- a
regression lands as easily as an improvement, and CI goes green again either
way. Nothing has ever been asked whether the number went down.

This reads the same committed measurements and compares named counts against
floors in `parity_floors.json`. It is a ratchet, not a target: floors move
only when a human runs `raise` and commits the result beside the work that
earned it, which is the point -- it makes a drop a deliberate, reviewable
act instead of a side effect of regenerating a snapshot.

Three directions, because not every number means "more is better":

  at_least  a progress metric -- credited reads, declared writers. May rise
            freely; falling is a regression.
  at_most   a defect metric -- refused rows, withheld coordinates. May fall
            freely; rising is a regression.
  exact     a denominator -- the catalog's own size. Any change at all is a
            refusal, because it means the measurement's basis moved (a new
            ExifTool pin, a different catalog) and every ratio computed
            against it needs re-reading, not silent adoption.

A tracked path that is missing from the report is a refusal, never a pass.
That is the whole point: this repo's instrument doctrine (AGENTS.md, "Name
the instrument, or the measurement is not evidence") is a list of tools that
failed by reporting confident numbers about something they had not measured.
A ratchet that treated an absent key as "no regression" would join it.

Sources are declared in the floors file, so this needs no arguments in CI.
Each is `<path>` or `<path>#<key>` to select a sub-document, and each metric
is named `<source>:<dotted path into counts>`.

Usage:
  parity_ratchet.py check                     # fail on any regression
  parity_ratchet.py report                    # the scoreboard
  parity_ratchet.py raise --note "why"        # re-baseline, for a human
"""
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

FLOORS = pathlib.Path(__file__).with_name("parity_floors.json")
DIRECTIONS = ("at_least", "at_most", "exact")


def dig(counts, path):
    """Follow a dotted path through `counts`; -> (found, value)."""
    node = counts
    for part in path.split("."):
        if not isinstance(node, dict) or part not in node:
            return False, None
        node = node[part]
    if isinstance(node, bool) or not isinstance(node, (int, float)):
        return False, None
    return True, node


def load(path, what):
    try:
        return json.loads(pathlib.Path(path).read_text())
    except FileNotFoundError:
        raise SystemExit(f"parity ratchet: {what} not found: {path}")
    except json.JSONDecodeError as exc:
        raise SystemExit(f"parity ratchet: {what} is not valid JSON: {path}: {exc}")


def read_sources(floors, root):
    """-> {name: counts}, one per declared source.

    A source is `<path>` or `<path>#<key>`, the latter selecting a
    sub-document (the observed snapshot wraps a whole join under
    `observed_join`). Every source must yield a `counts` mapping; a source
    that does not is a refusal, not an empty result.
    """
    loaded = {}
    for name, spec in sorted(floors["sources"].items()):
        path, _, inner = spec.partition("#")
        document = load(root / path, f"source {name!r}")
        if inner:
            document = document.get(inner)
            if not isinstance(document, dict):
                raise SystemExit(
                    f"parity ratchet: source {name!r} has no {inner!r} sub-document in {path}")
        # Validate the shape, but hand back the whole document: metric names
        # carry the full path (`join:counts.write_parity.…`) so they read as
        # the thing they actually name.
        if not isinstance(document.get("counts"), dict):
            raise SystemExit(f"parity ratchet: source {name!r} has no `counts` object ({path})")
        loaded[name] = document
    return loaded


def compare(floor, value, direction):
    """-> None when the metric is acceptable, else a reason string."""
    if direction == "at_least":
        return None if value >= floor else f"fell to {value} from a floor of {floor}"
    if direction == "at_most":
        return None if value <= floor else f"rose to {value} from a ceiling of {floor}"
    if direction == "exact":
        return None if value == floor else f"changed to {value} from a pinned {floor}"
    raise SystemExit(f"parity ratchet: unknown direction {direction!r}")


def split_metric(name, sources):
    """`<source>:<dotted path>` -> (counts, path). An unknown source refuses."""
    source, sep, path = name.partition(":")
    if not sep:
        raise SystemExit(f"parity ratchet: metric {name!r} must be '<source>:<path>'")
    if source not in sources:
        raise SystemExit(f"parity ratchet: metric {name!r} names source {source!r}, "
                         f"which is not declared (have: {', '.join(sorted(sources))})")
    return sources[source], path


def evaluate(sources, floors):
    """-> (regressions, improvements, missing) as lists of readable rows."""
    regressions, improvements, missing = [], [], []
    for name, spec in sorted(floors["metrics"].items()):
        direction = spec.get("direction")
        if direction not in DIRECTIONS:
            raise SystemExit(f"parity ratchet: {name} has direction {direction!r}; "
                             f"expected one of {', '.join(DIRECTIONS)}")
        counts, path = split_metric(name, sources)
        found, value = dig(counts, path)
        if not found:
            missing.append(name)
            continue
        floor = spec["floor"]
        reason = compare(floor, value, direction)
        if reason:
            regressions.append(f"{name}: {reason}")
        elif value != floor:
            improvements.append(f"{name}: {floor} -> {value}")
    return regressions, improvements, missing


def pinned_exiftool(root):
    version = root / ".exiftool-version"
    return version.read_text().strip() if version.is_file() else "(unread)"


def head_commit(root):
    try:
        out = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                             capture_output=True, text=True, timeout=10)
        return out.stdout.strip()[:12] if out.returncode == 0 else "(unread)"
    except (OSError, subprocess.SubprocessError):
        return "(unread)"


def header(floors, sources, root):
    """The instrument line every number here is a claim about."""
    print("=== instrument: parity_ratchet.py ===")
    print(f"repo        : {head_commit(root)}  exiftool {pinned_exiftool(root)}")
    print(f"floors      : {floors['measured_at'].get('commit', '(unrecorded)')} "
          f"/ exiftool {floors['measured_at'].get('exiftool', '(unrecorded)')}")
    for name, spec in sorted(floors["sources"].items()):
        print(f"source      : {name} <- {spec} "
              f"({len(sources[name]['counts'])} count groups)")
    print(f"metrics     : {len(floors['metrics'])} tracked")
    print()


def prepare(args, root):
    floors = load(args.floors, "floors")
    for key in ("sources", "metrics", "measured_at"):
        if key not in floors:
            raise SystemExit(f"parity ratchet: floors file has no {key!r}")
    if not floors["metrics"]:
        # A ratchet tracking nothing passes everything. Say so, rather than
        # reporting OK and letting a green check mean less than it looks.
        raise SystemExit("parity ratchet: floors file tracks no metrics; "
                         "an empty ratchet would pass every regression")
    sources = read_sources(floors, root)
    header(floors, sources, root)
    return floors, sources


def cmd_check(args, root):
    floors, sources = prepare(args, root)
    regressions, improvements, missing = evaluate(sources, floors)

    for row in improvements:
        print(f"  improved  {row}")
    for name in missing:
        print(f"  MISSING   {name}: not present in its source", file=sys.stderr)
    if missing:
        print(f"parity ratchet: {len(missing)} tracked metric(s) absent from their source. "
              f"An absent metric is not a passing metric -- either the source's shape "
              f"changed (re-point the metric) or the measurement stopped running.",
              file=sys.stderr)
    for row in regressions:
        print(f"  REGRESSED {row}", file=sys.stderr)

    if regressions or missing:
        print(f"\nparity ratchet: FAIL -- {len(regressions)} regression(s), "
              f"{len(missing)} missing.\nIf a drop is intended, say so explicitly: "
              f"re-run with `raise --note '<why>'` and commit the new floors "
              f"beside the change that lowered them.", file=sys.stderr)
        return 1
    print(f"parity ratchet: OK -- {len(improvements)} improved, none regressed.")
    return 0


def cmd_raise(args, root):
    floors, sources = prepare(args, root)
    moved, missing = [], []
    for name, spec in sorted(floors["metrics"].items()):
        counts, path = split_metric(name, sources)
        found, value = dig(counts, path)
        if not found:
            missing.append(name)
            continue
        if value != spec["floor"]:
            moved.append(f"{name}: {spec['floor']} -> {value}")
            spec["floor"] = value
    if missing:
        raise SystemExit("parity ratchet: refusing to re-baseline while "
                         f"{len(missing)} tracked metric(s) are absent: "
                         f"{', '.join(missing)}")
    floors["measured_at"] = {"commit": head_commit(root),
                             "exiftool": pinned_exiftool(root),
                             "note": args.note or floors["measured_at"].get("note", "")}
    pathlib.Path(args.floors).write_text(json.dumps(floors, indent=2, sort_keys=True) + "\n")
    for row in moved:
        print(f"  moved     {row}")
    print(f"parity ratchet: re-baselined {len(moved)} metric(s) into {args.floors}. "
          f"Commit it with the change that earned it.")
    return 0


def cmd_report(args, root):
    """Every tracked metric beside its floor -- the scoreboard, in one place."""
    floors, sources = prepare(args, root)
    width = max(len(name) for name in floors["metrics"])
    for name, spec in sorted(floors["metrics"].items()):
        counts, path = split_metric(name, sources)
        found, value = dig(counts, path)
        shown = value if found else "ABSENT"
        share = ""
        denominator = spec.get("share_of")
        if found and denominator:
            d_counts, d_path = split_metric(denominator, sources)
            d_found, total = dig(d_counts, d_path)
            if d_found and total:
                share = f"  {100.0 * value / total:6.2f}% of {total}"
        print(f"  {name:<{width}}  {str(shown):>8}  ({spec['direction']} {spec['floor']}){share}")
        if spec.get("note"):
            print(f"  {'':<{width}}  {spec['note']}")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    for name, help_text in (("check", "fail when a tracked metric regressed"),
                            ("raise", "re-baseline floors to the measured values"),
                            ("report", "print every tracked metric against its floor")):
        one = sub.add_parser(name, help=help_text)
        one.add_argument("--floors", default=str(FLOORS))
        one.add_argument("--root", default=None,
                         help="repository root the sources resolve against "
                              "(default: this script's own repository)")
        if name == "raise":
            one.add_argument("--note", default="", help="why the floors moved")
    args = parser.parse_args(argv)
    root = pathlib.Path(args.root) if args.root else pathlib.Path(__file__).resolve().parents[2]
    return {"check": cmd_check, "raise": cmd_raise, "report": cmd_report}[args.command](args, root)


if __name__ == "__main__":
    sys.exit(main())
