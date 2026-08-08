#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "pyyaml>=6.0",
# ]
# ///
"""
Sync the tag statistics quoted in prose to the numbers actually measured.

Every headline number about OxiDex used to be hand-typed in a dozen places, and
they drifted exactly as you would expect: README, the guide, the architecture
pages and a Rust doc comment all claimed 32,677 tags and "113% of ExifTool"
long after the databases held 16,684 and after that ratio had been shown
misleading. #609 corrected them by hand, which fixes today's number and
guarantees tomorrow's drift.

This script makes them derived. Each rule below names a file, a tightly
anchored pattern, and which measured statistic fills it. `update-coverage-docs`
runs this on every push to main, so a number can be wrong for at most one push.

Why regex rather than an include or a marker comment: the numbers appear in
places that cannot carry either. `docs/index.md` puts one in YAML frontmatter,
`oxidex-tags/src/lib.rs` puts one in a `//!` doc comment that rustdoc renders,
and prose embeds them mid-sentence. An HTML comment is wrong in all three.

Every rule asserts EXACTLY ONE match. A reworded sentence therefore fails the
run loudly instead of silently leaving a stale number behind -- which is the
whole failure mode being fixed, so it must not be reintroduced by the fix.

Usage:
    uv run scripts/sync_tag_stats.py --conformance /tmp/conformance.json
    uv run scripts/sync_tag_stats.py --check      # non-zero if anything stale
"""

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from generate_tag_coverage import (  # noqa: E402
    get_project_root,
    parse_yaml_tags,
    score_row,
)


class Rule:
    """One quoted statistic: where it lives and which number belongs there.

    `pattern` must contain a named group `val` around the number itself, and
    enough surrounding literal text to be unambiguous within that file.
    """

    def __init__(self, path, pattern, stat, note=""):
        self.path = path
        self.pattern = re.compile(pattern)
        self.stat = stat
        self.note = note


# `definitions` is the tag count from the oxidex-tags-* YAML databases. It is a
# definitions count, never a coverage claim -- the surrounding prose in each of
# these files says so explicitly, and that wording should not be softened.
RULES = [
    Rule("README.md",
         r"(?P<pre>It defines )(?P<val>[\d,]+)(?P<post> metadata tags)",
         "definitions"),
    Rule("oxidex-tags/src/lib.rs",
         r"(?P<pre>//! Contains )(?P<val>[\d,]+)(?P<post> metadata tag definitions)",
         "definitions"),
    Rule("docs/index.md",
         r"(?P<pre>    title: )(?P<val>[\d,]+)(?P<post> Metadata Tags)",
         "definitions",
         "YAML frontmatter -- cannot carry an HTML comment marker"),
    Rule("docs/guide/index.md",
         r"(?P<pre>- ✅ )(?P<val>[\d,]+)(?P<post> metadata tag definitions)",
         "definitions"),
    Rule("docs/guide/mcp-integration.md",
         r"(?P<pre>Explore the )(?P<val>[\d,]+)(?P<post> metadata tags defined)",
         "definitions"),
    Rule("docs/architecture/tag-database.md",
         r"(?P<pre>\| Total Tags \| )(?P<val>[\d,]+)(?P<post> \(tag definitions)",
         "definitions"),
    Rule("docs/reference/tag-database.md",
         r"(?P<pre>\*\*Total Tags:\*\* )(?P<val>[\d,]+)(?P<post> tag definitions)",
         "definitions"),
    Rule("docs/reference/formats/index.md",
         r"(?P<pre>\*\*Total Tags:\*\* )(?P<val>[\d,]+)(?P<post> tag definitions)",
         "definitions"),
    Rule("docs/reference/architecture.md",
         r"(?P<pre>\n)(?P<val>[\d,]+)(?P<post> metadata tag definitions automatically)",
         "definitions"),
    Rule("docs/AI_HARNESS.md",
         r"(?P<pre>\| Repo-wide definitions count \| \*\*)(?P<val>[\d,]+)(?P<post> tag definitions\*\*)",
         "definitions"),

    # Extraction stats. Only the coverage workflow can fill these -- they need
    # a conformance run -- so a buildless `--check` skips their VALUE while
    # still asserting their pattern still matches.
    Rule("docs/index.md",
         r"(?P<pre>    details: )(?P<val>[\d.]+%)(?P<post> measured extraction conformance)",
         "score",
         "YAML frontmatter"),
    Rule("docs/index.md",
         r"(?P<pre>conformance against pinned ExifTool, across )(?P<val>\d+)(?P<post> format families)",
         "formats",
         "YAML frontmatter"),
]


