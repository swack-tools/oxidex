#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Diff ONE file's OxiDex output against the pinned ExifTool oracle.

The corpus-scale harnesses answer "how are we doing"; this answers "what is
wrong with this file", which is the question you actually have while closing a
single gap. `conformance.py` takes a corpus directory, and
`just compare-exiftool-format` re-downloads ExifTool per run -- neither is
something you want to type ten times in an afternoon.

The instrument is the point
---------------------------
Every claim this prints is only evidence about the tools it actually ran, so it
runs the blessed ones and says so in its own output:

* ExifTool comes from ``exiftool_oracle.shared()``, never a bare ``exiftool``.
  That resolver raises on version skew and on a degraded tree (a pinned
  ``-ver`` with, say, no ``Archive::Zip`` still prints the right release while
  every container format silently collapses -- see AGENTS.md).
* OxiDex is this worktree's own binary, resolved and *reported*, so a stale
  ``target/debug`` cannot be mistaken for the change you just made.

Comparison is by bare tag name with the group prefix stripped, matching what the
comparison layer does: ExifTool's ``EXIF:ImageWidth`` and OxiDex's
``IFD0:ImageWidth`` are the same tag. Two groups can therefore collide on one
bare name (``AIFF:Comment`` vs ``ID3:Comment``); ``--show-collisions`` lists
those rather than letting a hidden one flatter the score.

Usage:
    uv run scripts/compare_file.py <path> [--binary PATH] [--show-collisions]
    just compare-file <path>

Exit status is 0 when the file matches the oracle exactly, 1 otherwise, so it
composes into a loop over several samples.
"""

from __future__ import annotations

import argparse
import collections
import re
import subprocess  # nosec B404
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from exiftool_oracle import OracleError, shared as shared_exiftool_oracle  # noqa: E402

#: One tag per line. ExifTool's `-a -G1 -s` prints `[Group]  TagName : value`;
#: OxiDex prints a bare `TagName: value` under the same flags, so the group is
#: OPTIONAL here. Requiring it silently parsed zero OxiDex tags and reported a
#: perfect file as 100% missing -- the same class of instrument error this
#: script exists to stop, committed inside the script itself.
TAG_LINE = re.compile(
    r"^(?:\[(?P<group>\S+)\]\s+)?(?P<name>[A-Za-z0-9_:]+)\s*:\s?(?P<value>.*)$"
)

#: Filesystem facts and the banner. They are not extraction results, they differ
#: by machine and by the second, and counting them would drown the real signal.
SKIP = frozenset({
    "ExifToolVersion", "FileName", "Directory", "FileSize",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate", "FilePermissions",
})

#: Same order attempt_build's builds use -- see model_fix_loop's binary resolver.
BINARY_CANDIDATES = ("target/debug/oxidex", "target/fixloop/oxidex", "target/release/oxidex")


def resolve_binary(repo_root: Path, override: str | None) -> Path:
    if override:
        path = Path(override)
        if not path.is_file():
            sys.exit(f"error: --binary {path} does not exist")
        return path
    for candidate in BINARY_CANDIDATES:
        path = repo_root / candidate
        if path.is_file():
            return path
    sys.exit(
        "error: no oxidex binary found (tried "
        + ", ".join(BINARY_CANDIDATES)
        + ")\n       build one with `cargo build --bin oxidex`"
    )


def parse_tags(text: str) -> tuple[dict[str, str], dict[str, list[str]]]:
    """Bare-name -> value, plus bare-name -> the groups that claimed it.

    First writer wins, matching both tools' own precedence when they print a
    duplicated name under two groups.
    """
    values: dict[str, str] = {}
    groups: dict[str, list[str]] = collections.defaultdict(list)
    for line in text.splitlines():
        match = TAG_LINE.match(line)
        if not match:
            continue
        name = match["name"].split(":")[-1]
        groups[name].append(match["group"] or "?")
        values.setdefault(name, match["value"].strip())
    return values, groups


def run(argv: list[str]) -> str:
    proc = subprocess.run(  # nosec B603
        argv, capture_output=True, text=True, errors="replace", timeout=120
    )
    return proc.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("path", help="the sample file to compare")
    parser.add_argument("--binary", help="oxidex binary to grade (default: this worktree's)")
    parser.add_argument("--show-collisions", action="store_true",
                        help="list bare names claimed by more than one group")
    args = parser.parse_args()

    sample = Path(args.path)
    if not sample.is_file():
        sys.exit(f"error: {sample} does not exist")

    repo_root = Path(__file__).resolve().parent.parent
    binary = resolve_binary(repo_root, args.binary)
    try:
        oracle = shared_exiftool_oracle()
    except OracleError as error:
        # Skew and degradation both land here. Neither is something to grade
        # through: a degraded oracle does not crash, it reports a confident,
        # precisely-formatted, completely wrong number.
        sys.exit(f"error: no usable ExifTool oracle: {error}")

    # Deliberately NOT `-a`. With duplicates kept, a bare name claimed by two
    # groups resolves to whichever line printed first, and that is not the value
    # either tool actually reports for the tag -- on CanonRaw.cr2 it manufactured
    # twelve WRONGs, several of them the correct pair simply swapped
    # (`exiftool='8.0' oxidex='8'` against `exiftool=8 oxidex='8.0'`). Without
    # `-a` each tool prints its own resolved value once, which is what a caller
    # sees and what the comparison layer grades. Collisions are surfaced by a
    # separate `-a` pass under --show-collisions, where they are the subject
    # rather than a contaminant.
    flags = ["-G1", "-s"]
    et_values, _ = parse_tags(run(oracle.command([*flags, str(sample)])))
    ox_values, _ = parse_tags(run([str(binary), *flags, str(sample)]))

    # A bare name several groups claim (AIFF:Comment and ID3:Comment both strip
    # to `Comment`) cannot be scored: each tool resolves it by its own
    # precedence, so a difference says nothing about extraction. Bucket those
    # AMBIGUOUS instead of counting them WRONG -- scoring them invented a defect
    # on AIFF.aif, where both tools emit both values and simply disagree about
    # which one answers the unqualified name.
    _, ambiguous_groups = parse_tags(run(oracle.command(["-a", *flags, str(sample)])))
    ambiguous = {t for t, g in ambiguous_groups.items() if len(g) > 1 and t not in SKIP}

    compared = {
        tag: value
        for tag, value in et_values.items()
        if tag not in SKIP and tag not in ambiguous
    }
    missing = {tag: value for tag, value in compared.items() if tag not in ox_values}
    wrong = {
        tag: (value, ox_values[tag])
        for tag, value in compared.items()
        if tag in ox_values and ox_values[tag] != value
    }

    print(f"file     {sample}")
    print(f"oracle   {' '.join(oracle.command())}")
    print(f"oxidex   {binary}")
    print(
        f"compared {len(compared)} tags -- MISSING {len(missing)}  WRONG {len(wrong)}"
        f"  (AMBIGUOUS {len(ambiguous)}, not scored)"
    )
    for tag, value in sorted(missing.items()):
        print(f"  MISSING {tag}: exiftool={value!r}")
    for tag, (want, got) in sorted(wrong.items()):
        print(f"  WRONG   {tag}: exiftool={want!r} oxidex={got!r}")

    if args.show_collisions:
        for tag in sorted(ambiguous):
            groups = ", ".join(ambiguous_groups[tag])
            print(f"  AMBIGUOUS {tag}: claimed by {groups}")

    return 0 if not missing and not wrong else 1


if __name__ == "__main__":
    sys.exit(main())