def compute_stats(project_root: Path, conformance_path):
    """The authoritative numbers. Definitions always; extraction if measured."""
    domains = parse_yaml_tags(project_root)
    stats = {
        "definitions": f"{sum(d['total_tags'] for d in domains.values()):,}",
        "tables": f"{sum(d['total_tables'] for d in domains.values()):,}",
    }

    if conformance_path:
        try:
            data = json.loads(Path(conformance_path).read_text())
        except (OSError, json.JSONDecodeError) as exc:
            sys.exit(f"could not read conformance JSON {conformance_path}: {exc}")
        per_format = data.get("per_format") or {}
        grand = {}
        formats = 0
        for counts in per_format.values():
            total, _, _ = score_row(counts)
            if not total:
                continue
            formats += 1
            for k in ("files", "matched", "value_diff", "missing", "renames"):
                grand[k] = grand.get(k, 0) + counts.get(k, 0)
        total, score, ceiling = score_row(grand)
        if total:
            stats["score"] = f"{score:.1%}"
            stats["ceiling"] = f"{ceiling:.1%}"
            stats["formats"] = str(formats)
            stats["scored_files"] = str(grand.get("files", 0))
    return stats


def apply(project_root: Path, stats: dict, check_only: bool):
    stale, changed, problems = [], [], []

    for rule in RULES:
        path = project_root / rule.path
        if not path.exists():
            problems.append(f"{rule.path}: file not found")
            continue

        # Match-count first, BEFORE checking whether this run computed the
        # statistic. Rule integrity is the thing `--check` exists to protect,
        # and it must be verified even for extraction stats that a buildless
        # run cannot fill -- otherwise CI's cheap check silently stops
        # covering exactly the rules most likely to rot.
        text = path.read_text()
        matches = list(rule.pattern.finditer(text))
        if len(matches) != 1:
            problems.append(
                f"{rule.path}: pattern matched {len(matches)} times, expected exactly 1"
                f"{' -- ' + rule.note if rule.note else ''}\n"
                f"    The prose was probably reworded. Update the rule in "
                f"scripts/sync_tag_stats.py rather than deleting it, or the "
                f"number silently stops being maintained."
            )
            continue

        want = stats.get(rule.stat)
        if want is None:
            # Extraction stats need a conformance run; skip rather than blank
            # the number out. A definitions-only sync must never erase a
            # measured figure it simply did not compute.
            continue

        got = matches[0].group("val")
        if got == want:
            continue

        stale.append(f"{rule.path}: {rule.stat} is {got}, should be {want}")
        if not check_only:
            path.write_text(rule.pattern.sub(
                lambda m: m.group("pre") + want + m.group("post"), text, count=1))
            changed.append(rule.path)

    return stale, changed, problems


def main():
    ap = argparse.ArgumentParser(description="Sync quoted tag statistics to measured values")
    ap.add_argument("--conformance", "-c",
                    help="conformance.py --json-out file, for extraction stats. "
                         "Without it only definition counts are synced.")
    ap.add_argument("--check", action="store_true",
                    help="report staleness and exit non-zero; change nothing")
    args = ap.parse_args()

    project_root = get_project_root()
    stats = compute_stats(project_root, args.conformance)
    print("Measured values:")
    for k, v in sorted(stats.items()):
        print(f"  {k:14s} {v}")

    stale, changed, problems = apply(project_root, stats, args.check)

    if problems:
        print("\nRule problems:", file=sys.stderr)
        for p in problems:
            print(f"  ✗ {p}", file=sys.stderr)

    if args.check:
        if stale:
            print("\nStale statistics:", file=sys.stderr)
            for s in stale:
                print(f"  ✗ {s}", file=sys.stderr)
            print("\nRun `just sync-tag-stats` to update them.", file=sys.stderr)
    elif changed:
        print(f"\nUpdated {len(changed)} file(s):")
        for c in sorted(set(changed)):
            print(f"  {c}")
    else:
        print("\nAll quoted statistics already current.")

    # A broken rule fails in both modes. Silently not-updating is the exact
    # failure this script exists to prevent.
    if problems or (args.check and stale):
        sys.exit(1)


if __name__ == "__main__":
    main()
